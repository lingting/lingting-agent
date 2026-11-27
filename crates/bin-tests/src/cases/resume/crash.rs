//! `resume.crash`（P6A-009 / P6A-016）：强杀 → 重启 → 占用 + 发一条消息 → 从检查点继续。
//!
//! 两段：
//!
//! - 一段普通执行被强杀：残留是带中断原因的 `PAUSED`、不计入 `runningRunCount`；**删掉事件目录**
//!   之后仍能恢复（证明恢复只读 SQLite，`R-8`）；恢复完成时该 Run 只有一条助手消息（`S-5`）；
//! - 一段「一轮工具之后被强杀」：已提交的助手消息与轮次在恢复后逐字符不变，历史步骤接着递增
//!   而不是从 1 重来（`R-4`）。
//!
//! 断言只引用接口响应与库内行：`run/get` / `run/history` / `message/list` / `probe/health`。
//! **不读日志、不读事件文件。**

use anyhow::{Context, Result, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiClient, Message, MessageListParams, MessageRole, Run, RunHistoryParams, RunStatus};

use super::super::tool::prompt;
use crate::case::CaseFuture;
use crate::util::context::TestContext;
use crate::util::flow::{self, Reopened};

/// 执行中的 Run 被自愈后的中断原因（与 `manager-agent` 的 `INTERRUPTED_RUNNING` 逐字一致）。
const INTERRUPTED_RUNNING: &str = "服务重启，执行中的 Run 已中断";

/// 用例入口。
pub(super) fn case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { require(context).await })
}

/// 两段都在同一个 `AGENT_HOME` 上完成：每次强杀后复用同一端口重启。
async fn require(context: &TestContext) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    notes.extend(plain(context).await?);
    notes.extend(after_tool_round(context).await?);
    Ok(notes)
}

/// 一、普通执行被强杀：`PAUSED` → 占用 + 发消息 → 继续到底。
async fn plain(context: &TestContext) -> Result<Vec<String>> {
    let server = super::start(context).await?;
    let session = flow::create_session(context, "强杀恢复").await?;
    let (server, run_id) = super::interrupt_running(context, server, &session.id).await?;

    // 自愈：带固定中断原因的可恢复中间态，没有结束时间（验收 S-2）。
    let paused = run_of(context, &run_id).await?;
    ensure!(
        paused.status == RunStatus::Paused
            && paused.interrupted_reason.as_deref() == Some(INTERRUPTED_RUNNING)
            && paused.finished_at_ms.is_none()
            && paused.resume_count == 0,
        "强杀重启后应是带中断原因、没有结束时间的 PAUSED：{paused:?}"
    );
    // 探活口径：`PAUSED` 不是执行中（验收 S-7）。
    let health = context.client().probe_health().await.map_err(ClientError::from_call)?;
    ensure!(
        health.running_run_count == 0,
        "PAUSED 不应计入 runningRunCount，实际 {}",
        health.running_run_count
    );

    // 恢复只读 SQLite：删掉这条会话的事件目录之后仍能恢复（验收 R-8）。
    let events = context.home().join("data").join("events").join(&session.id);
    if events.is_dir() {
        std::fs::remove_dir_all(&events).with_context(|| format!("删除事件目录失败：{}", events.display()))?;
    }

    let mut link = Reopened::open(context, &session.id).await?;
    let trigger = flow::send_message(context, &session.id, &format!("继续 {}", context.token())).await?;
    // 恢复段在 QUEUED 段之前：先等到被恢复 Run 的终态事件，再等本次消息的新 Run。
    let recovered = flow::drain_run(&mut link.stream, &run_id).await?;
    ensure!(
        recovered.terminal.event == lib_bin_cli::sdk::RunEventKind::RunCompleted,
        "被恢复的 Run 应以 run_completed 收尾，实际 {:?}（{}）",
        recovered.terminal.event,
        recovered.terminal.error.clone().unwrap_or_default()
    );
    let fresh = flow::drain_run(&mut link.stream, &trigger.run.id).await?;
    ensure!(
        fresh.terminal.event == lib_bin_cli::sdk::RunEventKind::RunCompleted,
        "本次消息的新 Run 应以 run_completed 收尾，实际 {:?}",
        fresh.terminal.event
    );

    let finished = run_of(context, &run_id).await?;
    ensure!(
        finished.status == RunStatus::Completed
            && finished.resume_count == 1
            && finished.finished_at_ms.is_some()
            && finished.interrupted_reason.is_none(),
        "恢复完成后应是 COMPLETED + resumeCount 1 + 有结束时间 + 中断原因已清空：{finished:?}"
    );

    // 一个 Run 一个结局：助手消息只有一条（验收 S-5）。
    let assistants = assistants_of(context, &session.id, &run_id).await?;
    ensure!(
        assistants.len() == 1,
        "恢复完成后该 Run 的助手消息应只有 1 条，实际 {} 条：{assistants:?}",
        assistants.len()
    );

    // 序号不重置（验收 R-4 的序号半边）：普通执行只有起点一条检查点，恢复既不重置也不重写它。
    let history = history_of(context, &run_id).await?;
    let sequence = history.steps.iter().map(|step| step.seq_no).collect::<Vec<_>>();
    let expected = (1..=i64::try_from(history.steps.len()).unwrap_or(0)).collect::<Vec<_>>();
    ensure!(
        history.resume_count == 1 && sequence == expected && sequence.first() == Some(&1),
        "恢复后的步骤应从 1 连续且不重置：resumeCount = {}，steps = {sequence:?}",
        history.resume_count
    );

    link.release(context).await?;
    context.observe_metrics(server.metrics().await);
    server.stop().await?;
    Ok(vec![
        format!("强杀重启后 Run 是 PAUSED（原因「{INTERRUPTED_RUNNING}」，无结束时间，runningRunCount = 0）"),
        format!(
            "删掉事件目录后仍恢复完成：status = COMPLETED，resumeCount = 1，助手消息 1 条，步骤 {:?}",
            sequence
        ),
    ])
}

/// 二、一轮工具之后被强杀：已提交的助手消息与轮次逐字符不变，序号接着递增。
async fn after_tool_round(context: &TestContext) -> Result<Vec<String>> {
    let server = super::start(context).await?;
    let session = flow::create_session(context, "工具轮后强杀").await?;
    let mut link = Reopened::open(context, &session.id).await?;
    // 场景关键字在前、工具剧本在后：Mock 的剧本是「`tool:` 之后的整段内容即 JSON」，
    // 因此 JSON 必须是最后一段；`mock:slow` 只影响分块间隔，放哪儿都能被识别。
    let content = format!("mock:slow {}\n{}", context.token(), prompt(one_round()));
    let sent = flow::send_message(context, &session.id, &content).await?;
    // 工具轮提交之后模型还会再请求一次（这一轮是慢速）：等「结果事件之后的那段增量」再强杀，
    // 此时「工具批次已提交、模型轮次还没提交」是合法的可恢复形态。
    wait_committed_round(&mut link.stream, &sent.run.id).await?;
    let server = super::crash(context, server).await?;
    drop(link);

    let paused = run_of(context, &sent.run.id).await?;
    ensure!(
        paused.status == RunStatus::Paused,
        "工具轮后强杀应留下 PAUSED，实际 {:?}",
        paused.status
    );
    let before = assistants_of(context, &session.id, &sent.run.id).await?;
    ensure!(
        before.len() == 1,
        "强杀前应已提交 1 条助手消息（工具调用），实际 {} 条",
        before.len()
    );
    let steps_before = history_of(context, &sent.run.id).await?.steps;

    let mut link = Reopened::open(context, &session.id).await?;
    let trigger = flow::send_message(context, &session.id, &format!("继续 {}", context.token())).await?;
    let recovered = flow::drain_run(&mut link.stream, &sent.run.id).await?;
    ensure!(
        recovered.terminal.event == lib_bin_cli::sdk::RunEventKind::RunCompleted,
        "被恢复的 Run 应以 run_completed 收尾，实际 {:?}（{}）",
        recovered.terminal.event,
        recovered.terminal.error.clone().unwrap_or_default()
    );
    let _ = flow::drain_run(&mut link.stream, &trigger.run.id).await?;

    // 崩溃前已提交的助手消息必须逐字段不变；恢复只是在它之后接着写（收尾的那条回复）。
    let after = assistants_of(context, &session.id, &sent.run.id).await?;
    ensure!(
        after.len() == before.len() + 1,
        "恢复后应只多一条收尾回复：{} -> {} 条",
        before.len(),
        after.len()
    );
    ensure!(
        after[0].id == before[0].id
            && after[0].content == before[0].content
            && after[0].tool_calls == before[0].tool_calls
            && after[0].created_at_ms == before[0].created_at_ms,
        "恢复不应改写已提交的助手消息：{after:?} != {before:?}"
    );
    // 收尾那一轮是纯文本、不再产生工具边界，因此步骤只保证「不丢、不改、不重置」。
    let steps_after = history_of(context, &sent.run.id).await?.steps;
    ensure!(
        steps_after.len() >= steps_before.len(),
        "恢复不应丢掉已提交的步骤：{} -> {}",
        steps_before.len(),
        steps_after.len()
    );
    ensure!(
        steps_after
            .iter()
            .zip(steps_before.iter())
            .all(|(after, before)| after.seq_no == before.seq_no && after.phase == before.phase),
        "崩溃前已提交的步骤在恢复后应逐条不变：{steps_before:?} -> {steps_after:?}"
    );

    link.release(context).await?;
    context.observe_metrics(server.metrics().await);
    server.stop().await?;
    Ok(vec![format!(
        "一轮工具之后强杀：已提交的助手消息 {} 条内容不变，步骤 {} 条在恢复后前缀逐条相同、总数 {}",
        after.len(),
        steps_before.len(),
        steps_after.len()
    )])
}

/// 读到「一次工具调用完成之后的第一条增量」为止。
///
/// `tool_call_completed` 是在本轮事务提交之后才发出的（D-P3-024），因此看到它就说明
/// 工具批次已经落库；之后的增量属于下一轮模型请求，此刻强杀正好落在两个边界之间。
async fn wait_committed_round(stream: &mut lib_bin_cli::client::EventStream, run_id: &str) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut committed = false;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        ensure!(!remaining.is_zero(), "等工具轮提交超时");
        let received = tokio::time::timeout(remaining, stream.recv())
            .await
            .context("等工具轮提交超时")?;
        let Some(event) = received? else {
            anyhow::bail!("下行流在工具轮提交之前结束");
        };
        let Some(run_event) = flow::run_event_of(&event, run_id) else {
            continue;
        };
        match run_event.event {
            lib_bin_cli::sdk::RunEventKind::ToolCallCompleted => committed = true,
            lib_bin_cli::sdk::RunEventKind::MessageDelta if committed => return Ok(()),
            _ => {}
        }
    }
}

/// 一轮 `calculator` 的工具脚本：之后模型再请求一次就是普通的慢速收尾。
fn one_round() -> serde_json::Value {
    serde_json::json!({
        "rounds": [
            {"calls": [{"name": "calculator", "arguments": {"expression": "1+1"}}]},
        ],
    })
}

/// `api/run/get`。
async fn run_of(context: &TestContext, run_id: &str) -> Result<Run> {
    flow::run_get(context, run_id).await
}

/// `api/run/history`。
async fn history_of(context: &TestContext, run_id: &str) -> Result<lib_bin_cli::sdk::RunHistory> {
    context
        .client()
        .run_history(RunHistoryParams {
            run_id: run_id.to_owned(),
        })
        .await
        .map_err(ClientError::from_call)
        .map_err(Into::into)
}

/// 某个 Run 的助手消息（`api/message/list` 过滤 `runId` 与角色）。
async fn assistants_of(context: &TestContext, session_id: &str, run_id: &str) -> Result<Vec<Message>> {
    let messages = context
        .client()
        .message_list(MessageListParams {
            session_id: session_id.to_owned(),
        })
        .await
        .map_err(ClientError::from_call)?;
    Ok(messages
        .into_iter()
        .filter(|message| message.role == MessageRole::Assistant && message.run_id.as_deref() == Some(run_id))
        .collect())
}
