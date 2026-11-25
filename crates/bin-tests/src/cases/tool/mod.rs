//! Tool System 用例（P3-030）：Schema 与清单、多轮闭环、并行、聚合、四种失败回灌、
//! 轮次上限、取消、审计、内置工具、配置校验、确认链路与客户端消费。
//!
//! 断言口径（与 `.steps/p3/acceptance.md` 对应）：
//!
//! - 数据来源只有四类：客户端收到的 `RunEvent`、接口响应（`run/get` / `message/list` /
//!   `api/tool/audit/list`）、Mock 侧 `/mock/last-request` 与 `/mock/stats`、`metrics` 快照；
//!   **不读日志**。
//! - 「模型请求调用工具」由 Mock 的工具调用脚本表达：用户输入末尾写 `tool:{…}`
//!   （见 [`crate::mock::llm::tools`]），脚本只在历史里还没有工具调用时生效，
//!   因此两轮闭环不需要 Mock 记住状态。
//! - 需要干净配置或 Mock 观测的用例一律 `SelfServed`：私有 Mock + 私有服务端，
//!   收尾前采集快照（`server::serve_once` 负责）。

mod builtin;
mod client;
mod config;
mod confirm;
mod context;
mod events;
mod executor;
mod loop_;

use std::future::Future;
use std::time::{Duration, Instant};

use anyhow::{Result, bail, ensure};
use lib_bin_cli::client::{ClientError, EventStream};
use lib_bin_cli::sdk::{
    ApiClient, ClientDecision, ClientPostKind, ClientPostPO, Message, MessageListParams, MessageSendVO, RunEvent,
    RunEventKind, RunGetParams, RunStatus, ToolAuditListParams, ToolCallVO,
};
use serde_json::Value;

use crate::case::{Case, CaseFn, CaseKind};
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::flow::{self, Drained, Occupied};
use crate::util::server::{self, config};

/// 等确认事件的上限：确认没有时限，这里只防「用例里挂起根本没有发生」。
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(10);
/// Tool System 用例；排序由上层统一处理。
pub(crate) fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    cases.extend(loop_::cases());
    cases.extend(executor::cases());
    cases.extend(context::cases());
    cases.extend(builtin::cases());
    cases.extend(confirm::cases());
    cases.extend(config::cases());
    cases.extend(events::cases());
    cases.extend(client::cases());
    cases
}

/// 声明一个自管用例：`prep` 一律为空（工具用例不依赖 WebUI 产物）。
pub(super) fn case(name: &'static str, kind: CaseKind, run: CaseFn) -> Case {
    Case {
        name,
        kind,
        prep: &[],
        run,
    }
}

/// 用例自带的用户输入：正文 + 末尾的工具调用脚本（脚本必须落在最后一条消息的末尾）。
pub(super) fn prompt(spec: Value) -> String {
    format!("按脚本处理这一轮。\ntool:{spec}")
}

/// 用私有 Mock 的配置起一次私有服务端，跑完用例体后采集快照并停止服务端。
pub(super) async fn serve_with<T>(
    context: &TestContext,
    mock: &MockServer,
    extra: &str,
    body: impl Future<Output = Result<T>>,
) -> Result<T> {
    let text = config(mock.base_url(), extra);
    server::serve_once(context, &text, body).await
}

/// 发一条带工具脚本的消息，并把该 Run 读到终态。
pub(super) async fn say(
    occupied: &mut Occupied,
    context: &TestContext,
    spec: Value,
) -> Result<(MessageSendVO, Drained)> {
    occupied.say(context, &prompt(spec)).await
}

/// 读到该 Run 的 `tool_call_confirm_required`（挂起用例用），同时返回途经的事件。
///
/// 途经事件一并交回：调用方常要断言「开始事件先于确认事件」，而流是单向的，读走就没了。
/// 挂起没有终态事件：读到确认事件即返回；中途收到终态说明挂起根本没发生，直接失败。
pub(super) async fn wait_confirm(stream: &mut EventStream, run_id: &str) -> Result<(RunEvent, Vec<RunEvent>)> {
    let deadline = Instant::now() + CONFIRM_TIMEOUT;
    let mut seen = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "Run {run_id} 在 {} 秒内没有发出确认事件",
            CONFIRM_TIMEOUT.as_secs()
        );
        let received = tokio::time::timeout(remaining, stream.recv()).await;
        let event = match received {
            Ok(received) => received?,
            Err(_) => bail!("等待确认事件超时"),
        };
        let Some(event) = event else {
            bail!("下行流在确认事件之前结束");
        };
        let Some(run_event) = flow::run_event_of(&event, run_id) else {
            continue;
        };
        if run_event.event == RunEventKind::ToolCallConfirmRequired {
            seen.push(run_event.clone());
            return Ok((run_event, seen));
        }
        ensure!(
            !flow::is_terminal(run_event.event),
            "Run 在发出确认事件之前就终止了：{:?}（{}）",
            run_event.event,
            run_event.error.clone().unwrap_or_default()
        );
        seen.push(run_event);
    }
}

/// 上行一次工具确认（`api/client/post { kind = TOOL_CONFIRM }`）。
pub(super) async fn post_confirm(
    context: &TestContext,
    client_id: &str,
    tool_call_id: &str,
    decision: ClientDecision,
) -> Result<()> {
    context
        .client()
        .client_post(ClientPostPO {
            client_id: client_id.to_string(),
            kind: ClientPostKind::ToolConfirm,
            tool_call_id: Some(tool_call_id.to_string()),
            decision: Some(decision),
        })
        .await
        .map_err(ClientError::from_call)?;
    Ok(())
}

/// 轮询 `run/get` 直到 Run 抵指定状态；超过上限返回当前状态由调用方断言。
pub(super) async fn wait_status(context: &TestContext, run_id: &str, want: RunStatus) -> Result<RunStatus> {
    let deadline = Instant::now() + CONFIRM_TIMEOUT;
    loop {
        let run = context
            .client()
            .run_get(RunGetParams {
                run_id: run_id.to_string(),
            })
            .await
            .map_err(ClientError::from_call)?;
        if run.status == want || Instant::now() >= deadline {
            return Ok(run.status);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// 轮询审计直到满足条件（挂起转终态是异步的：恢复后由队列重新执行）。
pub(super) async fn wait_audit(
    context: &TestContext,
    run_id: &str,
    done: impl Fn(&[ToolCallVO]) -> bool,
) -> Result<Vec<ToolCallVO>> {
    let deadline = Instant::now() + CONFIRM_TIMEOUT;
    loop {
        let records = audit(context, run_id).await?;
        if done(&records) || Instant::now() >= deadline {
            return Ok(records);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// 读到该 Run 的指定事件，返回途经的事件（取消类用例据此在「工具开始执行」之后动手）。
pub(super) async fn wait_event(stream: &mut EventStream, run_id: &str, kind: RunEventKind) -> Result<Vec<RunEvent>> {
    let deadline = Instant::now() + CONFIRM_TIMEOUT;
    let mut seen = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "Run {run_id} 在 {} 秒内没有 {:?} 事件",
            CONFIRM_TIMEOUT.as_secs(),
            kind
        );
        let received = tokio::time::timeout(remaining, stream.recv()).await;
        let event = match received {
            Ok(received) => received?,
            Err(_) => bail!("等待 {kind:?} 事件超时"),
        };
        let Some(event) = event else {
            bail!("下行流在 {kind:?} 事件之前结束");
        };
        let Some(run_event) = flow::run_event_of(&event, run_id) else {
            continue;
        };
        let matched = run_event.event == kind;
        let terminal = flow::is_terminal(run_event.event);
        seen.push(run_event);
        if matched {
            return Ok(seen);
        }
        ensure!(!terminal, "Run 在 {:?} 事件之前就终止了", kind);
    }
}

/// 按 Run 查审计记录（`GET api/tool/audit/list`）。
pub(super) async fn audit(context: &TestContext, run_id: &str) -> Result<Vec<ToolCallVO>> {
    let records = context
        .client()
        .tool_audit_list(ToolAuditListParams {
            run_id: run_id.to_string(),
        })
        .await
        .map_err(ClientError::from_call)?;
    Ok(records)
}

/// 会话的全部消息（`GET api/message/list`）。
pub(super) async fn messages(context: &TestContext, session_id: &str) -> Result<Vec<Message>> {
    let messages = context
        .client()
        .message_list(MessageListParams {
            session_id: session_id.to_string(),
        })
        .await
        .map_err(ClientError::from_call)?;
    Ok(messages)
}

/// 一次 Run 的事件里、指定类型的那些。
pub(super) fn events_of(drained: &Drained, kind: RunEventKind) -> Vec<&RunEvent> {
    drained.events.iter().filter(|event| event.event == kind).collect()
}

/// `seq` 是否从 0 开始连续递增（事件契约的硬要求）。
pub(super) fn seq_contiguous(events: &[RunEvent]) -> bool {
    events
        .iter()
        .enumerate()
        .all(|(index, event)| event.seq == i64::try_from(index).unwrap_or(i64::MAX))
}
