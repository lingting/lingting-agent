//! 客户端消费用例：下行事件可达（W-1）、TUI 展示与确认状态机（W-2）、WebUI 工具事件用例（W-3）。

use anyhow::{Context as _, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiClient, ClientDecision, MessageSendPO, RunEventKind, ToolCallStatus};
use lib_bin_tui::testing::{App, Command as TuiCommand, KeyCode, KeyEvent, KeyModifiers, Update};
use serde_json::json;

use super::{case, post_confirm, prompt, serve_with, wait_confirm};
use crate::case::{Case, CaseFuture, CaseKind, Prep};
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::flow::{self, Occupied};
use crate::util::frontend;

/// 只把 `http_request` 提为「需确认」：其余工具保持缺省权限。
const CONFIRM_ONLY: &str = r#"[tools.permissions]
http_request = "confirm"
"#;

/// 浏览器侧用例的服务端配置：把 `calculator` 提为「需确认」，卡片才会出现两个按钮。
const WEB_CONFIRM: &str = r#"[tools.permissions]
calculator = "confirm"
"#;

/// 客户端消费用例。
pub(super) fn cases() -> Vec<Case> {
    vec![
        case("tool.events", CaseKind::SelfServed, events),
        case("tool.tui", CaseKind::Shared, tui),
        case("tool.web", CaseKind::Shared, web),
        // 浏览器侧用例：需要 WebUI 产物与「需确认」的 calculator。
        Case {
            name: "tool.web.e2e",
            kind: CaseKind::SelfServed,
            prep: &[Prep::WebUiBuild],
            run: web_e2e,
        },
    ]
}

/// W-1：三个工具事件都经下行流到达客户端，且与终态事件的顺序一致。
fn events(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        serve_with(context, &mock, CONFIRM_ONLY, async {
            let mut occupied = Occupied::start(context, "tool.events").await?;
            let spec = json!({"calls": [{"name": "http_request", "arguments": {"url": "http://127.0.0.1:1/"}}]});
            let sent = context
                .client()
                .message_create(MessageSendPO {
                    session_id: occupied.session.id.clone(),
                    content: prompt(spec),
                })
                .await
                .map_err(ClientError::from_call)?;
            let run_id = sent.run.id.clone();
            let (confirm, mut seen) = wait_confirm(&mut occupied.stream, &run_id).await?;
            let tool_call_id = confirm.tool_call_id.clone().context("确认事件应带 toolCallId")?;
            // 驳回：让第三个工具事件（`tool_call_completed`）也走同一条下行流。
            post_confirm(context, &occupied.client_id, &tool_call_id, ClientDecision::Reject).await?;
            let drained = flow::drain_run(&mut occupied.stream, &run_id).await?;
            // 开始与确认事件已经在等确认时读走，按到达顺序接回来。
            seen.extend(drained.events.iter().cloned());

            let kinds: Vec<RunEventKind> = seen.iter().map(|event| event.event).collect();
            let position = |kind: RunEventKind| kinds.iter().position(|item| *item == kind);
            let started = position(RunEventKind::ToolCallStarted).context("应收到 tool_call_started")?;
            let required = position(RunEventKind::ToolCallConfirmRequired).context("应收到确认事件")?;
            let completed = position(RunEventKind::ToolCallCompleted).context("应收到 tool_call_completed")?;
            let terminal = position(RunEventKind::RunCompleted).context("应收到 run_completed")?;
            ensure!(
                started < required && required < completed && completed < terminal,
                "工具事件必须按「开始 → 待确认 → 结束 → 终态」到达：{kinds:?}"
            );
            ensure!(
                seen.iter()
                    .all(|event| event.run_id == run_id && event.session_id == occupied.session.id),
                "同一 Run 的事件 runId / sessionId 必须一致"
            );
            let completed_event = seen
                .iter()
                .find(|event| event.event == RunEventKind::ToolCallCompleted)
                .context("缺少结束事件")?;
            ensure!(
                completed_event.tool_status == Some(ToolCallStatus::Denied)
                    && completed_event.tool_call_id.as_deref() == Some(tool_call_id.as_str()),
                "驳回后的结束事件应是同一 toolCallId 的 DENIED：{completed_event:?}"
            );
            occupied.release(context).await?;
            Ok(vec![format!("下行流事件顺序：{kinds:?}（toolCallId {tool_call_id}）")])
        })
        .await
    })
}

/// W-2：三个事件转成对应 `Update`；无 pending 时 `y` / `n` 不产生上行请求。
fn tui(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut app = App::new("http://127.0.0.1:20104".to_string());
        let mut notes = Vec::new();
        let lines = |app: &App| -> Vec<String> { app.messages.iter().map(|message| message.content.clone()).collect() };

        // 一次调用的三个事件：开始 → 确认 → 结束；同一 `toolCallId` 只占一行。
        app.apply(Update::ToolStarted {
            tool_call_id: "call-1".to_string(),
            name: "calculator".to_string(),
        });
        ensure!(
            app.messages.len() == 1 && app.messages[0].content == "→ calculator",
            "开始事件应写一行工具调用：{:?}",
            lines(&app)
        );

        app.apply(Update::ToolConfirmRequired {
            tool_call_id: "call-2".to_string(),
            name: "http_request".to_string(),
            arguments: "{\"url\":\"http://127.0.0.1:1/\"}".to_string(),
        });
        let pending = app.pending.as_ref().context("确认事件应产出待确认项")?;
        ensure!(
            pending.tool_call_id == "call-2" && !pending.submitted,
            "待确认项应绑定该 toolCallId 且未提交"
        );
        ensure!(
            app.messages.iter().any(
                |message| message.content.contains("⚠ 需要确认") && message.content.contains("[y] 允许 / [n] 拒绝")
            ),
            "确认行应带 y / n 提示：{:?}",
            lines(&app)
        );

        // `y` 批准：发出一条 `Confirm`，并把该行改成「已提交」；再按不重复上行。
        let commands = app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        match commands.as_slice() {
            [TuiCommand::Confirm { tool_call_id, decision }] => {
                ensure!(
                    tool_call_id == "call-2" && *decision == ClientDecision::Approve,
                    "y 应上行一次 APPROVE：{tool_call_id} / {decision:?}"
                );
            }
            other => anyhow::bail!("y 应产生一条 Confirm 命令，实际 {other:?}"),
        }
        ensure!(
            app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE))
                .is_empty(),
            "已提交后重复按键不应再上行"
        );
        ensure!(
            app.messages
                .iter()
                .any(|message| message.content.contains("已提交，等待结果")),
            "提交后该行应改成「已提交，等待结果」：{:?}",
            lines(&app)
        );
        notes.push("y → Confirm{APPROVE}，重复按键不上行".to_string());

        // 结果事件到达：确认项清空，行改成结果文本。
        app.apply(Update::ToolFinished {
            tool_call_id: "call-2".to_string(),
            name: "http_request".to_string(),
            status: ToolCallStatus::Denied,
            duration_ms: 0,
            error: Some("工具未授权：http_request".to_string()),
        });
        ensure!(app.pending.is_none(), "结果事件到达后不应再有待确认项");
        ensure!(
            app.messages
                .iter()
                .any(|message| message.content == "✗ http_request：工具未授权"),
            "未授权应展示固定文案：{:?}",
            lines(&app)
        );

        // 成功结果行带耗时。
        app.apply(Update::ToolFinished {
            tool_call_id: "call-1".to_string(),
            name: "calculator".to_string(),
            status: ToolCallStatus::Ok,
            duration_ms: 12,
            error: None,
        });
        ensure!(
            app.messages
                .iter()
                .any(|message| message.content == "✓ calculator（12ms）"),
            "成功行应带耗时：{:?}",
            lines(&app)
        );
        notes.push("结果事件：DENIED / OK 各自成行".to_string());

        // 没有 pending 时 y / n 不产生任何上行（落到输入框里）。
        ensure!(
            app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE))
                .iter()
                .all(|command| !matches!(command, TuiCommand::Confirm { .. })),
            "没有待确认项时 y 不应上行确认"
        );
        ensure!(
            app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
                .iter()
                .all(|command| !matches!(command, TuiCommand::Confirm { .. })),
            "没有待确认项时 n 不应上行确认"
        );
        notes.push("无待确认时 y / n 不上行".to_string());

        // `n` 驳回：单独一次确认。
        app.apply(Update::ToolConfirmRequired {
            tool_call_id: "call-3".to_string(),
            name: "file_read".to_string(),
            arguments: "{\"path\":\"a.txt\"}".to_string(),
        });
        match app
            .handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
            .as_slice()
        {
            [TuiCommand::Confirm { decision, .. }] => ensure!(
                *decision == ClientDecision::Reject,
                "n 应上行 REJECT，实际 {decision:?}"
            ),
            other => anyhow::bail!("n 应产生一条 Confirm 命令，实际 {other:?}"),
        }
        notes.push("n → Confirm{REJECT}".to_string());

        Ok(notes)
    })
}

/// W-3：前端工具卡片与确认状态机的用例（`useChat` / `ToolCard`）全部通过。
fn web(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        frontend::tool_tests().await?;
        Ok(vec!["前端工具事件用例通过（tool-events.test.tsx）".to_string()])
    })
}

/// W-3 的浏览器侧：真实 Chromium 里把「卡片 → 确认 → 执行」走一遍（P3-029 的视觉条目）。
///
/// 服务端配置把 `calculator` 提为「需确认」，因此一张卡片会依次经过 待确认 → 等结果 → 完成；
/// 前端用例同时把两个状态的截图落在 `lingting-agent-ui/tests/.artifacts/`，供人工目测比对。
fn web_e2e(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        serve_with(context, &mock, WEB_CONFIRM, async {
            // 产物落到本用例自己的 `AGENT_HOME`：`--all` 时它与 `web.e2e` 各跑一个
            // Playwright 进程，共用 `tests/.artifacts` 会互相覆盖。
            let output = context.home().join("playwright");
            frontend::e2e_project(context.base_url(), "tool", &output).await?;
            Ok(vec![
                "浏览器侧工具卡片用例通过（playwright --project tool）".to_string(),
            ])
        })
        .await
    })
}
