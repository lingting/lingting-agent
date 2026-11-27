//! `resume.terminal`（P6A-012）：终态唯一收口——取消、优雅关闭、恢复中再次中断、竞态。
//!
//! 两半证据：
//!
//! - 存储半边在临时库上直接驱动 `SessionService` / `Repository`：`PAUSED` 被取消后中断原因清空、
//!   检查点按终态策略裁剪，未命中的二次取消不改任何字段；
//! - 接口半边起一次私有服务端，用「强杀 → 重启」制造中断态，再分别施加取消、优雅关闭与二次强杀。
//!
//! 证据只有接口响应与库内行，**不读日志、不读事件文件**；每次强杀都复用同一 `AGENT_HOME` 与端口。

use std::sync::Arc;
use std::time::Duration;

use agent_core::session::{
    CHECKPOINT_KEEP_LAST, CheckpointPhase, RunCancelPO as DomainRunCancelPO, RunStatus as DomainRunStatus,
};
use anyhow::{Context, Result, anyhow, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{
    ApiClient, MessageRole, RunCancelPO as SdkRunCancelPO, RunEventKind, RunHistoryParams, RunStatus as SdkRunStatus,
};
use lib_core::time;
use service_core::repository::Repository;

use crate::util::context::TestContext;
use crate::util::flow::{self, Reopened};
use crate::util::server::Server;

/// 取消的固定原因（与 `service-core` 的 `REASON_CANCELLED` 逐字一致）。
const CANCEL_REASON: &str = "用户取消执行";
/// 执行中的 Run 被自愈后的中断原因（与 `manager-agent` 的 `INTERRUPTED_RUNNING` 逐字一致）。
const INTERRUPTED_RUNNING: &str = "服务重启，执行中的 Run 已中断";
/// 自愈时排队中 Run 的原因：本用例只断言它落在 `CANCELLED`，文案不参与判定。
const QUEUED_ERROR: &str = "服务重启，排队中的 Run 已取消";
/// 等优雅关闭自行走完的上限：关闭要收尾客户端、落终态并 flush 事件文件。
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

/// 用例主体：先跑存储半边（无服务端），再起一次私有服务端跑接口半边。
pub(super) async fn require(context: &TestContext) -> Result<Vec<String>> {
    let mut notes = cancel_storage().await?;
    notes.extend(cancel_shutdown_and_interrupt(context).await?);
    Ok(notes)
}

/// 一、取消可恢复中断态（存储半边）：中断原因清空、检查点按终态策略裁剪、二次取消不改行。
async fn cancel_storage() -> Result<Vec<String>> {
    let (database, service) = super::fixture("resume-terminal-cancel").await?;
    let repository = Repository::new(Arc::clone(&database));
    let run_id = super::new_run(&service, "终态收口取消").await?;

    // 超过保留上限的检查点：取消是终态，应当被裁到 `CHECKPOINT_KEEP_LAST`。
    let total = u32::try_from(CHECKPOINT_KEEP_LAST).unwrap_or_default() + 5;
    for round in 0..total {
        service
            .append_checkpoint(
                &run_id,
                &super::checkpoint(CheckpointPhase::ToolsDone, round, i64::from(round)),
            )
            .await?;
    }
    let written = service.checkpoints_of_run(&run_id).await?.len();
    ensure!(
        written == usize::try_from(total).unwrap_or_default(),
        "应写入 {total} 条检查点，实际 {written}"
    );

    // `RUNNING → PAUSED` 走启动自愈的同一条 SQL（`PAUSED` 的唯一写入点，D-P6-004）。
    ensure!(service.start_run(&run_id).await?, "Run 应能推进到 RUNNING");
    repository
        .with_orphan_healing(INTERRUPTED_RUNNING, QUEUED_ERROR, time::now_millis())
        .await
        .context("自愈残留 Run 失败")?;
    let paused = super::run_of(&service, &run_id).await?;
    ensure!(
        paused.status == DomainRunStatus::Paused
            && paused.interrupted_reason.as_deref() == Some(INTERRUPTED_RUNNING)
            && paused.finished_at_ms.is_none(),
        "自愈后应是带中断原因、没有结束时间的 PAUSED：{paused:?}"
    );
    ensure!(
        service.checkpoints_of_run(&run_id).await?.len() == written,
        "未落终态前一条检查点都不应被清理"
    );

    // 取消：与普通取消同一个入口，语句里同时清空中断原因。
    let cancelled = service
        .cancel_run(DomainRunCancelPO { run_id: run_id.clone() })
        .await
        .context("取消 PAUSED 的 Run 失败")?;
    ensure!(
        cancelled.status == DomainRunStatus::Cancelled,
        "取消后应为 CANCELLED，实际 {:?}",
        cancelled.status
    );
    ensure!(
        cancelled.interrupted_reason.is_none(),
        "取消后不应保留中断原因：{cancelled:?}"
    );
    ensure!(
        cancelled.error.as_deref() == Some(CANCEL_REASON) && cancelled.finished_at_ms.is_some(),
        "取消应带固定原因与结束时间：{cancelled:?}"
    );

    let stored = super::run_of(&service, &run_id).await?;
    ensure!(
        stored.status == cancelled.status
            && stored.error == cancelled.error
            && stored.interrupted_reason.is_none()
            && stored.finished_at_ms == cancelled.finished_at_ms
            && stored.resume_count == cancelled.resume_count,
        "库内行应与取消返回值逐字段同形：{stored:?} != {cancelled:?}"
    );

    let remaining = service.checkpoints_of_run(&run_id).await?;
    ensure!(
        remaining.len() == CHECKPOINT_KEEP_LAST,
        "取消是终态，检查点应裁到 {CHECKPOINT_KEEP_LAST} 条，实际 {}",
        remaining.len()
    );

    // 二次取消：条件更新未命中，返回既有口径，且一行都不改。
    let error = service
        .cancel_run(DomainRunCancelPO { run_id: run_id.clone() })
        .await
        .err()
        .context("终态的 Run 不应再被取消")?;
    ensure!(
        format!("{error}").contains("不可取消"),
        "二次取消应返回既有的不可取消口径：{error}"
    );
    let after = super::run_of(&service, &run_id).await?;
    ensure!(
        after.status == stored.status
            && after.error == stored.error
            && after.finished_at_ms == stored.finished_at_ms
            && after.updated_at_ms == stored.updated_at_ms,
        "未命中的取消不应改写任何字段：{after:?} != {stored:?}"
    );

    Ok(vec![
        format!("PAUSED 被取消：CANCELLED +「{CANCEL_REASON}」+ 有结束时间，interruptedReason 已清空"),
        format!("检查点从 {written} 条裁到 {} 条（终态保留策略）", remaining.len()),
        "未命中的二次取消返回「不可取消」，status / error / finishedAtMs / updatedAtMs 逐字段不变".to_owned(),
    ])
}

/// 二、接口半边：取消、优雅关闭、恢复中再次中断、竞态（同一会话同一个 Run 串起来）。
async fn cancel_shutdown_and_interrupt(context: &TestContext) -> Result<Vec<String>> {
    let port = context.port()?;
    let mut server = start(context).await?;
    let mut notes = Vec::new();
    let session = flow::create_session(context, "终态收口").await?;

    // 造一个可恢复中断态：占用 → `mock:slow` → 收到首段增量即强杀。
    let mut link = Reopened::open(context, &session.id).await?;
    let slow = format!("mock:slow 终态收口 {}", context.token());
    let sent = flow::send_message(context, &session.id, &slow).await?;
    flow::wait_first_delta(&mut link.stream, &sent.run.id).await?;
    let running = flow::run_get(context, &sent.run.id).await?;
    ensure!(
        running.status == SdkRunStatus::Running && running.finished_at_ms.is_none(),
        "强杀前 Run 应为 RUNNING 且没有结束时间：{running:?}"
    );
    drop(link);
    server = crash(context, server, port).await?;
    let paused = flow::run_get(context, &sent.run.id).await?;
    ensure!(
        paused.status == SdkRunStatus::Paused
            && paused.interrupted_reason.as_deref() == Some(INTERRUPTED_RUNNING)
            && paused.finished_at_ms.is_none(),
        "强杀重启后应是带中断原因的 PAUSED：{paused:?}"
    );

    // 优雅关闭：重新占用但**不发消息**（占用本身不触发恢复），关停后中断态逐字段不变。
    let link = Reopened::open(context, &session.id).await?;
    context
        .client()
        .probe_shutdown()
        .await
        .map_err(ClientError::from_call)?;
    let exit = server.wait_exit(SHUTDOWN_TIMEOUT).await?;
    drop(link);
    server = start(context).await?;
    let after_shutdown = flow::run_get(context, &sent.run.id).await?;
    ensure!(
        after_shutdown.status == SdkRunStatus::Paused
            && after_shutdown.interrupted_reason == paused.interrupted_reason
            && after_shutdown.resume_count == paused.resume_count
            && after_shutdown.updated_at_ms == paused.updated_at_ms
            && after_shutdown.finished_at_ms.is_none(),
        "优雅关闭不应改动中断态（逐字段）：{after_shutdown:?} != {paused:?}"
    );
    notes.push(format!(
        "优雅关闭：进程自行退出（{exit}），中断态 status / interruptedReason / resumeCount / updatedAtMs 逐字段不变"
    ));

    // 恢复中再次强杀：占用 + 一条消息 → 旧 Run 被恢复 → 强杀 → 重启后仍是同一个 PAUSED。
    let mut link = Reopened::open(context, &session.id).await?;
    let trigger = flow::send_message(context, &session.id, &format!("继续 {}", context.token())).await?;
    // 恢复段在 `QUEUED` 段之前：先等到旧 Run 的增量，说明它已被恢复并重新执行。
    flow::wait_first_delta(&mut link.stream, &sent.run.id).await?;
    let recovered = flow::run_get(context, &sent.run.id).await?;
    ensure!(
        recovered.status == SdkRunStatus::Running && recovered.resume_count == paused.resume_count + 1,
        "恢复中的 Run 应为 RUNNING 且 resumeCount 加一：{recovered:?}"
    );
    drop(link);
    server = crash(context, server, port).await?;
    let again = flow::run_get(context, &sent.run.id).await?;
    ensure!(
        again.status == SdkRunStatus::Paused
            && again.interrupted_reason.as_deref() == Some(INTERRUPTED_RUNNING)
            && again.resume_count == recovered.resume_count
            && again.finished_at_ms.is_none(),
        "恢复中再次强杀后仍是同一个 PAUSED，resumeCount 不重置：{again:?}"
    );
    let interrupted_trigger = flow::run_get(context, &trigger.run.id).await?;
    ensure!(
        interrupted_trigger.status == SdkRunStatus::Cancelled,
        "本次消息产生的 QUEUED Run 应被自愈成 CANCELLED，实际 {:?}",
        interrupted_trigger.status
    );
    ensure!(
        !has_assistant(context, &session.id).await?,
        "两次中断都不应补写助手消息"
    );
    notes.push(format!(
        "恢复中再次强杀：resumeCount 从 {} 涨到 {} 后保持不变，仍是单条 PAUSED；本次消息的 QUEUED Run 落 CANCELLED",
        paused.resume_count, recovered.resume_count
    ));

    // 检查点清理的快照口径（验收 `P-5` / P6A-018）：取消是终态，裁剪掉的条数必须等于
    // `checkpointsPruned` 的增量——两条数字都来自接口与快照，不读日志。
    let steps_before_cancel = steps_of(context, &sent.run.id).await?;
    let snapshot_before = server.metrics().await.map_err(|reason| anyhow!("未采集：{reason}"))?;

    // 取消中断态：与普通取消逐字段同形（固定原因、有结束时间、中断原因清空）。
    let cancelled = context
        .client()
        .run_cancel(SdkRunCancelPO {
            run_id: sent.run.id.clone(),
        })
        .await
        .map_err(ClientError::from_call)?;
    ensure!(
        cancelled.status == SdkRunStatus::Cancelled
            && cancelled.error.as_deref() == Some(CANCEL_REASON)
            && cancelled.interrupted_reason.is_none()
            && cancelled.finished_at_ms.is_some()
            && cancelled.resume_count == again.resume_count,
        "取消中断态应落 CANCELLED + 固定原因 + 清空中断原因：{cancelled:?}"
    );
    let terminal = flow::run_get(context, &sent.run.id).await?;
    ensure!(
        terminal.status == cancelled.status
            && terminal.error == cancelled.error
            && terminal.interrupted_reason.is_none()
            && terminal.finished_at_ms == cancelled.finished_at_ms,
        "库内行应与取消返回值同形：{terminal:?} != {cancelled:?}"
    );

    let steps_after_cancel = steps_of(context, &sent.run.id).await?;
    let snapshot_after = server.metrics().await.map_err(|reason| anyhow!("未采集：{reason}"))?;
    let pruned = steps_before_cancel.saturating_sub(steps_after_cancel);
    let pruned_metric = snapshot_after
        .resume
        .checkpoints_pruned
        .saturating_sub(snapshot_before.resume.checkpoints_pruned);
    ensure!(
        u64::try_from(pruned).unwrap_or_default() == pruned_metric,
        "checkpointsPruned 的增量应等于取消删掉的条数 {pruned}（{steps_before_cancel} → {steps_after_cancel}），实际 {pruned_metric}"
    );
    ensure!(
        steps_after_cancel <= CHECKPOINT_KEEP_LAST,
        "取消是终态，检查点应裁到 {CHECKPOINT_KEEP_LAST} 条以内，实际 {steps_after_cancel}"
    );

    // 竞态的顺序形态：取消与恢复都以条件更新为前提，未命中的一方返回既有口径且不改行。
    let missed = context
        .client()
        .run_cancel(SdkRunCancelPO {
            run_id: sent.run.id.clone(),
        })
        .await;
    let error = ClientError::from_call(missed.expect_err("终态的 Run 不应再被取消"));
    ensure!(
        format!("{error}").contains("不可取消"),
        "二次取消应返回既有文案：{error}"
    );
    let unchanged = flow::run_get(context, &sent.run.id).await?;
    ensure!(
        unchanged.status == terminal.status
            && unchanged.error == terminal.error
            && unchanged.finished_at_ms == terminal.finished_at_ms
            && unchanged.updated_at_ms == terminal.updated_at_ms,
        "未命中的取消不应改写任何字段：{unchanged:?} != {terminal:?}"
    );

    // 恢复不再命中终态：发一条消息，只有新 Run 被执行，已终态的 Run 保持不变。
    let mut link = Reopened::open(context, &session.id).await?;
    let next = flow::send_message(context, &session.id, &format!("收尾 {}", context.token())).await?;
    let drained = flow::drain_run(&mut link.stream, &next.run.id).await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCompleted,
        "新 Run 应以 run_completed 收尾，实际 {:?}（{}）",
        drained.terminal.event,
        drained.terminal.error.clone().unwrap_or_default()
    );
    let still = flow::run_get(context, &sent.run.id).await?;
    ensure!(
        still.status == terminal.status && still.updated_at_ms == terminal.updated_at_ms,
        "发消息不应改写已终态的 Run：{still:?} != {terminal:?}"
    );
    link.release(context).await?;
    context.observe_metrics(server.metrics().await);
    server.stop().await?;
    notes.push(format!(
        "PAUSED 取消：CANCELLED +「{CANCEL_REASON}」+ 有结束时间；二次取消返回「不可取消」，随后发消息只执行新 Run（run_completed）"
    ));
    notes.push(format!(
        "检查点清理与快照复算：取消后步骤 {steps_before_cancel} → {steps_after_cancel}，checkpointsPruned 增量 = {pruned_metric}（逐值相等）"
    ));
    Ok(notes)
}

/// 启动服务端（同一 `AGENT_HOME` 与端口）。
async fn start(context: &TestContext) -> Result<Server> {
    Server::start(context.port()?, context.home().to_path_buf())
        .await
        .with_context(|| format!("启动服务端失败：{}", context.home().display()))
}

/// 强杀（不是优雅关闭）后在同一 `AGENT_HOME` 与端口上重启：进程内的执行状态随进程一起丢失。
async fn crash(context: &TestContext, server: Server, port: u16) -> Result<Server> {
    let home = server.log_dir().to_path_buf();
    server.stop().await.context("强杀服务端失败")?;
    Server::start(port, home)
        .await
        .with_context(|| format!("重启服务端失败：{}", context.home().display()))
}

/// `api/run/history` 的步骤条数：检查点条数的接口口径（取消前 / 取消后各读一次）。
async fn steps_of(context: &TestContext, run_id: &str) -> Result<usize> {
    context
        .client()
        .run_history(RunHistoryParams {
            run_id: run_id.to_owned(),
        })
        .await
        .map(|history| history.steps.len())
        .map_err(ClientError::from_call)
        .map_err(Into::into)
}
/// 会话里是否已有助手消息。
async fn has_assistant(context: &TestContext, session_id: &str) -> Result<bool> {
    Ok(flow::messages(context, session_id)
        .await?
        .iter()
        .any(|message| message.role == MessageRole::Assistant))
}
