//! `resume.notify`（P6A-008 / 验收 `R-5`）：未配对的工具调用**一律不执行**，改为落一条
//! `ERROR` 审计与一条 TOOL 结果消息，文本是「上次执行被中断：<简短原因>」。
//!
//! 三种形态各一个被强杀后的 Run：
//!
//! - 已配对（助手调用 + TOOL 结果都在）：不执行、不再追加任何审计行；
//! - 有 `PENDING` 开始标记（执行前写过、执行后没写完）：不执行，把那条标记收尾成 `ERROR`；
//! - 旧库行（没有开始标记）：不执行，补一条 `ERROR` 审计。
//!
//! 断言只引用接口响应与库内行：`run/get`、`message/list`、`tool/audit/list`、`tool_calls` 行数。
//! 通知文本的红线（不含参数原文、不含环境路径）在这里一起断言。**不读日志、不读事件文件。**

use std::future::Future;
use std::result::Result as StdResult;

use anyhow::{Context, Result, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{
    ApiClient, MessageListParams, MessageRole, RunEventKind, RunGetParams, RunStatus, ToolAuditListParams,
};
use lib_core::time;

use super::PausedRun;
use crate::case::CaseFuture;
use crate::util::context::TestContext;
use crate::util::flow::{self, Reopened};
use crate::util::store;

/// 中断通知的固定前缀（与 `agent-runtime` 的 `error::INTERRUPTED_NOTICE` 逐字一致）。
const NOTICE: &str = "上次执行被中断：服务重启";
/// 注入的调用标识与参数：参数原文不该出现在回灌文本里。
const CALL_ID: &str = "call-resume";
const ARGUMENTS: &str = r#"{"expression":"1+1"}"#;

/// 用例入口。
pub(super) fn case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { require(context).await })
}

async fn require(context: &TestContext) -> Result<Vec<String>> {
    let server = super::start(context).await?;
    let (server, runs) = super::interrupt_batch(context, server, "通知", 3).await?;
    let pool = store::pool(context).await?;
    let mut notes = Vec::new();
    notes.extend(paired(context, &pool, &runs[0]).await?);
    notes.extend(pending(context, &pool, &runs[1]).await?);
    notes.extend(legacy(context, &pool, &runs[2]).await?);
    pool.close().await;
    context.observe_metrics(server.metrics().await);
    server.stop().await?;
    Ok(notes)
}

/// 一、已配对：助手调用与 TOOL 结果都在，恢复时一次都不执行。
async fn paired(context: &TestContext, pool: &sqlx::SqlitePool, run: &PausedRun) -> Result<Vec<String>> {
    assistant_message(pool, run).await?;
    tool_message(pool, run, "1+1=2").await?;
    round_of_checkpoint(pool, run, 1).await?;
    let audit_before = audit(context, &run.run_id).await?;
    ensure!(
        audit_before.is_empty(),
        "已配对形态不该有审计行（没有写过开始标记）：{audit_before:?}"
    );

    finish_resume(context, run).await?;
    let audit_after = audit(context, &run.run_id).await?;
    ensure!(
        audit_after.is_empty(),
        "已配对的调用不应被执行、也不应新增审计行：{audit_after:?}"
    );
    Ok(vec![
        "已配对：工具调用次数 0（审计行数 0），恢复直接用已有轨迹".to_owned(),
    ])
}

/// 二、有 `PENDING` 开始标记：不执行，把标记收尾成 `ERROR` 并回灌通知。
async fn pending(context: &TestContext, pool: &sqlx::SqlitePool, run: &PausedRun) -> Result<Vec<String>> {
    assistant_message(pool, run).await?;
    round_of_checkpoint(pool, run, 1).await?;
    start_marker(pool, run).await?;
    finish_resume(context, run).await?;

    let audit = audit(context, &run.run_id).await?;
    ensure!(
        audit.len() == 1,
        "有开始标记的调用应只收尾成 1 条审计行，实际 {} 条：{audit:?}",
        audit.len()
    );
    ensure!(
        audit[0].tool_call_id == CALL_ID,
        "审计行的调用标识应是被中断的那次：{audit:?}"
    );
    ensure!(
        audit[0].status == lib_bin_cli::sdk::ToolCallStatus::Error,
        "审计状态应复用 ERROR：{audit:?}"
    );
    notes_of_messages(context, run, &audit[0].result.clone().unwrap_or_default()).await
}

/// 三、旧库行（没有开始标记）：不执行，但同样补一条 `ERROR` 审计与 TOOL 结果。
async fn legacy(context: &TestContext, pool: &sqlx::SqlitePool, run: &PausedRun) -> Result<Vec<String>> {
    assistant_message(pool, run).await?;
    round_of_checkpoint(pool, run, 1).await?;
    finish_resume(context, run).await?;

    let audit = audit(context, &run.run_id).await?;
    ensure!(
        audit.len() == 1,
        "没有开始标记的调用也应补 1 条审计行，实际 {} 条：{audit:?}",
        audit.len()
    );
    ensure!(
        audit[0].status == lib_bin_cli::sdk::ToolCallStatus::Error,
        "补写的审计状态应是 ERROR：{audit:?}"
    );
    notes_of_messages(context, run, &audit[0].result.clone().unwrap_or_default()).await
}

/// 回灌文本与 TOOL 结果消息的红线：固定前缀 + 白名单原因，不含参数原文与环境路径。
async fn notes_of_messages(context: &TestContext, run: &PausedRun, result: &str) -> Result<Vec<String>> {
    ensure!(result.contains(NOTICE), "审计结果应是「{NOTICE}」，实际：{result}");
    ensure!(
        !result.contains("1+1") && !result.contains(ARGUMENTS),
        "通知文本不应含调用参数原文：{result}"
    );
    ensure!(
        !result.contains(&context.home().to_string_lossy().to_string()),
        "通知文本不应含 AGENT_HOME 绝对路径：{result}"
    );

    let messages = context
        .client()
        .message_list(MessageListParams {
            session_id: run.session_id.clone(),
        })
        .await
        .map_err(ClientError::from_call)?;
    let tool = messages
        .iter()
        .find(|message| message.role == MessageRole::Tool && message.tool_call_id.as_deref() == Some(CALL_ID));
    let tool = tool.context("恢复后应有一条 TOOL 结果消息")?;
    ensure!(
        tool.content.contains(NOTICE),
        "TOOL 结果消息应是同一条通知文本，实际：{}",
        tool.content
    );
    Ok(vec![format!(
        "未配对调用不执行，回灌「{NOTICE}」（审计 1 条 ERROR，TOOL 消息 1 条，不含参数原文与绝对路径）"
    )])
}

/// 触发恢复并等这个 Run 自己跑完（通知之后模型继续收尾）。
async fn finish_resume(context: &TestContext, run: &PausedRun) -> Result<()> {
    let mut link = Reopened::open(context, &run.session_id).await?;
    let _trigger = flow::send_message(context, &run.session_id, &format!("继续 {}", context.token())).await?;
    let drained = flow::drain_run(&mut link.stream, &run.run_id).await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCompleted,
        "恢复后 Run 应以 run_completed 收尾，实际 {:?}（{}）",
        drained.terminal.event,
        drained.terminal.error.clone().unwrap_or_default()
    );
    let recovered = context
        .client()
        .run_get(RunGetParams {
            run_id: run.run_id.clone(),
        })
        .await
        .map_err(ClientError::from_call)?;
    ensure!(
        recovered.status == RunStatus::Completed && recovered.resume_count == 1,
        "恢复完成后应是 COMPLETED + resumeCount 1：{recovered:?}"
    );
    link.release(context).await?;
    Ok(())
}

/// 注入一条助手消息：带一次 `calculator` 调用，成为轨迹里的最后一个工具组。
async fn assistant_message(pool: &sqlx::SqlitePool, run: &PausedRun) -> Result<()> {
    let tool_calls = format!(
        r#"[{{"id":"{CALL_ID}","name":"calculator","arguments":{}}}]"#,
        esc(ARGUMENTS)
    );
    insert(
        sqlx::query(
            "INSERT INTO messages (id, session_id, run_id, role, content, tool_calls, seq, created_at_ms) \
             VALUES (?, ?, ?, 'ASSISTANT', '调用工具', ?, \
                     (SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE session_id = ?), ?)",
        )
        .bind(format!("assist-{}", run.run_id))
        .bind(&run.session_id)
        .bind(&run.run_id)
        .bind(&tool_calls)
        .bind(&run.session_id)
        .bind(time::now_millis())
        .execute(pool),
    )
    .await
}

/// 注入一条 TOOL 结果消息（已配对形态用）。
async fn tool_message(pool: &sqlx::SqlitePool, run: &PausedRun, content: &str) -> Result<()> {
    insert(
        sqlx::query(
            "INSERT INTO messages (id, session_id, run_id, role, content, tool_call_id, tool_name, seq, created_at_ms) \
             VALUES (?, ?, ?, 'TOOL', ?, ?, 'calculator', \
                     (SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE session_id = ?), ?)",
        )
        .bind(format!("tool-{}", run.run_id))
        .bind(&run.session_id)
        .bind(&run.run_id)
        .bind(content)
        .bind(CALL_ID)
        .bind(&run.session_id)
        .bind(time::now_millis())
        .execute(pool),
    )
    .await
}

/// 注入执行前写下的开始标记（`PENDING` 形态用）。
async fn start_marker(pool: &sqlx::SqlitePool, run: &PausedRun) -> Result<()> {
    insert(
        sqlx::query(
            "INSERT INTO tool_calls (id, run_id, session_id, tool_call_id, tool_name, file_access, status, \
             arguments, duration_ms, tool_round, created_at_ms, started_at_ms) \
             VALUES (?, ?, ?, ?, 'calculator', 'READ', 'PENDING', ?, 0, 1, ?, ?)",
        )
        .bind(format!("pending-{}", run.run_id))
        .bind(&run.run_id)
        .bind(&run.session_id)
        .bind(CALL_ID)
        .bind(ARGUMENTS)
        .bind(time::now_millis())
        .bind(time::now_millis())
        .execute(pool),
    )
    .await
}

/// 把最近一条检查点的轮次改成「已提交 1 个工具组」：注入的助手消息就是那一组。
async fn round_of_checkpoint(pool: &sqlx::SqlitePool, run: &PausedRun, round: i64) -> Result<()> {
    insert(
        sqlx::query("UPDATE run_checkpoints SET tool_round = ? WHERE run_id = ?")
            .bind(round)
            .bind(&run.run_id)
            .execute(pool),
    )
    .await
}

/// 工具审计（查询口径：不含 `PENDING`）。
async fn audit(context: &TestContext, run_id: &str) -> Result<Vec<lib_bin_cli::sdk::ToolCallVO>> {
    context
        .client()
        .tool_audit_list(ToolAuditListParams {
            run_id: run_id.to_owned(),
        })
        .await
        .map_err(ClientError::from_call)
        .map_err(Into::into)
}

/// 把一段文本转成 JSON 字符串字面量（嵌进 `tool_calls` 的参数字段）。
fn esc(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

/// 执行注入语句：0 行即失败。
async fn insert(
    statement: impl Future<Output = StdResult<sqlx::sqlite::SqliteQueryResult, sqlx::Error>>,
) -> Result<()> {
    let affected = statement.await.context("执行注入语句失败")?.rows_affected();
    ensure!(affected > 0, "注入语句没有命中任何行");
    Ok(())
}
