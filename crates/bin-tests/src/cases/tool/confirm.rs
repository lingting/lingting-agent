//! 工具确认链路用例（F-1 ~ F-7）：挂起、不占并发位、不阻塞同会话、批准 / 驳回、重连补发与断连收尾。
//!
//! 口径：挂起态是 `RunStatus::Waiting`（D-P3-027），「在等哪一次调用」以
//! `tool_call_confirm_required` 的 `toolCallId` 与库内的 `PENDING` 行为准（查询接口不返回
//! `PENDING`，见 D-P6-008）；`Run` 契约里没有 `pendingToolCallId` 字段，因此不从 `run/get` 反推。
//!
//! 挂起用例要在服务端**仍然存活**时取 `metrics` 快照，所以快照走
//! `util::server::capture`（`serve_once` 只在收尾时采集一次）。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context as _, Result, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{
    ApiClient, ClientCreatePO, ClientDecision, ClientPostKind, ClientPostPO, ClientStreamParams, RunEventKind,
    RunStatus, SessionCreatePO, ToolCallStatus,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::{audit, case, post_confirm, prompt, serve_with, wait_audit, wait_confirm, wait_status};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::mock::llm::{MockServer, observe};
use crate::util::context::TestContext;
use crate::util::flow::{self, Occupied};
use crate::util::store;

/// 驳回时回灌给模型的固定文本（`agent-tool` 的 `REJECTED_TEXT`）。
const REJECTED_TEXT: &str = "用户拒绝调用该工具";
/// 待确认的调用已被处理（重复确认、过期 `toolCallId`）的固定文案。
const NOT_FOUND: &str = "待确认的工具调用不存在或已处理";
/// 断连后客户端已被移除的固定文案。
const NOT_REGISTERED: &str = "客户端未注册，请重新连接";

/// 确认链路用例。
pub(super) fn cases() -> Vec<Case> {
    vec![
        case("tool.confirm.approve", CaseKind::SelfServed, approve),
        case("tool.confirm.disconnect", CaseKind::SelfServed, disconnect),
        case("tool.confirm.reconnect", CaseKind::SelfServed, reconnect),
        case("tool.confirm.reject", CaseKind::SelfServed, reject),
    ]
}

/// 用例自带配置：`http_request` 需确认；并发上限压到 1，让「挂起不占位」可观测。
fn config_text(host: &str) -> String {
    format!(
        r#"[tools.permissions]
http_request = "confirm"

[tools.http]
allow_hosts = ["{host}"]

[agent]
max_concurrent_runs = 1
run_queue_timeout_ms = 60000
"#
    )
}

/// 只回 200 的假服务端：确认后工具真的被调用时命中数才会增长。
struct OkServer {
    url: String,
    host: String,
    hits: Arc<AtomicU64>,
    accept: tokio::task::JoinHandle<()>,
}

impl OkServer {
    async fn start() -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.context("绑定假服务端失败")?;
        let host = listener.local_addr().context("读取假服务端地址失败")?.to_string();
        let hits = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&hits);
        let accept = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(async move {
                    let mut buffer = [0u8; 1024];
                    let _ = socket.read(&mut buffer).await;
                    let body = "confirmed";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Ok(Self {
            url: format!("http://{host}/run"),
            host,
            hits,
            accept,
        })
    }

    fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }
}

impl Drop for OkServer {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

/// 挂起一次 `http_request` 调用：建会话 → 占用 → 发消息 → 读到确认事件。
async fn suspend(context: &TestContext, stub_url: String) -> Result<(Occupied, String, lib_bin_cli::sdk::RunEvent)> {
    let mut occupied = Occupied::start(context, "tool.confirm").await?;
    let spec = json!({"calls": [{"name": "http_request", "arguments": {"url": stub_url}}]});
    let sent = context
        .client()
        .message_create(lib_bin_cli::sdk::MessageSendPO {
            session_id: occupied.session.id.clone(),
            content: prompt(spec),
        })
        .await
        .map_err(ClientError::from_call)?;
    let run_id = sent.run.id.clone();
    let (confirm, _seen) = wait_confirm(&mut occupied.stream, &run_id).await?;
    Ok((occupied, run_id, confirm))
}

/// 一次 `client_post { TOOL_CONFIRM }` 的固定 400 断言。
async fn expect_confirm_400(context: &TestContext, client_id: &str, tool_call_id: &str) -> Result<String> {
    match context
        .client()
        .client_post(ClientPostPO {
            client_id: client_id.to_string(),
            kind: ClientPostKind::ToolConfirm,
            tool_call_id: Some(tool_call_id.to_string()),
            decision: Some(ClientDecision::Approve),
        })
        .await
        .map_err(ClientError::from_call)
    {
        Err(ClientError::Status { code, message }) => {
            ensure!(code == 400, "重复确认应返回 400，实际 {code}");
            Ok(message)
        }
        other => anyhow::bail!("重复确认应返回固定 400，实际 {other:?}"),
    }
}

/// 批准：确认前不执行、挂起不占并发位、不阻塞同会话、批准后只补一次模型请求。
fn approve(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let stub = OkServer::start().await?;
        let extra = config_text(&stub.host);
        serve_with(context, &mock, &extra, async {
            let (mut occupied, run_id, confirm) = suspend(context, stub.url.clone()).await?;
            let tool_call_id = confirm.tool_call_id.clone().context("确认事件应带 toolCallId")?;
            ensure!(
                confirm.tool_name.as_deref() == Some("http_request"),
                "确认事件应带工具名：{:?}",
                confirm.tool_name
            );
            ensure!(
                confirm
                    .tool_arguments
                    .as_deref()
                    .is_some_and(|args| args.contains("url")),
                "确认事件应带已脱敏的参数：{:?}",
                confirm.tool_arguments
            );

            // F-1：确认前工具没有被调用，库里留下 PENDING 开始标记且没有结果。
            // 查询口径不返回 PENDING（D-P6-008），所以这一条只能从库内行核对；
            // 执行前写入的开始标记同时带 `startedAtMs`（P6A-006）。
            ensure!(stub.hits() == 0, "确认前不应执行工具");
            let pending = store::tool_calls(context, &run_id).await?;
            ensure!(
                pending.len() == 1
                    && pending[0].status == agent_tool::ToolCallStatus::Pending
                    && pending[0].result.is_none()
                    && pending[0].started_at_ms.is_some(),
                "挂起时库里应有一条无结果、带开始标记的 PENDING 审计：{pending:?}"
            );
            ensure!(
                audit(context, &run_id).await?.is_empty(),
                "PENDING 行不应出现在审计查询结果里（D-P6-008）"
            );

            // F-2：Run 回 WAITING（D-P3-027），且不占并发位。
            ensure!(
                wait_status(context, &run_id, RunStatus::Waiting).await? == RunStatus::Waiting,
                "挂起后 run/get 应为 WAITING"
            );
            let snapshot = crate::util::server::capture(context.base_url())
                .await
                .map_err(|reason| anyhow::anyhow!("挂起时的快照未采集：{reason}"))?;
            ensure!(
                snapshot.limit.running_now == 0,
                "挂起的 Run 不应占用并发位：running_now = {}",
                snapshot.limit.running_now
            );

            // F-3：同一会话的第二个 Run 在挂起期间照常执行。
            let before = observe::stats(&mock).await?["chatRequests"]
                .as_u64()
                .unwrap_or_default();
            let (_, second) = occupied.say(context, "挂起期间的第二个 Run").await?;
            ensure!(
                second.terminal.event == RunEventKind::RunCompleted,
                "挂起不应阻塞同会话的后续 Run，实际 {:?}",
                second.terminal.event
            );
            let after = observe::stats(&mock).await?["chatRequests"]
                .as_u64()
                .unwrap_or_default();
            ensure!(after == before + 1, "第二个 Run 应只请求模型一次：{before} → {after}");

            // F-4：批准后重新执行，工具确实被调用，审计转为 OK。
            post_confirm(context, &occupied.client_id, &tool_call_id, ClientDecision::Approve).await?;
            let resumed = flow::drain_run(&mut occupied.stream, &run_id).await?;
            ensure!(
                resumed.terminal.event == RunEventKind::RunCompleted,
                "批准后应以 run_completed 收尾，实际 {:?}（{}）",
                resumed.terminal.event,
                resumed.terminal.error.clone().unwrap_or_default()
            );
            ensure!(stub.hits() == 1, "批准后工具应被执行一次，实际命中 {}", stub.hits());
            let finished = wait_audit(context, &run_id, |records| {
                records
                    .first()
                    .is_some_and(|record| record.status == ToolCallStatus::Ok)
            })
            .await?;
            ensure!(
                finished.len() == 1 && finished[0].status == ToolCallStatus::Ok,
                "批准后审计应转为 OK：{finished:?}"
            );
            let requests = observe::stats(&mock).await?["chatRequests"]
                .as_u64()
                .unwrap_or_default();
            ensure!(
                requests == 3,
                "恢复不应重跑已完成的轮次（共 3 次请求：首轮 + 第二个 Run + 恢复后一轮），实际 {requests}"
            );

            // 重复确认：固定 400，且不产生第二条审计记录。
            let message = expect_confirm_400(context, &occupied.client_id, &tool_call_id).await?;
            ensure!(message == NOT_FOUND, "重复确认的固定文案不符：{message}");
            ensure!(
                audit(context, &run_id).await?.len() == 1,
                "重复确认不应产生第二条审计记录"
            );

            occupied.release(context).await?;
            Ok(vec![
                format!("挂起：WAITING，running_now=0，审计 PENDING（{tool_call_id}）"),
                format!(
                    "批准后：Completed，工具命中 {} 次，模型共 {requests} 次请求",
                    stub.hits()
                ),
                format!("重复确认：{NOT_FOUND}"),
            ])
        })
        .await
    })
}

/// 驳回：回灌固定文本、审计 DENIED，工具始终没有被调用。
fn reject(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let stub = OkServer::start().await?;
        let extra = config_text(&stub.host);
        serve_with(context, &mock, &extra, async {
            let (mut occupied, run_id, confirm) = suspend(context, stub.url.clone()).await?;
            let tool_call_id = confirm.tool_call_id.clone().context("确认事件应带 toolCallId")?;
            post_confirm(context, &occupied.client_id, &tool_call_id, ClientDecision::Reject).await?;
            let resumed = flow::drain_run(&mut occupied.stream, &run_id).await?;
            ensure!(
                resumed.terminal.event == RunEventKind::RunCompleted,
                "驳回后 Run 仍应完成，实际 {:?}",
                resumed.terminal.event
            );
            ensure!(
                resumed.content.contains(REJECTED_TEXT),
                "驳回应把固定文本回灌给模型：{}",
                resumed.content
            );
            ensure!(stub.hits() == 0, "驳回不应执行工具，实际命中 {}", stub.hits());
            let records = wait_audit(context, &run_id, |records| {
                records
                    .first()
                    .is_some_and(|record| record.status == ToolCallStatus::Denied)
            })
            .await?;
            ensure!(
                records.len() == 1
                    && records[0].status == ToolCallStatus::Denied
                    && records[0].result.as_deref() == Some(REJECTED_TEXT),
                "驳回后审计应为 DENIED + 固定文本：{records:?}"
            );
            occupied.release(context).await?;
            Ok(vec![format!("驳回：Completed，审计 DENIED，回灌「{REJECTED_TEXT}」")])
        })
        .await
    })
}

/// 重连补发：下行流建立后，服务端对挂起中的 Run 各补发一次确认事件。
///
/// 构造方式按 P3-027 的口径：**不断连**（不重复注册、不释放占用），只是在下行流建立之前
/// 先让 Run 挂起。
fn reconnect(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let stub = OkServer::start().await?;
        let extra = config_text(&stub.host);
        serve_with(context, &mock, &extra, async {
            let session = context
                .client()
                .session_create(SessionCreatePO {
                    title: Some("tool.confirm.reconnect".to_string()),
                    model: None,
                })
                .await
                .map_err(ClientError::from_call)?;
            let created = context
                .client()
                .client_create(ClientCreatePO {})
                .await
                .map_err(ClientError::from_call)?;
            let client_id = created.client_id.clone();
            flow::occupy(context, &client_id, &session.id).await?;

            let spec = json!({"calls": [{"name": "http_request", "arguments": {"url": stub.url}}]});
            let sent = context
                .client()
                .message_create(lib_bin_cli::sdk::MessageSendPO {
                    session_id: session.id.clone(),
                    content: prompt(spec),
                })
                .await
                .map_err(ClientError::from_call)?;
            let run_id = sent.run.id.clone();
            ensure!(
                wait_status(context, &run_id, RunStatus::Waiting).await? == RunStatus::Waiting,
                "没有下行流时 Run 也应挂起为 WAITING"
            );

            // 建立下行流：服务端此时补发挂起中的确认事件。
            let mut stream = context
                .client()
                .subscribe_client(ClientStreamParams {
                    client_id: client_id.clone(),
                })
                .await?;
            let (confirm, _seen) = wait_confirm(&mut stream, &run_id).await?;
            let tool_call_id = confirm.tool_call_id.clone().context("补发的确认事件应带 toolCallId")?;
            ensure!(
                confirm.tool_name.as_deref() == Some("http_request"),
                "补发的事件应带工具名：{:?}",
                confirm.tool_name
            );

            post_confirm(context, &client_id, &tool_call_id, ClientDecision::Approve).await?;
            let resumed = flow::drain_run(&mut stream, &run_id).await?;
            ensure!(
                resumed.terminal.event == RunEventKind::RunCompleted,
                "补发后确认应让 Run 完成，实际 {:?}",
                resumed.terminal.event
            );
            ensure!(stub.hits() == 1, "确认后工具应执行一次，实际命中 {}", stub.hits());
            flow::release(context, &client_id).await?;
            Ok(vec![format!("重连补发：同一 toolCallId {tool_call_id} 再次确认成功")])
        })
        .await
    })
}

/// 断连收尾：Run 落 CANCELLED、`PENDING` 审计收尾为 CANCELLED、重复确认返回固定 400。
fn disconnect(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let stub = OkServer::start().await?;
        let extra = config_text(&stub.host);
        serve_with(context, &mock, &extra, async {
            let (occupied, run_id, confirm) = suspend(context, stub.url.clone()).await?;
            let tool_call_id = confirm.tool_call_id.clone().context("确认事件应带 toolCallId")?;
            let client_id = occupied.client_id.clone();
            // 断开下行流：断连处理链负责取消挂起的 Run 并反注册客户端。
            drop(occupied);

            let status = wait_status(context, &run_id, RunStatus::Cancelled).await?;
            ensure!(
                status == RunStatus::Cancelled,
                "断连后 Run 应落 CANCELLED，实际 {status:?}"
            );
            let records = wait_audit(context, &run_id, |records| {
                records
                    .first()
                    .is_some_and(|record| record.status == ToolCallStatus::Cancelled)
            })
            .await?;
            ensure!(
                records.len() == 1 && records[0].status == ToolCallStatus::Cancelled,
                "PENDING 审计应随取消收尾为 CANCELLED：{records:?}"
            );
            ensure!(stub.hits() == 0, "断连后工具不应被执行");

            // 重复确认：客户端已被反注册，固定 400（文案来自鉴权第一层）。
            let message = expect_confirm_400(context, &client_id, &tool_call_id).await?;
            ensure!(
                message == NOT_REGISTERED,
                "断连后客户端已移除，文案应为「{NOT_REGISTERED}」，实际「{message}」"
            );

            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok(vec![format!(
                "断连收尾：Run CANCELLED，审计 CANCELLED，重复确认 → {NOT_REGISTERED}"
            )])
        })
        .await
    })
}
