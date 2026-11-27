//! `resume.metrics`（P6A-014 / 验收 `O-1`、`O-2`）：`metrics` 快照的 `resume` 段六项，
//! 且数字能与接口侧事实逐项复算。
//!
//! 复算口径：`checkpointsWritten` == 该服务端上各 Run 的 `api/run/history` 步骤数之和；
//! `resumeSucceeded` == 各 Run 的 `resumeCount` 之和；`resumeAttempts` == 进入判定的次数；
//! 这一轮既没有拒绝（没有无检查点的旧 Run）也没有不可恢复（没有注入损坏），两者应为 0。
//!
//! 数字只来自 `GET api/probe/metrics` 与接口响应。**不读日志、不读事件文件。**

use anyhow::{Result, anyhow, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiClient, RunEventKind, RunGetParams, RunHistoryParams};

use crate::case::CaseFuture;
use crate::util::context::TestContext;
use crate::util::flow::{self, Reopened};

/// 用例入口。
pub(super) fn case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { require(context).await })
}

async fn require(context: &TestContext) -> Result<Vec<String>> {
    let server = super::start(context).await?;
    let session = flow::create_session(context, "恢复指标").await?;
    let (server, paused_id) = super::interrupt_running(context, server, &session.id).await?;

    // 崩溃前已写的检查点条数：进程重启后计数器从 0 开始，因此复算时要把它扣掉。
    let before_crash = history_of(context, &paused_id).await?.steps.len();

    // 恢复一次：`PAUSED → RUNNING` 命中一次，检查点接着写。
    let mut link = Reopened::open(context, &session.id).await?;
    let trigger = flow::send_message(context, &session.id, &format!("继续 {}", context.token())).await?;
    let recovered = flow::drain_run(&mut link.stream, &paused_id).await?;
    ensure!(
        recovered.terminal.event == RunEventKind::RunCompleted,
        "被恢复的 Run 应以 run_completed 收尾，实际 {:?}",
        recovered.terminal.event
    );
    let _ = flow::drain_run(&mut link.stream, &trigger.run.id).await?;
    link.release(context).await?;

    // 接口侧事实：本进程写下的检查点条数 = 被恢复 Run 新增的步骤 + 新 Run 的全部步骤。
    let recovered_history = history_of(context, &paused_id).await?;
    let fresh_history = history_of(context, &trigger.run.id).await?;
    let steps = i64::try_from(recovered_history.steps.len().saturating_sub(before_crash)).unwrap_or_default()
        + i64::try_from(fresh_history.steps.len()).unwrap_or_default();
    let resumes = recovered_history.resume_count + fresh_history.resume_count;
    let run = context
        .client()
        .run_get(RunGetParams {
            run_id: paused_id.clone(),
        })
        .await
        .map_err(ClientError::from_call)?;
    ensure!(run.resume_count == 1, "被恢复的 Run 应只推进一次：{run:?}");

    let capture = server.metrics().await;
    let metrics = capture.map_err(|reason| anyhow!("未采集：{reason}"))?;
    // 六项的字段名是契约（验收 O-1）：`resume` 段序列化后就是这六个键（与其余各段同为 snake_case）。
    let keys = sorted_keys(&serde_json::to_value(metrics.resume)?)?;
    ensure!(
        keys == [
            "checkpoints_pruned",
            "checkpoints_written",
            "resume_attempts",
            "resume_rejected",
            "resume_succeeded",
            "resume_unrecoverable",
        ],
        "resume 段应是六个固定字段：{keys:?}"
    );

    let resume = metrics.resume;
    ensure!(
        i64::try_from(resume.checkpoints_written).unwrap_or_default() == steps,
        "checkpointsWritten 应等于两个 Run 的步骤数之和 {}，实际 {}",
        steps,
        resume.checkpoints_written
    );
    ensure!(
        i64::try_from(resume.resume_succeeded).unwrap_or_default() == resumes,
        "resumeSucceeded 应等于各 Run 的 resumeCount 之和 {}，实际 {}",
        resumes,
        resume.resume_succeeded
    );
    ensure!(
        resume.resume_attempts == 1,
        "只拾起过一个 PAUSED Run，resumeAttempts 应为 1，实际 {}",
        resume.resume_attempts
    );
    ensure!(
        resume.resume_rejected == 0 && resume.resume_unrecoverable == 0 && resume.checkpoints_pruned == 0,
        "这一轮没有拒绝、损坏与清理：{resume:?}"
    );

    context.observe_metrics(Ok(metrics));
    server.stop().await?;
    Ok(vec![
        format!(
            "resume 段六字段：写入 {} == 步骤数 {}，推进 {} == resumeCount 之和 {}",
            resume.checkpoints_written, steps, resume.resume_succeeded, resumes
        ),
        format!(
            "判定 {} 次，拒绝 {}，不可恢复 {}，清理 {}（本轮均为 0）",
            resume.resume_attempts, resume.resume_rejected, resume.resume_unrecoverable, resume.checkpoints_pruned
        ),
    ])
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

/// 对象字段名排序后返回：白名单比对与字段顺序无关。
fn sorted_keys(value: &serde_json::Value) -> Result<Vec<String>> {
    let object = value.as_object().ok_or_else(|| anyhow!("resume 段应是 JSON 对象"))?;
    let mut keys = object.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    Ok(keys)
}
