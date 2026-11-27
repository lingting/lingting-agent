//! `resume.broken`（P6A-007 / 验收 `R-6`、`R-7`、`C-6`、`E-4`）：损坏的检查点与不一致的轨迹
//! 一律判「不可恢复」——Run 直接落 `FAILED` + 固定文案，不重试、不降级成从头执行。
//!
//! 七种情形各用一个被强杀后的 Run：
//!
//! ① 序号与指针不一致；② 检查点记录的消息上界不存在；③ 轨迹里出现孤儿 TOOL 结果；
//! ④ `PENDING` 开始标记出现在更早的工具组；⑤ 检查点轮次与轨迹不符；⑥ 未知格式版本；
//! ⑦ 没有任何检查点（升级前的旧 Run）：**不判死**，保持可取消的中断态。
//!
//! 损坏由用例直接注入库内行（这是「制造数据问题」而不是「读日志」），结论只从接口读：
//! `run/get` 的 `status` / `error`、`run/history` 的 `steps`、`tool/audit/list` 的行数。
//! 全部文案都要能被 `AGENT_HOME` 绝对路径检索命中 0 次（`E-4`）。

use std::future::Future;
use std::result::Result as StdResult;

use anyhow::{Context, Result, anyhow, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiClient, RunEventKind, RunGetParams, RunHistoryParams, RunStatus};

use super::PausedRun;
use crate::case::CaseFuture;
use crate::util::context::TestContext;
use crate::util::flow::{self, Reopened};
use crate::util::store;

/// 五类一致性损坏的固定文案（与 `agent-runtime` 的 `error::RESUME_*` 逐字一致）。
const RESUME_GAP: &str = "检查点序号不连续，Run 不可恢复";
const RESUME_MESSAGES_LOST: &str = "检查点记录的消息缺失，Run 不可恢复";
const RESUME_TRACE_BROKEN: &str = "工具轨迹与检查点不一致，Run 不可恢复";
const RESUME_PENDING_LEAK: &str = "工具开始标记出现在非最后的工具组，Run 不可恢复";
const RESUME_PHASE_MISMATCH: &str = "检查点阶段与工具轨迹不一致，Run 不可恢复";
/// 未知格式版本的固定文案（D-P6-003 / 验收 `C-6`）。
const CHECKPOINT_UNSUPPORTED: &str = "检查点格式版本不受支持，Run 不可恢复";

/// 用例入口。
pub(super) fn case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { require(context).await })
}

async fn require(context: &TestContext) -> Result<Vec<String>> {
    let server = super::start(context).await?;
    // 第一轮：三个被强杀的 Run，各自承担一种损坏。
    let (server, first) = super::interrupt_batch(context, server, "损坏一", 3).await?;
    let (server, second) = super::interrupt_batch(context, server, "损坏二", 4).await?;
    let paused = first.into_iter().chain(second).collect::<Vec<_>>();

    let pool = store::pool(context).await?;
    let mut notes = Vec::new();
    // ① 指针与最近一条检查点不一致：`last_checkpoint_seq` 指向不存在的序号。
    let gap = &paused[0];
    sql(sqlx::query("UPDATE runs SET last_checkpoint_seq = 2 WHERE id = ?")
        .bind(&gap.run_id)
        .execute(&pool))
    .await?;
    notes.extend(expect_failed(context, gap, RESUME_GAP).await?);

    // ② 检查点记录的消息上界在库里找不到。
    let lost = &paused[1];
    sql(
        sqlx::query("UPDATE run_checkpoints SET last_message_seq = 999999 WHERE run_id = ?")
            .bind(&lost.run_id)
            .execute(&pool),
    )
    .await?;
    notes.extend(expect_failed(context, lost, RESUME_MESSAGES_LOST).await?);

    // ③ 孤儿 TOOL 结果：找不到对应的助手调用。
    let broken = &paused[2];
    sql(sqlx::query(
        "INSERT INTO messages (id, session_id, run_id, role, content, tool_call_id, tool_name, seq, created_at_ms) \
         SELECT 'orphan-' || id, session_id, id, 'TOOL', '孤儿结果', 'call-orphan', 'calculator', \
                (SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE session_id = ?), created_at_ms \
         FROM runs WHERE id = ?",
    )
    .bind(&broken.session_id)
    .bind(&broken.run_id)
    .execute(&pool))
    .await?;
    notes.extend(expect_failed(context, broken, RESUME_TRACE_BROKEN).await?);

    // ④ `PENDING` 开始标记出现在已提交工具组之前的轮次。
    let leak = &paused[3];
    sql(sqlx::query(
        "INSERT INTO tool_calls (id, run_id, session_id, tool_call_id, tool_name, file_access, status, \
         arguments, duration_ms, tool_round, created_at_ms, started_at_ms) \
         VALUES ('leak-' || ?, ?, ?, 'call-leak', 'calculator', 'READ', 'PENDING', '{}', 0, 5, 1, 1)",
    )
    .bind(&leak.run_id)
    .bind(&leak.run_id)
    .bind(&leak.session_id)
    .execute(&pool))
    .await?;
    notes.extend(expect_failed(context, leak, RESUME_PENDING_LEAK).await?);

    // ⑤ 检查点记录的轮次与已提交的工具组数不一致。
    let mismatch = &paused[4];
    sql(
        sqlx::query("UPDATE run_checkpoints SET tool_round = 3 WHERE run_id = ?")
            .bind(&mismatch.run_id)
            .execute(&pool),
    )
    .await?;
    notes.extend(expect_failed(context, mismatch, RESUME_PHASE_MISMATCH).await?);

    // ⑥ 未知格式版本：不猜测、不降级（验收 C-6）。
    let unsupported = &paused[5];
    sql(
        sqlx::query("UPDATE run_checkpoints SET format_version = 99 WHERE run_id = ?")
            .bind(&unsupported.run_id)
            .execute(&pool),
    )
    .await?;
    notes.extend(expect_failed(context, unsupported, CHECKPOINT_UNSUPPORTED).await?);

    // ⑦ 没有任何检查点：不进入执行，也不判死（R-7）。
    let legacy = &paused[6];
    sql(sqlx::query("DELETE FROM run_checkpoints WHERE run_id = ?")
        .bind(&legacy.run_id)
        .execute(&pool))
    .await?;
    notes.extend(expect_kept(context, legacy).await?);

    pool.close().await;
    // 快照口径（`O-2`）：不可恢复次数等于本用例构造的损坏场景数（①~⑥ 六类）；
    // 「没有检查点」记拒绝而不是不可恢复（⑦ 不判死）。
    let snapshot = server.metrics().await.map_err(|reason| anyhow!("未采集：{reason}"))?;
    ensure!(
        snapshot.resume.resume_unrecoverable == 6,
        "resumeUnrecoverable 应等于损坏场景数 6，实际 {}",
        snapshot.resume.resume_unrecoverable
    );
    ensure!(
        snapshot.resume.resume_rejected == 1,
        "没有检查点的 Run 只记一次拒绝，实际 {}",
        snapshot.resume.resume_rejected
    );
    context.observe_metrics(Ok(snapshot));
    server.stop().await?;
    Ok(notes)
}

/// 触发一次恢复，断言落 `FAILED` + 指定文案、且没有执行任何工具调用。
async fn expect_failed(context: &TestContext, run: &PausedRun, expected: &str) -> Result<Vec<String>> {
    let audit_before = store::tool_calls(context, &run.run_id).await?.len();
    let mut link = Reopened::open(context, &run.session_id).await?;
    let _trigger = flow::send_message(context, &run.session_id, &format!("继续 {}", context.token())).await?;
    let drained = flow::drain_run(&mut link.stream, &run.run_id).await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunFailed,
        "损坏的检查点应变 run_failed，实际 {:?}",
        drained.terminal.event
    );
    ensure!(
        drained.terminal.error.as_deref() == Some(expected),
        "run_failed 的文案应是「{expected}」，实际 {:?}",
        drained.terminal.error
    );

    let run_after = run_of(context, &run.run_id).await?;
    ensure!(
        run_after.status == RunStatus::Failed && run_after.error.as_deref() == Some(expected),
        "Run 应落 FAILED 且 error 是固定文案：{run_after:?}"
    );
    // E-4：文案里不含环境绝对路径。
    ensure!(
        !expected.contains(&context.home().to_string_lossy().to_string()),
        "固定文案不应含 AGENT_HOME 绝对路径：{expected}"
    );
    // 不重试、不从头执行：审计行数不增加。
    let audit_after = store::tool_calls(context, &run.run_id).await?.len();
    ensure!(
        audit_after == audit_before,
        "不可恢复不应执行任何工具调用：{audit_after} != {audit_before}"
    );

    link.release(context).await?;
    Ok(vec![format!("{expected}（status = FAILED，审计行数不变）")])
}

/// 触发一次恢复，断言「没有检查点」既不执行也不改行（R-7）。
async fn expect_kept(context: &TestContext, run: &PausedRun) -> Result<Vec<String>> {
    let before = run_of(context, &run.run_id).await?;
    let mut link = Reopened::open(context, &run.session_id).await?;
    let trigger = flow::send_message(context, &run.session_id, &format!("继续 {}", context.token())).await?;
    // 恢复不进入执行：等本次消息的新 Run 跑完即可，被拒绝的 Run 不会有任何事件。
    let _ = flow::drain_run(&mut link.stream, &trigger.run.id).await?;

    let after = run_of(context, &run.run_id).await?;
    ensure!(
        after.status == RunStatus::Paused
            && after.updated_at_ms == before.updated_at_ms
            && after.resume_count == before.resume_count
            && after.finished_at_ms.is_none(),
        "没有检查点的 Run 不应被判定或改写：{after:?} != {before:?}"
    );
    let history = context
        .client()
        .run_history(RunHistoryParams {
            run_id: run.run_id.clone(),
        })
        .await
        .map_err(ClientError::from_call)?;
    ensure!(
        history.steps.is_empty(),
        "没有检查点的 Run 步骤应为空：{:?}",
        history.steps
    );
    ensure!(
        history.interrupted_reason.is_some(),
        "中断原因应可读（NoCheckpoint 不覆盖自愈写下的原因）：{history:?}"
    );
    link.release(context).await?;
    Ok(vec![format!(
        "无检查点的 Run：保持 PAUSED（resumeCount = {}，steps = 0，中断原因读得到）",
        after.resume_count
    )])
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

/// 执行一条注入语句：0 行即失败（注入不成立，后面的断言就没有意义）。
async fn sql(statement: impl Future<Output = StdResult<sqlx::sqlite::SqliteQueryResult, sqlx::Error>>) -> Result<()> {
    let affected = statement.await.context("执行注入语句失败")?.rows_affected();
    ensure!(affected > 0, "注入语句没有命中任何行");
    Ok(())
}
