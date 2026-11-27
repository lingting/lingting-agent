//! `resume.trigger`（P6A-011 / 验收 `R-2`、`I-3`、`E-1`、`E-2`）：恢复的唯一触发条件是
//! 「会话已被占用 ∧ 该会话收到一条新消息」，且恢复段一定早于本次消息产生的 `QUEUED` Run。
//!
//! 四段：
//!
//! - 未占用时发消息：新 Run 停在 `QUEUED`，`PAUSED` 的行逐字段不变（`E-1`）；
//! - 只占用不发消息：`PAUSED` 的行逐字段不变（`R-2` ①）；
//! - 占用 + 发一条消息：`PAUSED` 被推进，`resumeCount` 只加一（`R-2` ②）；
//! - 事件顺序：被恢复 Run 的 `run_completed` 序号早于新 Run 的 `run_started`（`I-3`）；
//!   会话里原本排队的 Run 不会被「恢复」推进（`E-2`）。
//!
//! 断言只引用接口响应与 SSE 事件。**不读日志、不读事件文件。**

use std::time::Duration;

use anyhow::{Context, Result, ensure};
use lib_bin_cli::client::{ClientError, EventStream};
use lib_bin_cli::sdk::{ApiClient, MessageSendVO, Run, RunEventKind, RunGetParams, RunStatus};

use crate::case::CaseFuture;
use crate::util::context::TestContext;
use crate::util::flow::{self, Reopened};

/// 只占用 / 未占用时「什么都没发生」的观察窗口：恢复只能在消息创建之后发生，留一拍足够。
const QUIET: Duration = Duration::from_secs(2);
/// 读事件的整体上限。
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);

/// 用例入口。
pub(super) fn case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { require(context).await })
}

async fn require(context: &TestContext) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    let server = super::start(context).await?;
    let session = flow::create_session(context, "恢复触发").await?;
    let (server, paused_id) = super::interrupt_running(context, server, &session.id).await?;
    let paused = run_of(context, &paused_id).await?;
    ensure!(
        paused.status == RunStatus::Paused,
        "强杀重启后应是 PAUSED，实际 {:?}",
        paused.status
    );

    // 一、未占用时发消息：新 Run 停在 QUEUED，PAUSED 的行逐字段不变（E-1）。
    let queued = flow::send_message(context, &session.id, &format!("未占用 {}", context.token())).await?;
    ensure!(
        queued.run.status == RunStatus::Queued,
        "未占用时新 Run 应停在 QUEUED，实际 {:?}",
        queued.run.status
    );
    ensure_same(context, &paused, QUIET).await?;
    notes.push("未占用时发消息：新 Run 停在 QUEUED，PAUSED 的 status / updatedAtMs / resumeCount 不变".to_owned());

    // 二、只占用不发消息：PAUSED 的行逐字段不变（R-2 ①）。
    let mut link = Reopened::open(context, &session.id).await?;
    ensure_same(context, &paused, QUIET).await?;
    notes.push("只占用不发消息：PAUSED 逐字段不变（占用本身不触发恢复）".to_owned());

    // 三、占用 + 发一条消息：PAUSED 被推进，且它的终态事件早于新 Run 的 run_started（R-2 ② / I-3）。
    let trigger = flow::send_message(context, &session.id, &format!("继续 {}", context.token())).await?;
    ensure_response_shape(&trigger, &session.id)?;
    let order = order_of(&mut link.stream, &paused_id, &trigger.run.id).await?;
    ensure!(
        order.recovered_completed < order.fresh_started,
        "被恢复 Run 的 run_completed（seq {}）应早于新 Run 的 run_started（seq {}）",
        order.recovered_completed,
        order.fresh_started
    );
    let recovered = run_of(context, &paused_id).await?;
    ensure!(
        recovered.resume_count == paused.resume_count + 1,
        "被恢复 Run 的 resumeCount 应只加一：{} -> {}",
        paused.resume_count,
        recovered.resume_count
    );
    // 原本排队的 Run 不是 `PAUSED`：恢复段不该碰它（E-2）。
    let queued_after = run_of(context, &queued.run.id).await?;
    ensure!(
        queued_after.resume_count == 0,
        "只推进 PAUSED：排队的 Run 不应被恢复，resumeCount = {}",
        queued_after.resume_count
    );
    notes.push(format!(
        "占用 + 一条消息：被恢复 Run 的 run_completed（seq {}）早于新 Run 的 run_started（seq {}），resumeCount {} -> {}",
        order.recovered_completed, order.fresh_started, paused.resume_count, recovered.resume_count
    ));

    link.release(context).await?;
    context.observe_metrics(server.metrics().await);
    server.stop().await?;
    Ok(notes)
}

/// 事件顺序的两个序号。
struct Order {
    /// 被恢复 Run 的 `run_completed`。
    recovered_completed: i64,
    /// 本次消息产生的新 Run 的 `run_started`。
    fresh_started: i64,
}

/// 从下行流里读到这两个序号为止；任一条没出现就失败（超时也算）。
async fn order_of(stream: &mut EventStream, recovered_id: &str, fresh_id: &str) -> Result<Order> {
    let mut recovered_completed = None;
    let mut fresh_started = None;
    let deadline = std::time::Instant::now() + EVENT_TIMEOUT;
    while recovered_completed.is_none() || fresh_started.is_none() {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        ensure!(!remaining.is_zero(), "等事件顺序超时（{EVENT_TIMEOUT:?}）");
        let received = tokio::time::timeout(remaining, stream.recv())
            .await
            .context("等待事件顺序超时")?;
        let Some(event) = received? else {
            anyhow::bail!("下行流在读到两个序号之前结束");
        };
        let Some(run_event) = flow::run_event_of(&event, recovered_id).or_else(|| {
            // 同一条流里有两条 Run：先按两条 ID 各认一次。
            flow::run_event_of(&event, fresh_id)
        }) else {
            continue;
        };
        match run_event.event {
            RunEventKind::RunCompleted if run_event.run_id == recovered_id => {
                recovered_completed = Some(run_event.seq);
            }
            RunEventKind::RunStarted if run_event.run_id == fresh_id => {
                fresh_started = Some(run_event.seq);
            }
            _ => {}
        }
    }
    Ok(Order {
        recovered_completed: recovered_completed.unwrap_or_default(),
        fresh_started: fresh_started.unwrap_or_default(),
    })
}

/// 等待窗口结束后，`PAUSED` 的行应与窗口开始前逐字段相同。
async fn ensure_same(context: &TestContext, before: &Run, window: Duration) -> Result<()> {
    tokio::time::sleep(window).await;
    let after = run_of(context, &before.id).await?;
    ensure!(
        after.status == before.status
            && after.updated_at_ms == before.updated_at_ms
            && after.resume_count == before.resume_count
            && after.interrupted_reason == before.interrupted_reason,
        "这一段时间里 PAUSED 的行不应被改写：{after:?} != {before:?}"
    );
    Ok(())
}

/// `api/message/create` 的响应结构不变：仍是「本次写入的用户消息 + 本次创建的 Run」。
fn ensure_response_shape(sent: &MessageSendVO, session_id: &str) -> Result<()> {
    ensure!(
        !sent.message.id.is_empty() && !sent.run.id.is_empty(),
        "响应应带用户消息与 Run：{sent:?}"
    );
    ensure!(
        sent.message.session_id == session_id && sent.run.session_id == session_id,
        "响应里的会话 ID 应是本次目标会话：{sent:?}"
    );
    ensure!(
        sent.message.run_id.as_deref() == Some(sent.run.id.as_str()),
        "用户消息应归属本次创建的 Run：{sent:?}"
    );
    Ok(())
}

/// `api/run/get`。
async fn run_of(context: &TestContext, run_id: &str) -> Result<Run> {
    context
        .client()
        .run_get(RunGetParams {
            run_id: run_id.to_owned(),
        })
        .await
        .map_err(ClientError::from_call)
        .map_err(Into::into)
}
