//! 会话流程辅助：注册 → 建会话 → 占用 → 订阅下行流 → 发消息 → 等终态。
//!
//! `chat.*` / `run.*` / `session.*` 用例都要走这条链路，动作与断言口径放在这里，
//! 各领域只写自己关心的部分（P2-026）。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use lib_bin_cli::client::{Client, ClientError, EventStream};
use lib_bin_cli::sdk::{
    ApiClient, ClientCreatePO, ClientDeletePO, ClientEvent, ClientEventKind, ClientStreamParams, MessageSendPO,
    MessageSendVO, OccupancyCreatePO, OccupancyDeletePO, OccupancyVO, Run, RunEvent, RunEventKind, RunGetParams,
    RunStatus, SessionCreatePO, SessionSummary,
};
use tokio::time::sleep;

use crate::util::context::TestContext;

/// 一次 Run 从发起到终态的上限：慢速分块场景也只在该量级内结束。
pub(crate) const RUN_TIMEOUT: Duration = Duration::from_secs(30);
/// 轮询间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 注册一个客户端，返回 `clientId`。
pub(crate) async fn register(context: &TestContext) -> Result<String> {
    let registered = context
        .client()
        .client_create(ClientCreatePO {})
        .await
        .map_err(ClientError::from_call)?;
    ensure!(!registered.client_id.is_empty(), "注册返回的 clientId 为空");
    Ok(registered.client_id)
}

/// 建立占用：成功即启动该会话的运行时会话并打开事件文件。
pub(crate) async fn occupy(context: &TestContext, client_id: &str, session_id: &str) -> Result<OccupancyVO> {
    let occupancy = context
        .client()
        .occupancy_create(OccupancyCreatePO {
            client_id: client_id.to_string(),
            session_id: session_id.to_string(),
        })
        .await
        .map_err(ClientError::from_call)?;
    Ok(occupancy)
}

/// 收尾：先交还占用、再反注册，避免影响同一服务端的其他用例。
pub(crate) async fn release(context: &TestContext, client_id: &str) -> Result<()> {
    context
        .client()
        .occupancy_delete(OccupancyDeletePO {
            client_id: client_id.to_string(),
        })
        .await
        .map_err(ClientError::from_call)?;
    context
        .client()
        .client_delete(ClientDeletePO {
            client_id: client_id.to_string(),
        })
        .await
        .map_err(ClientError::from_call)?;
    Ok(())
}

/// 从下行事件里取出属于指定 Run 的那条 `RunEvent`；其他事件（问询、别的 Run）返回空。
pub(crate) fn run_event_of(event: &ClientEvent, run_id: &str) -> Option<RunEvent> {
    if event.event != ClientEventKind::SessionEvent {
        return None;
    }
    let run_event = event.session_event.clone()?;
    (run_event.run_id == run_id).then_some(run_event)
}

/// 终态事件：一条 Run 在事件文件里的结束。
pub(crate) fn is_terminal(kind: RunEventKind) -> bool {
    matches!(
        kind,
        RunEventKind::RunCompleted
            | RunEventKind::RunFailed
            | RunEventKind::RunCancelled
            | RunEventKind::ServerShutdown
    )
}

/// 一次 Run 从下行流收到的事件。
pub(crate) struct Drained {
    /// 按到达顺序收到的事件（只含该 Run）。
    pub(crate) events: Vec<RunEvent>,
    /// `message_delta` 拼接出的正文。
    pub(crate) content: String,
    /// 最后一条终态事件。
    pub(crate) terminal: RunEvent,
}

/// 读下行流直到该 Run 的终态事件；超时或流提前结束都返回可读错误。
pub(crate) async fn drain_run(stream: &mut EventStream, run_id: &str) -> Result<Drained> {
    let deadline = Instant::now() + RUN_TIMEOUT;
    let mut events = Vec::new();
    let mut content = String::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "Run {run_id} 在 {} 秒内没有终态事件（已收 {} 条）",
            RUN_TIMEOUT.as_secs(),
            events.len()
        );
        let received = tokio::time::timeout(remaining, stream.recv()).await;
        let event = match received {
            Ok(received) => received?,
            Err(_) => bail!("等待下行事件超时（已收 {} 条）", events.len()),
        };
        let Some(event) = event else {
            bail!("下行流在终态事件之前结束（已收 {} 条）", events.len())
        };
        let Some(run_event) = run_event_of(&event, run_id) else {
            continue;
        };
        if run_event.event == RunEventKind::MessageDelta {
            content.push_str(run_event.delta.as_deref().unwrap_or_default());
        }
        let terminal = is_terminal(run_event.event);
        events.push(run_event.clone());
        if terminal {
            return Ok(Drained {
                events,
                content,
                terminal: run_event,
            });
        }
    }
}

/// 轮询 `api/run/get` 直到 Run 抵终态，返回最后一次读到的 Run。
pub(crate) async fn wait_run(client: &Client, run_id: &str) -> Result<Run> {
    let deadline = Instant::now() + RUN_TIMEOUT;
    loop {
        let run = client
            .run_get(RunGetParams {
                run_id: run_id.to_string(),
            })
            .await
            .map_err(ClientError::from_call)?;
        match run.status {
            RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled => return Ok(run),
            // 等工具确认（WAITING）不是终态：确认或取消后才离开等待。
            RunStatus::Queued | RunStatus::Running | RunStatus::Waiting => {}
        }
        ensure!(
            Instant::now() < deadline,
            "Run {run_id} 在 {} 秒内未抵终态，当前状态 {:?}",
            RUN_TIMEOUT.as_secs(),
            run.status
        );
        sleep(POLL_INTERVAL).await;
    }
}

/// 本次占用的事件文件路径：`<应用根目录>/data/events/<sessionId>/<runtimeSessionId>.jsonl`。
pub(crate) fn journal_path(context: &TestContext, session_id: &str, runtime_session_id: &str) -> PathBuf {
    context
        .home()
        .join("data")
        .join("events")
        .join(session_id)
        .join(format!("{runtime_session_id}.jsonl"))
}

/// 逐行读事件文件。
pub(crate) fn read_journal(path: &Path) -> Result<Vec<RunEvent>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("读取事件文件失败：{}", path.display()))?;
    text.lines()
        .map(|line| serde_json::from_str::<RunEvent>(line).context("事件文件里有一行不是有效的 RunEvent"))
        .collect()
}

/// 轮询事件文件直到最后一条是终态事件，返回全文。
pub(crate) async fn wait_journal_terminal(
    context: &TestContext,
    session_id: &str,
    runtime_session_id: &str,
) -> Result<Vec<RunEvent>> {
    let path = journal_path(context, session_id, runtime_session_id);
    let deadline = Instant::now() + RUN_TIMEOUT;
    loop {
        let events = read_journal(&path).unwrap_or_default();
        if events.last().is_some_and(|event| is_terminal(event.event)) {
            return Ok(events);
        }
        ensure!(
            Instant::now() < deadline,
            "事件文件在 {} 秒内没有终态事件：{}",
            RUN_TIMEOUT.as_secs(),
            path.display()
        );
        sleep(POLL_INTERVAL).await;
    }
}

/// 读到该 Run 的第一条 `message_delta`，返回此前已收到的事件。
///
/// 用于「先确认流式已经开始、再施加取消 / 断连」的用例：之后的终态仍用 [`drain_run`] 读。
pub(crate) async fn wait_first_delta(stream: &mut EventStream, run_id: &str) -> Result<Vec<RunEvent>> {
    let deadline = Instant::now() + RUN_TIMEOUT;
    let mut events = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "Run {run_id} 在 {} 秒内没有增量",
            RUN_TIMEOUT.as_secs()
        );
        let received = tokio::time::timeout(remaining, stream.recv()).await;
        let event = match received {
            Ok(received) => received?,
            Err(_) => bail!("等待下行事件超时（已收 {} 条）", events.len()),
        };
        let Some(event) = event else {
            bail!("下行流在第一条增量之前结束（已收 {} 条）", events.len());
        };
        let Some(run_event) = run_event_of(&event, run_id) else {
            continue;
        };
        let delta = run_event.event == RunEventKind::MessageDelta;
        events.push(run_event);
        if delta {
            return Ok(events);
        }
    }
}

/// 一个已占用会话：注册 → 建会话 → 占用 → 订阅下行流，一次做完。
pub(crate) struct Occupied {
    /// 占用该会话的客户端。
    pub(crate) client_id: String,
    /// 会话摘要。
    pub(crate) session: SessionSummary,
    /// 占用信息：事件文件名取自 `runtime_session_id`。
    pub(crate) occupancy: OccupancyVO,
    /// 该客户端的下行事件流。
    pub(crate) stream: EventStream,
}

impl Occupied {
    /// 建立整条链路：必须先占用再发消息，否则 Run 会停在 `QUEUED`（D-P1.1-004）。
    pub(crate) async fn start(context: &TestContext, title: &str) -> Result<Self> {
        let session = context
            .client()
            .session_create(SessionCreatePO {
                title: Some(title.to_string()),
                model: None,
            })
            .await
            .map_err(ClientError::from_call)?;
        let client_id = register(context).await?;
        let occupancy = occupy(context, &client_id, &session.id).await?;
        let stream = context
            .client()
            .subscribe_client(ClientStreamParams {
                client_id: client_id.clone(),
            })
            .await?;
        Ok(Self {
            client_id,
            session,
            occupancy,
            stream,
        })
    }

    /// 发一条消息并把它发起的 Run 读到终态。
    pub(crate) async fn say(&mut self, context: &TestContext, content: &str) -> Result<(MessageSendVO, Drained)> {
        let sent = context
            .client()
            .message_create(MessageSendPO {
                session_id: self.session.id.clone(),
                content: content.to_string(),
            })
            .await
            .map_err(ClientError::from_call)?;
        let drained = drain_run(&mut self.stream, &sent.run.id).await?;
        Ok((sent, drained))
    }

    /// 交还占用并反注册（断连类用例不调用它：客户端已经断开）。
    pub(crate) async fn release(self, context: &TestContext) -> Result<()> {
        release(context, &self.client_id).await
    }
}
