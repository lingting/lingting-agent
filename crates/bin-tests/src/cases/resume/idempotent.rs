//! `resume.idempotent`（P6A-010 / 验收 `R-3`）：同一个 `PAUSED` Run 在两条消息连续到达时
//! 只被恢复一次——`resumeCount` 只加一，工具审计行数不增加（没有重复执行任何调用）。
//!
//! 断言只引用接口响应与库内行：`run/get` 的 `resumeCount`、`tool_calls` 的行数。
//! **不读日志、不读事件文件。**

use anyhow::{Result, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiClient, RunEventKind, RunGetParams, RunStatus};

use crate::case::CaseFuture;
use crate::util::context::TestContext;
use crate::util::flow::{self, Reopened};
use crate::util::store;

/// 用例入口。
pub(super) fn case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { require(context).await })
}

async fn require(context: &TestContext) -> Result<Vec<String>> {
    let server = super::start(context).await?;
    let session = flow::create_session(context, "恢复幂等").await?;
    let (server, paused_id) = super::interrupt_running(context, server, &session.id).await?;
    let before = tool_rows(context, &paused_id).await?;

    // 占用后连续两条消息：唤醒通道容量为 1，但「恢复待处理」是独立标记，两条都要被处理，
    // 而被恢复的仍然只有那一个 Run（D-P6-006 的推进幂等）。
    let mut link = Reopened::open(context, &session.id).await?;
    let first = flow::send_message(context, &session.id, &format!("第一条 {}", context.token())).await?;
    let second = flow::send_message(context, &session.id, &format!("第二条 {}", context.token())).await?;

    // 恢复段在 QUEUED 段之前：先等被恢复 Run 的终态，再依次等两条消息的 Run。
    let recovered = flow::drain_run(&mut link.stream, &paused_id).await?;
    ensure!(
        recovered.terminal.event == RunEventKind::RunCompleted,
        "被恢复的 Run 应以 run_completed 收尾，实际 {:?}（{}）",
        recovered.terminal.event,
        recovered.terminal.error.clone().unwrap_or_default()
    );
    let _ = flow::drain_run(&mut link.stream, &first.run.id).await?;
    let _ = flow::drain_run(&mut link.stream, &second.run.id).await?;

    let after = run_of(context, &paused_id).await?;
    ensure!(
        after.status == RunStatus::Completed && after.resume_count == 1,
        "两条消息连续到达时只应恢复一次（resumeCount = 1）：{after:?}"
    );
    let rows_after = tool_rows(context, &paused_id).await?;
    ensure!(
        rows_after == before,
        "恢复不应写入或重复任何工具调用：{rows_after} != {before}"
    );

    link.release(context).await?;
    context.observe_metrics(server.metrics().await);
    server.stop().await?;
    Ok(vec![
        format!("连续两条消息后 resumeCount = {}（只恢复一次）", after.resume_count),
        format!("该 Run 的工具审计行数保持 {rows_after}（没有重复执行）"),
    ])
}

/// 该 Run 的审计行数：含 `PENDING` 行（这条 Run 没有工具调用，正常为 0）。
async fn tool_rows(context: &TestContext, run_id: &str) -> Result<usize> {
    Ok(store::tool_calls(context, run_id).await?.len())
}

/// `api/run/get`。
async fn run_of(context: &TestContext, run_id: &str) -> Result<lib_bin_cli::sdk::Run> {
    context
        .client()
        .run_get(RunGetParams {
            run_id: run_id.to_owned(),
        })
        .await
        .map_err(ClientError::from_call)
        .map_err(Into::into)
}
