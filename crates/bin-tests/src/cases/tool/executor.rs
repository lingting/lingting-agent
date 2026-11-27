//! 执行器侧用例：四种失败回灌、未授权、结果截断与取消收尾。
//!
//! 覆盖判据：L-2 / L-3 / L-4（参数非法 / 超时 / 失败）、P-1 / P-2 / P-3（未授权）、
//! T-7（截断）、C-4 / C-5（取消）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, ensure};
use lib_bin_cli::sdk::{ApiClient, MessageSendPO, RunCancelPO, RunEventKind, RunGetParams, RunStatus, ToolCallStatus};
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::time::sleep;

use super::{audit, case, events_of, say, serve_with, wait_event};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::mock::llm::{MockServer, observe};
use crate::util::context::TestContext;
use crate::util::flow::{self, Occupied};

/// 参数非法的固定前缀（`agent-tool` 的 `ToolError::Display` 是唯一产点）。
const INVALID_ARGS: &str = "参数不合法：";
/// 执行失败的固定前缀。
const TOOL_FAILED: &str = "工具执行失败：";
/// 未授权的固定文本。
const TOOL_DENIED: &str = "工具未授权：http_request";
/// 超时的固定前缀（上限 300 毫秒由本用例的配置决定）。
const TOOL_TIMEOUT: &str = "工具执行超时（> 300 毫秒）";
/// 截断提示（`agent-tool` 的 `truncate` 固定文案）。
const TRUNCATED: &str = "结果已截断";
/// 超时上限：短到让挂起的请求必然超时，又长到不会误伤正常的本地调用。
const TIMEOUT_PREFIX: &str = r#"[tools]
timeout_ms = 300

[tools.permissions]
http_request = "allow"
"#;
/// 结果上限压到 10 个字符：`current_time` 的输出必然被截断。
const TRUNCATE_EXTRA: &str = r#"[tools]
max_result_chars = 10
"#;
/// 取消用例的超时上限：足够长，取消必须来自用例而不是超时。
const CANCEL_EXTRA: &str = r#"[tools]
timeout_ms = 15000

[tools.permissions]
http_request = "allow"
"#;

/// 执行器侧用例。
pub(super) fn cases() -> Vec<Case> {
    vec![
        case("tool.cancel", CaseKind::SelfServed, cancel),
        case("tool.denied", CaseKind::SelfServed, denied),
        case("tool.failed", CaseKind::SelfServed, failed),
        case("tool.invalid_args", CaseKind::SelfServed, invalid_args),
        case("tool.timeout", CaseKind::SelfServed, timeout),
        case("tool.truncate", CaseKind::SelfServed, truncate),
    ]
}

/// 只接不回的服务端：连接建立后一个字节都不写，用来构造「工具一直等下去」。
///
/// 同时记录「连接是否被对端关闭」：取消后客户端必须断开，否则连接会一直挂着（C-4）。
struct HangServer {
    url: String,
    closed: Arc<AtomicBool>,
    accept: tokio::task::JoinHandle<()>,
}

impl HangServer {
    async fn start() -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("绑定挂起服务端口失败")?;
        let port = listener.local_addr().context("读取挂起服务地址失败")?.port();
        let closed = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&closed);
        let accept = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let flag = Arc::clone(&flag);
                // 每条连接一个探测任务：读到 EOF 即说明客户端已经离开。
                tokio::spawn(async move {
                    let mut buffer = [0u8; 256];
                    loop {
                        match stream.read(&mut buffer).await {
                            Ok(0) => {
                                flag.store(true, Ordering::Relaxed);
                                return;
                            }
                            Ok(_) => {}
                            Err(_) => {
                                flag.store(true, Ordering::Relaxed);
                                return;
                            }
                        }
                    }
                });
            }
        });
        Ok(Self {
            url: format!("http://127.0.0.1:{port}/hang"),
            closed,
            accept,
        })
    }

    /// 客户端是否已经断开（取消后应当断开）。
    fn closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }
}

impl Drop for HangServer {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

/// 参数非法：缺必填字段被拒，固定文案回灌，Run 仍 `COMPLETED`。
fn invalid_args(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        serve_with(context, &mock, "", async {
            let mut occupied = Occupied::start(context, "tool.invalid_args").await?;
            let (sent, drained) = say(
                &mut occupied,
                context,
                json!({"calls": [{"name": "calculator", "arguments": {"expr": "1+1"}}]}),
            )
            .await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "参数非法是工具结果，Run 应完成，实际 {:?}",
                drained.terminal.event
            );
            ensure!(
                drained.content.contains(INVALID_ARGS) && drained.content.contains("expression"),
                "回灌文本应点明参数不合法与字段：{}",
                drained.content
            );
            let records = audit(context, &sent.run.id).await?;
            ensure!(
                records.len() == 1 && records[0].status == ToolCallStatus::InvalidArguments,
                "审计应为 INVALID_ARGUMENTS：{records:?}"
            );
            occupied.release(context).await?;
            Ok(vec![format!("回灌：{}", records[0].result.clone().unwrap_or_default())])
        })
        .await
    })
}

/// 执行失败：除零被拒，固定文案回灌，且不带内部细节。
fn failed(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        serve_with(context, &mock, "", async {
            let mut occupied = Occupied::start(context, "tool.failed").await?;
            let (sent, _) = say(
                &mut occupied,
                context,
                json!({"calls": [{"name": "calculator", "arguments": {"expression": "1/0"}}]}),
            )
            .await?;
            let records = audit(context, &sent.run.id).await?;
            ensure!(
                records.len() == 1 && records[0].status == ToolCallStatus::Error,
                "除零应落 ERROR：{records:?}"
            );
            let text = records[0].result.clone().unwrap_or_default();
            ensure!(
                text.starts_with(TOOL_FAILED) && text.contains("除数不能为零"),
                "失败文案应固定且说明原因：{text}"
            );
            ensure!(
                !text.contains(":\\") && !text.contains("/home"),
                "失败文本不应带内部路径：{text}"
            );
            occupied.release(context).await?;
            Ok(vec![format!("回灌：{text}")])
        })
        .await
    })
}

/// 未授权：默认拒绝的工具不进清单；模型凭空调用时回 `DENIED`，且没有发出任何连接。
fn denied(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let hang = HangServer::start().await?;
        let result = serve_with(context, &mock, "", async {
            let mut occupied = Occupied::start(context, "tool.denied").await?;
            let (sent, drained) = say(
                &mut occupied,
                context,
                json!({"calls": [{"name": "http_request", "arguments": {"url": hang.url.clone()}}]}),
            )
            .await?;

            // P-1：默认配置下 http_request 不在模型可见清单里。
            let recorded = observe::last_request(&mock).await?;
            let names: Vec<&str> = recorded["tools"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|tool| tool["function"]["name"].as_str())
                .collect();
            ensure!(!names.contains(&"http_request"), "默认拒绝的工具不应进清单：{names:?}");

            // P-2：结果文本是固定文案，Run 仍完成。
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "未授权也是工具结果，Run 应完成，实际 {:?}",
                drained.terminal.event
            );
            ensure!(
                drained.content.contains(TOOL_DENIED),
                "回灌文本应为固定文案：{}",
                drained.content
            );

            // P-3：审计 DENIED、耗时是「没触网」的量级。
            let records = audit(context, &sent.run.id).await?;
            ensure!(
                records.len() == 1 && records[0].status == ToolCallStatus::Denied,
                "审计应为 DENIED：{records:?}"
            );
            ensure!(
                records[0].duration_ms < 1000,
                "拒绝不应触网，耗时应是个位数毫秒级：{}ms",
                records[0].duration_ms
            );
            ensure!(!hang.closed(), "被拒绝的调用不应建立任何连接");
            occupied.release(context).await?;
            Ok(vec![format!("清单 {names:?} 不含 http_request；回灌 {TOOL_DENIED}")])
        })
        .await;
        // 挂起服务从未被连接：监听端没有拿到过任何 socket。
        result
    })
}

/// 执行超时：工具等不到响应时由执行器判定 TIMEOUT，固定文案回灌，Run 完成。
fn timeout(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let hang = HangServer::start().await?;
        let host = hang
            .url
            .trim_start_matches("http://")
            .split('/')
            .next()
            .context("无法从挂起服务地址取出 host")?
            .to_string();
        let extra = format!("{TIMEOUT_PREFIX}\n[tools.http]\nallow_hosts = [\"{host}\"]\n");
        serve_with(context, &mock, &extra, async {
            let mut occupied = Occupied::start(context, "tool.timeout").await?;
            let started = Instant::now();
            let (sent, drained) = say(
                &mut occupied,
                context,
                json!({"calls": [{"name": "http_request", "arguments": {"url": hang.url.clone()}}]}),
            )
            .await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "超时也是工具结果，Run 应完成，实际 {:?}",
                drained.terminal.event
            );
            let completed = events_of(&drained, RunEventKind::ToolCallCompleted);
            ensure!(completed.len() == 1, "应有一条工具结束事件");
            ensure!(
                completed[0].tool_status == Some(ToolCallStatus::Timeout),
                "结束事件状态应为 TIMEOUT，实际 {:?}（{}）",
                completed[0].tool_status,
                completed[0].error.clone().unwrap_or_default()
            );
            ensure!(
                completed[0].error.as_deref() == Some(TOOL_TIMEOUT),
                "超时文案应为固定值，实际 {:?}",
                completed[0].error
            );
            let records = audit(context, &sent.run.id).await?;
            ensure!(
                records.len() == 1 && records[0].status == ToolCallStatus::Timeout,
                "审计应为 TIMEOUT：{records:?}"
            );
            // 超时上限 300ms，整条 Run 不该被拖成秒级（还有一轮模型往返）。
            ensure!(
                started.elapsed() < Duration::from_secs(20),
                "超时用例耗时异常：{:?}",
                started.elapsed()
            );
            occupied.release(context).await?;
            Ok(vec![format!(
                "超时：{}，耗时 {}ms",
                TOOL_TIMEOUT, records[0].duration_ms
            )])
        })
        .await
    })
}

/// 结果截断：`max_result_chars` 压到 10 后，回灌文本带固定截断提示。
fn truncate(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        serve_with(context, &mock, TRUNCATE_EXTRA, async {
            let mut occupied = Occupied::start(context, "tool.truncate").await?;
            let (sent, drained) = say(
                &mut occupied,
                context,
                json!({"calls": [{"name": "current_time", "arguments": {}}]}),
            )
            .await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "截断不影响 Run 完成"
            );
            let records = audit(context, &sent.run.id).await?;
            ensure!(records.len() == 1, "应有一条审计记录");
            let text = records[0].result.clone().unwrap_or_default();
            ensure!(
                text.contains(TRUNCATED) && text.contains("原文共"),
                "回灌文本应带固定截断提示：{text}"
            );
            ensure!(
                text.chars().take(10).count() == 10,
                "截断应落在字符边界且保留前 10 个字符：{text}"
            );
            ensure!(
                drained.content.contains(TRUNCATED),
                "模型侧看到的也应是截断文本：{}",
                drained.content
            );
            occupied.release(context).await?;
            Ok(vec![format!("截断结果：{text}")])
        })
        .await
    })
}

/// 取消：取消后不再有新事件、工具状态 `CANCELLED`、连接被关闭、审计不落库。
fn cancel(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let hang = HangServer::start().await?;
        let host = hang
            .url
            .trim_start_matches("http://")
            .split('/')
            .next()
            .context("无法从挂起服务地址取出 host")?
            .to_string();
        let extra = format!("{CANCEL_EXTRA}\n[tools.http]\nallow_hosts = [\"{host}\"]\n");
        serve_with(context, &mock, &extra, async {
            let mut occupied = Occupied::start(context, "tool.cancel").await?;
            let sent = context
                .client()
                .message_create(MessageSendPO {
                    session_id: occupied.session.id.clone(),
                    content: super::prompt(
                        json!({"calls": [{"name": "http_request", "arguments": {"url": hang.url.clone()}}]}),
                    ),
                })
                .await
                .map_err(lib_bin_cli::client::ClientError::from_call)?;
            let run_id = sent.run.id.clone();

            // 工具真正开始执行之后再取消：否则取消可能落在批次启动之前。
            let before = wait_event(&mut occupied.stream, &run_id, RunEventKind::ToolCallStarted).await?;
            ensure!(
                before.iter().any(|event| event.event == RunEventKind::ToolCallStarted),
                "取消前应先收到 tool_call_started"
            );
            context
                .client()
                .run_cancel(RunCancelPO { run_id: run_id.clone() })
                .await
                .map_err(lib_bin_cli::client::ClientError::from_call)?;

            let drained = flow::drain_run(&mut occupied.stream, &run_id).await?;
            let mut events = before;
            events.extend(drained.events.iter().cloned());
            ensure!(
                drained.terminal.event == RunEventKind::RunCancelled,
                "取消应以 run_cancelled 收尾，实际 {:?}",
                drained.terminal.event
            );
            // 已经发出开始事件的调用必须补一条 CANCELLED 的结束事件（D-P3-019）。
            ensure!(
                events.iter().any(|event| event.event == RunEventKind::ToolCallCompleted
                    && event.tool_status == Some(ToolCallStatus::Cancelled)),
                "取消后应补一条 CANCELLED 的工具结束事件：{:?}",
                events
                    .iter()
                    .map(|event| (event.event, event.tool_status))
                    .collect::<Vec<_>>()
            );

            // 无孤儿任务：工具连接被关闭、取消的批次不留终态、Run 落 CANCELLED。
            // 执行前写入的开始标记（P6A-006）会随取消收尾为 `CANCELLED`，因此这里断言的是
            // 「没有终态、没有结果」，而不是「审计为空」——口径变更见 6A 的任务结案说明。
            let closed = wait_for(|| hang.closed()).await;
            ensure!(closed, "取消后应关闭工具的 HTTP 连接");
            let records = audit(context, &run_id).await?;
            ensure!(
                records.len() == 1 && records[0].status == ToolCallStatus::Cancelled && records[0].result.is_none(),
                "取消的批次不留终态：开始标记应收尾为 CANCELLED 且没有结果，实际 {records:?}"
            );
            let run = context
                .client()
                .run_get(RunGetParams { run_id: run_id.clone() })
                .await
                .map_err(lib_bin_cli::client::ClientError::from_call)?;
            ensure!(
                run.status == RunStatus::Cancelled,
                "run/get 应为 CANCELLED，实际 {:?}",
                run.status
            );
            // 1 秒内不应再有新事件或新审计记录。
            let quiet = tokio::time::timeout(Duration::from_secs(1), occupied.stream.recv()).await;
            ensure!(!matches!(quiet, Ok(Ok(Some(_)))), "取消后不应再收到该 Run 的事件");
            ensure!(
                audit(context, &run_id).await?.len() == records.len(),
                "取消后不应新增审计记录"
            );

            // 快照里的「读取瞬间在跑的 Run」应归零（无后台任务残留）。
            drop(occupied);
            Ok(vec![
                "取消收尾：run_cancelled，工具 CANCELLED，连接已关闭，审计只有收尾为 CANCELLED 的开始标记".to_string(),
            ])
        })
        .await
    })
}

/// 轮询到条件成立（上限 10 秒）。
async fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        sleep(Duration::from_millis(50)).await;
    }
    condition()
}
