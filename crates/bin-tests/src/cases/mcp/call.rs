//! 调用语义（P4-018）：`mcp.call`（C-1 / C-9）、`mcp.result`（C-2）、`mcp.call_error`（C-3）、
//! `mcp.timeout`（C-4）、`mcp.cancel`（C-5）、`mcp.denied`（S-5 / C-6）、`mcp.confirm`（C-7）、
//! `mcp.audit`（C-8）。
//!
//! 证据只来自接口响应、客户端事件、Mock 的 `/mock/last-request`、`metrics` 快照与替身的结构化证据
//! 文件：MCP 工具与内置工具走同一个执行器，因此事件、审计与失败回灌的口径完全一致。

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{
    ApiClient, ClientDecision, McpStatusKind, MessageSendPO, RunCancelPO, RunEventKind, RunStatus, ToolCallStatus,
    ToolFileAccess,
};
use serde_json::{Value, json};

use super::{case, prompt, serve_with, visible, wait_status as wait_server};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::cases::tool::{
    audit, events_of, messages, post_confirm, seq_contiguous, wait_audit, wait_confirm, wait_event,
    wait_status as run_status,
};
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::flow::{self, Occupied};
use crate::util::mcp as mcp_util;
use crate::util::support::temp_directory;

/// 图片段的固定占位文本（`agent-mcp` 的 `result` 模块）。
const OMITTED_IMAGE: &str = "[图片内容已省略]";
/// 空内容的固定文案。
const EMPTY_CONTENT: &str = "（MCP 工具没有返回内容）";
/// 工具级错误剧本里的固定文本。
const TOOL_ERROR_TEXT: &str = "磁盘只读，写入被拒绝";
/// 协议级错误的固定文案前缀（`agent-mcp/src/error.rs`）。
const INVALID_RESPONSE: &str = "MCP 服务器返回了无法解析的响应";
/// 执行器超时的固定文案。
const TIMEOUT_TEXT: &str = "工具执行超时（> 500 毫秒）";
/// 驳回工具确认时回灌的固定文本。
const REJECTED_TEXT: &str = "用户拒绝调用该工具";
/// 未授权调用的固定文案前缀。
const DENIED_PREFIX: &str = "工具未授权：";
/// 超时用例的 `[tools] timeout_ms`。
const TIMEOUT_MS: u64 = 500;

/// 调用语义用例。
pub(super) fn cases() -> Vec<Case> {
    vec![
        case("mcp.audit", CaseKind::SelfServed, audit_case),
        case("mcp.call", CaseKind::SelfServed, call),
        case("mcp.call_error", CaseKind::SelfServed, call_error),
        case("mcp.cancel", CaseKind::SelfServed, cancel),
        case("mcp.confirm", CaseKind::SelfServed, confirm),
        case("mcp.denied", CaseKind::SelfServed, denied),
        case("mcp.result", CaseKind::SelfServed, result),
        case("mcp.timeout", CaseKind::SelfServed, timeout),
    ]
}

/// 一条 stdio 替身的 `mcp.toml`。
fn single_stdio(evidence: &Path, scenario: &str, extra: &str) -> Result<String> {
    Ok(format!(
        "{}{}",
        mcp_util::header(5000, 0),
        mcp_util::stdio_server(super::SERVER, true, scenario, evidence, None, extra)?
    ))
}

/// 一轮脚本：只要求模型调用一个工具。
fn one_call(tool: &str, arguments: Value) -> Value {
    json!({ "calls": [{ "name": visible(tool), "arguments": arguments }] })
}

/// C-1 / C-9：调用成功闭环，事件契约不变。
fn call(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-call")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "normal", "")?;
        serve_with(context, &mock, "", &mcp, async {
            wait_server(context, super::SERVER, McpStatusKind::Ready).await?;
            let mut occupied = Occupied::start(context, "mcp.call").await?;
            let spec = one_call("echo", json!({"text": "闭环"}));
            let (sent, drained) = occupied.say(context, &prompt(spec)).await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "调用成功应以 run_completed 收尾，实际 {:?}（{}）",
                drained.terminal.event,
                drained.terminal.error.clone().unwrap_or_default()
            );
            // 模型侧的终答回显最后一条消息，也就是工具结果。
            ensure!(
                drained.content.contains("ECHO") && drained.content.contains("闭环"),
                "终答应基于工具结果：{}",
                drained.content
            );

            // 事件契约：seq 连续、开始与结束按 toolCallId 配对、没有新增事件类型。
            ensure!(seq_contiguous(&drained.events), "事件 seq 应从 0 连续递增");
            let started = events_of(&drained, RunEventKind::ToolCallStarted);
            let completed = events_of(&drained, RunEventKind::ToolCallCompleted);
            ensure!(
                started.len() == 1 && completed.len() == 1,
                "一次调用应各有一条开始与结束事件：{} / {}",
                started.len(),
                completed.len()
            );
            ensure!(
                started[0].tool_call_id == completed[0].tool_call_id
                    && started[0].tool_name.as_deref() == Some(visible("echo").as_str()),
                "事件里的工具名应是可见名：{:?} / {:?}",
                started[0].tool_name,
                completed[0].tool_name
            );
            ensure!(
                completed[0].tool_status == Some(ToolCallStatus::Ok),
                "工具结束状态应为 OK：{:?}",
                completed[0].tool_status
            );
            for event in &drained.events {
                let wire = serde_json::to_value(event.event)?;
                ensure!(!wire.to_string().contains("mcp"), "不应出现 MCP 专属事件类型：{wire}");
            }

            // 审计：可见名 + OK；轨迹里 TOOL 消息带同名。
            let records = audit(context, &sent.run.id).await?;
            ensure!(
                records.len() == 1
                    && records[0].tool_name == visible("echo")
                    && records[0].status == ToolCallStatus::Ok,
                "审计应记录一次 OK 的调用：{records:?}"
            );
            let stored = messages(context, &occupied.session.id).await?;
            let tool_message = stored
                .iter()
                .find(|message| message.tool_name.as_deref() == Some(visible("echo").as_str()))
                .context("历史里应有 TOOL 消息")?;
            ensure!(
                tool_message.content.contains("ECHO"),
                "TOOL 消息应携带工具结果：{tool_message:?}"
            );

            // 第二轮请求携带配对的 tool 结果（C-1 的闭环证据）。
            let second = crate::mock::llm::observe::last_request(&mock).await?;
            let history = second["messages"].as_array().cloned().unwrap_or_default();
            ensure!(
                history.iter().any(|message| {
                    message["role"] == Value::String("tool".to_owned())
                        && message["content"].as_str().is_some_and(|text| text.contains("ECHO"))
                }),
                "第二轮请求应携带工具结果：{history:?}"
            );
            let recorded = mcp_util::evidence(&evidence)?;
            let executed = mcp_util::call_of(&recorded, "echo").context("替身应收到一次 echo 调用")?;
            ensure!(
                executed["arguments"].as_str().is_some_and(|text| text.contains("闭环")),
                "替身收到的参数应是模型给出的参数：{executed}"
            );
            occupied.release(context).await?;
            notes.push(format!("闭环：{}", drained.content));
            Ok(())
        })
        .await?;
        Ok(notes)
    })
}

/// C-2：四种结果形态都有确定文本，且总文本非空。
fn result(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-result")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "results", "")?;
        let notes = serve_with(context, &mock, "", &mcp, async {
            wait_server(context, super::SERVER, McpStatusKind::Ready).await?;
            let mut occupied = Occupied::start(context, "mcp.result").await?;
            let spec = json!({"calls": [
                {"name": visible("text_only"), "arguments": {}},
                {"name": visible("text_image"), "arguments": {}},
                {"name": visible("structured_only"), "arguments": {}},
                {"name": visible("empty"), "arguments": {}},
            ]});
            let (sent, drained) = occupied.say(context, &prompt(spec)).await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "四种结果形态都不影响 Run 收尾：{:?}",
                drained.terminal.event
            );
            let records = audit(context, &sent.run.id).await?;
            ensure!(records.len() == 4, "四次调用应有四条审计：{records:?}");
            let text_of = |name: &str| -> String {
                records
                    .iter()
                    .find(|record| record.tool_name == visible(name))
                    .and_then(|record| record.result.clone())
                    .unwrap_or_default()
            };

            let text_only = text_of("text_only");
            ensure!(text_only == "纯文本结果", "纯文本应原样回灌：{text_only}");

            let text_image = text_of("text_image");
            ensure!(
                text_image.contains(crate::mock::mcp::IMAGE_TEXT) && text_image.contains(OMITTED_IMAGE),
                "文本 + 图片应保留文本并把图片换成占位：{text_image}"
            );
            ensure!(
                !text_image.contains(&crate::mock::mcp::IMAGE_DATA[..16]),
                "图片的 base64 不应进上下文：{text_image}"
            );

            let structured = text_of("structured_only");
            let parsed: Value = serde_json::from_str(&structured).context("结构化结果应是 JSON 文本")?;
            ensure!(
                parsed == crate::mock::mcp::structured_content(),
                "只有结构化内容时应退到 structuredContent 的 JSON：{structured}"
            );

            let empty = text_of("empty");
            ensure!(empty == EMPTY_CONTENT, "空内容应使用固定文案：{empty}");

            ensure!(
                records.iter().all(|record| record.status == ToolCallStatus::Ok),
                "四种形态都是成功调用：{records:?}"
            );
            // 模型侧的第二轮请求携带全部四条结果：图片段是占位文本，其余文本原样。
            let second = crate::mock::llm::observe::last_request(&mock).await?;
            let history = serde_json::to_string(&second["messages"])?;
            for expected in [
                "纯文本结果".to_owned(),
                crate::mock::mcp::IMAGE_TEXT.to_owned(),
                OMITTED_IMAGE.to_owned(),
                EMPTY_CONTENT.to_owned(),
            ] {
                ensure!(history.contains(&expected), "第二轮请求应携带 `{expected}`：{history}");
            }
            ensure!(
                !history.contains(&crate::mock::mcp::IMAGE_DATA[..16]),
                "图片的 base64 不应进上下文：{history}"
            );
            occupied.release(context).await?;
            Ok(vec![
                format!("纯文本：{text_only}"),
                format!("文本 + 图片：{text_image}"),
                format!("仅结构化：{structured}"),
                format!("空内容：{empty}"),
            ])
        })
        .await?;
        Ok(notes)
    })
}

/// C-3：工具级错误与协议级错误都变成可读文本，Run 正常结束。
fn call_error(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-call-error")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "error", "")?;
        let notes = serve_with(context, &mock, "", &mcp, async {
            wait_server(context, super::SERVER, McpStatusKind::Ready).await?;
            let mut occupied = Occupied::start(context, "mcp.call_error").await?;
            let spec = json!({"calls": [
                {"name": visible("fail_tool"), "arguments": {}},
                {"name": visible("protocol_error"), "arguments": {}},
            ]});
            let (sent, drained) = occupied.say(context, &prompt(spec)).await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "两种失败都是工具结果，Run 应完成：{:?}",
                drained.terminal.event
            );
            let records = audit(context, &sent.run.id).await?;
            ensure!(records.len() == 2, "两次失败调用应有两条审计：{records:?}");
            ensure!(
                records.iter().all(|record| record.status == ToolCallStatus::Error),
                "两条审计都应是 ERROR：{records:?}"
            );
            // 审计的顺序不保证等于模型给出的调用顺序（同一轮是并行执行的），按可见名取。
            let result_of = |name: &str| -> String {
                records
                    .iter()
                    .find(|record| record.tool_name == visible(name))
                    .and_then(|record| record.result.clone())
                    .unwrap_or_default()
            };
            let tool_error = result_of("fail_tool");
            ensure!(
                tool_error.contains(TOOL_ERROR_TEXT),
                "工具级错误的文本应回灌：{tool_error}"
            );
            let protocol_error = result_of("protocol_error");
            ensure!(
                protocol_error.contains(INVALID_RESPONSE) && protocol_error.contains(super::SERVER),
                "协议级错误应归一成固定文案 + server id：{protocol_error}"
            );
            ensure!(
                !protocol_error.contains("mock 协议级错误"),
                "协议原文不应交给模型：{protocol_error}"
            );
            // 模型侧的终答回显最后一条结果；两段文本都在第二轮的请求历史里。
            ensure!(
                drained.content.contains(INVALID_RESPONSE),
                "终答应基于最后一条工具结果：{}",
                drained.content
            );
            let second = crate::mock::llm::observe::last_request(&mock).await?;
            let history = serde_json::to_string(&second["messages"])?;
            ensure!(
                history.contains(TOOL_ERROR_TEXT) && history.contains(INVALID_RESPONSE),
                "第二轮请求应携带两段可读文本：{history}"
            );
            ensure!(!history.contains("mock 协议级错误"), "协议原文不应交给模型：{history}");
            // 失败之后同一 Run 的下一轮仍可继续（这里由模型的第二轮回复证明）。
            occupied.release(context).await?;
            Ok(vec![
                format!("工具级错误：{tool_error}"),
                format!("协议级错误：{protocol_error}"),
            ])
        })
        .await?;
        Ok(notes)
    })
}

/// C-4：超过 `[tools] timeout_ms` 判 `TIMEOUT`，Run 继续。
fn timeout(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-timeout")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "slow", "")?;
        let config_extra = format!("[tools]\ntimeout_ms = {TIMEOUT_MS}\n");
        let notes = serve_with(context, &mock, &config_extra, &mcp, async {
            wait_server(context, super::SERVER, McpStatusKind::Ready).await?;
            let mut occupied = Occupied::start(context, "mcp.timeout").await?;
            let started = Instant::now();
            let (sent, drained) = occupied.say(context, &prompt(one_call("slow", json!({})))).await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "超时也是工具结果，Run 应完成：{:?}",
                drained.terminal.event
            );
            let completed = events_of(&drained, RunEventKind::ToolCallCompleted);
            ensure!(
                completed.len() == 1 && completed[0].tool_status == Some(ToolCallStatus::Timeout),
                "结束事件应是 TIMEOUT：{:?}",
                completed.first().map(|event| event.tool_status)
            );
            ensure!(
                completed[0].error.as_deref() == Some(TIMEOUT_TEXT),
                "超时文案应是固定值：{:?}",
                completed[0].error
            );
            let records = audit(context, &sent.run.id).await?;
            ensure!(
                records.len() == 1 && records[0].status == ToolCallStatus::Timeout,
                "审计应为 TIMEOUT：{records:?}"
            );
            ensure!(
                started.elapsed() < Duration::from_secs(20),
                "超时不应把整条 Run 拖成秒级：{:?}",
                started.elapsed()
            );

            // Run 继续：同一会话的下一次调用正常执行。
            let (_, second) = occupied
                .say(context, &prompt(one_call("echo", json!({"text": "继续"}))))
                .await?;
            ensure!(
                second.terminal.event == RunEventKind::RunCompleted && second.content.contains("继续"),
                "超时之后的下一次调用应正常：{:?} {}",
                second.terminal.event,
                second.content
            );
            let recorded = mcp_util::evidence(&evidence)?;
            ensure!(
                mcp_util::call_of(&recorded, "slow").is_some(),
                "替身应收到过 slow 调用：{recorded}"
            );
            occupied.release(context).await?;
            Ok(vec![
                format!("超时：{TIMEOUT_TEXT}，耗时 {}ms", records[0].duration_ms),
                "超时之后的下一次调用正常完成".to_owned(),
            ])
        })
        .await?;
        Ok(notes)
    })
}

/// C-5：调用中取消 → `CANCELLED`，无残留后台任务。
fn cancel(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-cancel")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "slow", "")?;
        let notes = serve_with(context, &mock, "[tools]\ntimeout_ms = 15000\n", &mcp, async {
            wait_server(context, super::SERVER, McpStatusKind::Ready).await?;
            let mut occupied = Occupied::start(context, "mcp.cancel").await?;
            let sent = context
                .client()
                .message_create(MessageSendPO {
                    session_id: occupied.session.id.clone(),
                    content: prompt(one_call("slow", json!({}))),
                })
                .await
                .map_err(ClientError::from_call)?;
            let run_id = sent.run.id.clone();

            // 工具真正开始执行之后再取消，否则取消可能落在批次启动之前。
            let before = wait_event(&mut occupied.stream, &run_id, RunEventKind::ToolCallStarted).await?;
            let started = Instant::now();
            context
                .client()
                .run_cancel(RunCancelPO { run_id: run_id.clone() })
                .await
                .map_err(ClientError::from_call)?;
            let drained = flow::drain_run(&mut occupied.stream, &run_id).await?;
            let mut events = before;
            events.extend(drained.events.iter().cloned());
            ensure!(
                drained.terminal.event == RunEventKind::RunCancelled,
                "取消应以 run_cancelled 收尾：{:?}",
                drained.terminal.event
            );
            ensure!(
                events.iter().any(|event| event.event == RunEventKind::ToolCallCompleted
                    && event.tool_status == Some(ToolCallStatus::Cancelled)),
                "已发出开始事件的调用应补一条 CANCELLED 的结束事件：{:?}",
                events
                    .iter()
                    .map(|event| (event.event, event.tool_status))
                    .collect::<Vec<_>>()
            );
            ensure!(
                started.elapsed() < Duration::from_secs(10),
                "取消应立即生效（不是等超时）：{:?}",
                started.elapsed()
            );
            ensure!(
                audit(context, &run_id).await?.is_empty(),
                "取消的批次不落库：审计应为空"
            );
            let run = context
                .client()
                .run_get(lib_bin_cli::sdk::RunGetParams { run_id: run_id.clone() })
                .await
                .map_err(ClientError::from_call)?;
            ensure!(
                run.status == RunStatus::Cancelled,
                "run/get 应为 CANCELLED：{:?}",
                run.status
            );
            ensure!(
                mcp_util::call_of(&mcp_util::evidence(&evidence)?, "slow").is_some(),
                "替身应收到过 slow 调用（取消发生在调用之后）"
            );
            occupied.release(context).await?;
            Ok(vec![
                format!(
                    "取消：RunCancelled，工具补 CANCELLED 事件，耗时 {:?}",
                    started.elapsed()
                ),
                "取消的批次不落库".to_owned(),
            ])
        })
        .await?;
        Ok(notes)
    })
}

/// S-5 / C-6：`deny` 的工具对模型不可见（不进请求体），执行期回 `DENIED`。
fn denied(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-denied")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "normal", "permission = 'deny'\n")?;
        let notes = serve_with(context, &mock, "", &mcp, async {
            wait_server(context, super::SERVER, McpStatusKind::Ready).await?;
            // `deny` 的条目仍在注册表里（执行期要能查到并回 DENIED），但对模型不可见：
            // 下面的请求体断言才是 S-5 的判据。
            let listed = super::tools(context, Some(super::SERVER)).await?;
            ensure!(
                listed.iter().all(|tool| tool.tool_name.starts_with("mcp__")),
                "清单里的都应是从替身发现的工具：{listed:?}"
            );

            let mut occupied = Occupied::start(context, "mcp.denied").await?;
            let (sent, drained) = occupied
                .say(context, &prompt(one_call("read_file", json!({"path": "x"}))))
                .await?;
            // 模型可见清单里没有它，但模型仍可能凭空构造这个名字。
            let requested = super::request_tool_names(&super::request_tools(&mock).await?);
            ensure!(
                !requested.contains(&visible("read_file")),
                "`deny` 的工具不应出现在请求体：{requested:?}"
            );
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "未授权也是工具结果，Run 应完成：{:?}",
                drained.terminal.event
            );
            let expected = format!("{DENIED_PREFIX}{}", visible("read_file"));
            ensure!(
                drained.content.contains(&expected),
                "回灌文本应为固定文案：{}",
                drained.content
            );
            let records = audit(context, &sent.run.id).await?;
            ensure!(
                records.len() == 1
                    && records[0].status == ToolCallStatus::Denied
                    && records[0].result.as_deref() == Some(expected.as_str()),
                "审计应为 DENIED + 固定文案：{records:?}"
            );
            let recorded = mcp_util::evidence(&evidence)?;
            ensure!(
                mcp_util::calls(&recorded).is_empty(),
                "被拒绝的调用不应到达替身：{recorded}"
            );
            occupied.release(context).await?;
            Ok(vec![
                format!("请求体不含 deny 工具；回灌 {expected}"),
                "替身没有收到任何调用".to_owned(),
            ])
        })
        .await?;
        Ok(notes)
    })
}

/// C-7：`confirm` → 挂起（`WAITING`、不占并发位）→ 批准后执行、驳回后回灌拒绝文本。
fn confirm(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-confirm")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "normal", "permission = 'confirm'\n")?;

        // 一、批准：挂起 → 批准 → 执行一次。
        serve_with(context, &mock, "", &mcp, async {
            wait_server(context, super::SERVER, McpStatusKind::Ready).await?;
            let mut occupied = Occupied::start(context, "mcp.confirm").await?;
            let sent = context
                .client()
                .message_create(MessageSendPO {
                    session_id: occupied.session.id.clone(),
                    content: prompt(one_call("read_file", json!({"path": "a.txt"}))),
                })
                .await
                .map_err(ClientError::from_call)?;
            let run_id = sent.run.id.clone();
            let (confirm, _seen) = wait_confirm(&mut occupied.stream, &run_id).await?;
            let tool_call_id = confirm.tool_call_id.clone().context("确认事件应带 toolCallId")?;
            ensure!(
                confirm.tool_name.as_deref() == Some(visible("read_file").as_str()),
                "确认事件的工具名应是可见名：{:?}",
                confirm.tool_name
            );

            // 挂起时工具还没有被调用；Run 进 WAITING，且不占并发位。
            ensure!(
                mcp_util::calls(&mcp_util::evidence(&evidence)?).is_empty(),
                "确认前不应执行工具"
            );
            ensure!(
                run_status(context, &run_id, RunStatus::Waiting).await? == RunStatus::Waiting,
                "挂起后 run/get 应为 WAITING"
            );
            let snapshot = crate::util::server::capture(context.base_url())
                .await
                .map_err(|reason| anyhow::anyhow!("挂起时的快照未采集：{reason}"))?;
            ensure!(
                snapshot.limit.running_now == 0,
                "挂起的 Run 不应占并发位：running_now = {}",
                snapshot.limit.running_now
            );
            let pending = audit(context, &run_id).await?;
            ensure!(
                pending.len() == 1 && pending[0].status == ToolCallStatus::Pending,
                "挂起时应有一条 PENDING 审计：{pending:?}"
            );

            post_confirm(context, &occupied.client_id, &tool_call_id, ClientDecision::Approve).await?;
            let resumed = flow::drain_run(&mut occupied.stream, &run_id).await?;
            ensure!(
                resumed.terminal.event == RunEventKind::RunCompleted,
                "批准后应以 run_completed 收尾：{:?}",
                resumed.terminal.event
            );
            let executed = mcp_util::wait_evidence(&evidence, |value| !mcp_util::calls(value).is_empty()).await?;
            ensure!(
                mcp_util::call_of(&executed, "read_file").is_some(),
                "批准后工具应被执行：{executed}"
            );
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
            occupied.release(context).await?;
            Ok::<(), anyhow::Error>(())
        })
        .await?;
        notes.push(format!(
            "批准：WAITING（running_now=0）→ OK，工具名 {}",
            visible("read_file")
        ));

        // 二、驳回：回灌固定文本、审计 DENIED、工具不被调用。
        serve_with(context, &mock, "", &mcp, async {
            wait_server(context, super::SERVER, McpStatusKind::Ready).await?;
            let mut occupied = Occupied::start(context, "mcp.confirm").await?;
            let sent = context
                .client()
                .message_create(MessageSendPO {
                    session_id: occupied.session.id.clone(),
                    content: prompt(one_call("write_log", json!({"path": "b.txt"}))),
                })
                .await
                .map_err(ClientError::from_call)?;
            let run_id = sent.run.id.clone();
            let (confirm, _seen) = wait_confirm(&mut occupied.stream, &run_id).await?;
            let tool_call_id = confirm.tool_call_id.clone().context("确认事件应带 toolCallId")?;
            post_confirm(context, &occupied.client_id, &tool_call_id, ClientDecision::Reject).await?;
            let resumed = flow::drain_run(&mut occupied.stream, &run_id).await?;
            ensure!(
                resumed.terminal.event == RunEventKind::RunCompleted,
                "驳回后 Run 仍应完成：{:?}",
                resumed.terminal.event
            );
            ensure!(
                resumed.content.contains(REJECTED_TEXT),
                "驳回应把固定文本回灌给模型：{}",
                resumed.content
            );
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
            assert_no_call(&evidence, "write_log")?;
            occupied.release(context).await?;
            Ok::<(), anyhow::Error>(())
        })
        .await?;
        notes.push(format!("驳回：回灌「{REJECTED_TEXT}」，工具未被调用"));
        Ok(notes)
    })
}

/// C-8：审计里的 `toolName` 是可见名、`fileAccess` 等于配置声明、`status` 与事件一致。
fn audit_case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-audit")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "normal", "file_access = { 'write*' = 'write' }\n")?;
        let notes = serve_with(context, &mock, "", &mcp, async {
            wait_server(context, super::SERVER, McpStatusKind::Ready).await?;
            let mut occupied = Occupied::start(context, "mcp.audit").await?;
            let (sent, drained) = occupied
                .say(context, &prompt(one_call("write_log", json!({"path": "log.txt"}))))
                .await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "审计用例的 Run 应完成：{:?}",
                drained.terminal.event
            );
            let records = audit(context, &sent.run.id).await?;
            ensure!(records.len() == 1, "应有一条审计记录：{records:?}");
            let record = &records[0];
            ensure!(
                record.tool_name == visible("write_log"),
                "审计里的 toolName 应是可见名：{}",
                record.tool_name
            );
            ensure!(
                record.file_access == ToolFileAccess::Write,
                "fileAccess 应等于配置声明（write* → WRITE）：{:?}",
                record.file_access
            );
            ensure!(
                record.status == ToolCallStatus::Ok,
                "审计状态应为 OK：{:?}",
                record.status
            );
            ensure!(
                record.duration_ms >= crate::mock::mcp::WRITE_DELAY_MS as i64 / 2,
                "审计耗时应覆盖替身的延迟：{}ms",
                record.duration_ms
            );
            let completed = events_of(&drained, RunEventKind::ToolCallCompleted);
            ensure!(
                completed.len() == 1
                    && completed[0].tool_status == Some(ToolCallStatus::Ok)
                    && completed[0].tool_name.as_deref() == Some(record.tool_name.as_str()),
                "审计与结束事件应一致：{:?}",
                completed.first().map(|event| (&event.tool_name, event.tool_status))
            );
            occupied.release(context).await?;
            Ok(vec![format!(
                "审计：{} / {:?} / {:?} / {}ms",
                record.tool_name, record.file_access, record.status, record.duration_ms
            )])
        })
        .await?;
        Ok(notes)
    })
}

/// 断言替身没有收到某个工具的调用。
fn assert_no_call(evidence: &Path, tool: &str) -> Result<()> {
    let recorded = mcp_util::evidence(evidence)?;
    ensure!(
        mcp_util::call_of(&recorded, tool).is_none(),
        "替身不应收到 {tool} 调用：{recorded}"
    );
    Ok(())
}
