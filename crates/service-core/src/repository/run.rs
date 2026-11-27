//! Run 表的读写：创建、出队、终态条件更新、启动自愈与统计。
//!
//! 需要多语句原子性的操作以 `with_xxx` 命名，事务经 [`Repository::begin`] 取得（D-P2-034）；
//! 单语句操作直接走连接池。

use agent_core::session::{Message, Run, RunStatus};
use agent_tool::{REJECTED_TEXT, ToolCallStatus};
use lib_db::{DbError, Transaction};
use sqlx::Row;

use crate::session::{PendingToolCall, ResumeDecision};

use super::{Repository, decode_enum};

/// Run 查询的公共投影。
const SELECT_RUN: &str = "\
SELECT id, session_id, status, model, error, interrupted_reason, resume_count, last_checkpoint_seq, \
       input_tokens, output_tokens, created_at_ms, updated_at_ms, finished_at_ms \
FROM runs";

impl Repository {
    /// 未结束的 Run 状态取值：终态更新与统计都只针对这四个（`status` 列存 wire 值）。
    ///
    /// 取值来自 [`RunStatus::ACTIVE`]，不在这里另写列表：`WAITING`（等工具确认）与 `PAUSED`
    /// （非预期中断）都是未结束、都能被取消，但都不能被队列当成 `QUEUED` 取走。
    fn open_statuses() -> String {
        RunStatus::ACTIVE
            .iter()
            .map(|status| format!("'{status}'"))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// 按 ID 取 Run；不存在时为空。
    pub async fn run(&self, run_id: &str) -> Result<Option<Run>, DbError> {
        let sql = format!("{SELECT_RUN} WHERE id = ?");
        let row = sqlx::query(&sql).bind(run_id).fetch_optional(self.pool()).await?;
        row.as_ref().map(run_from_row).transpose()
    }

    /// 在已开启的事务里写入一条 Run（`QUEUED`），并刷新所属会话的更新时间。
    async fn insert_run_in(&self, transaction: &mut Transaction, run: &Run, now_ms: i64) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO runs \
             (id, session_id, status, model, error, input_tokens, output_tokens, created_at_ms, updated_at_ms, finished_at_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&run.id)
        .bind(&run.session_id)
        .bind(run.status.to_string())
        .bind(&run.model)
        .bind(&run.error)
        .bind(run.input_tokens)
        .bind(run.output_tokens)
        .bind(run.created_at_ms)
        .bind(run.updated_at_ms)
        .bind(run.finished_at_ms)
        .execute(transaction.connection())
        .await?;
        self.touch_session_in(transaction, &run.session_id, now_ms).await
    }

    /// 发送消息的原子写入：先建 `QUEUED` 的 Run，再写用户消息并刷新会话（同一个事务）。
    ///
    /// 顺序不可交换：`messages.run_id` 有外键指向 `runs(id)`，Run 必须先落库。
    pub async fn with_run_exchange(&self, run: &Run, message: &Message, now_ms: i64) -> Result<(), DbError> {
        let mut transaction = self.begin().await?;
        self.insert_run_in(&mut transaction, run, now_ms).await?;
        self.insert_message_in(&mut transaction, message).await?;
        transaction.commit().await
    }

    /// 取该会话最早的 `QUEUED` Run（只读，不推进状态）。
    ///
    /// 推进放到执行方拿到全局并发许可之后（P2-025）：`RUNNING` 才代表「正在执行」，
    /// 排队等许可的 Run 仍停在 `QUEUED`，因此「同时 RUNNING 的数量不超过上限」可观测。
    pub async fn queued_run(&self, session_id: &str) -> Result<Option<Run>, DbError> {
        let sql = format!("{SELECT_RUN} WHERE session_id = ? AND status = ? ORDER BY created_at_ms, id LIMIT 1");
        let row = sqlx::query(&sql)
            .bind(session_id)
            .bind(RunStatus::Queued.to_string())
            .fetch_optional(self.pool())
            .await?;
        row.as_ref().map(run_from_row).transpose()
    }

    /// 把 `QUEUED` 的 Run 推进到 `RUNNING`；返回是否真的推进（0 行表示已被取消或已终态）。
    pub async fn start_run(&self, run_id: &str, now_ms: i64) -> Result<bool, DbError> {
        let affected = sqlx::query("UPDATE runs SET status = ?, updated_at_ms = ? WHERE id = ? AND status = ?")
            .bind(RunStatus::Running.to_string())
            .bind(now_ms)
            .bind(run_id)
            .bind(RunStatus::Queued.to_string())
            .execute(self.pool())
            .await?
            .rows_affected();
        Ok(affected > 0)
    }
    /// 把 `PAUSED` 的 Run 推进到 `RUNNING`，并在同一语句里累加恢复次数（D-P6-006 / D-P6-007）。
    ///
    /// 条件更新以 `status = 'PAUSED'` 为前提：影响 0 行说明已被别的触发推进、或已被取消，
    /// 调用方据此直接放弃（不产事件、不改行）。`PAUSED` 是唯一会写 `interrupted_reason` 的状态
    /// （D-P6-004），因此推进时一并清空，恢复后的行与正常执行的行同形。
    pub async fn resume_paused_run(&self, run_id: &str, now_ms: i64) -> Result<bool, DbError> {
        let affected = sqlx::query(
            "UPDATE runs SET status = ?, resume_count = resume_count + 1, interrupted_reason = NULL, \
             updated_at_ms = ? WHERE id = ? AND status = ?",
        )
        .bind(RunStatus::Running.to_string())
        .bind(now_ms)
        .bind(run_id)
        .bind(RunStatus::Paused.to_string())
        .execute(self.pool())
        .await?
        .rows_affected();
        Ok(affected > 0)
    }

    /// 完成 Run：写入助手消息、刷新会话，并把 Run 落成 `COMPLETED`（同一个事务）。
    ///
    /// 条件更新先把状态推进到终态：影响 0 行说明 Run 已经不在 `RUNNING`（被取消或已终态），
    /// 此时事务整体回滚，**不写助手消息**，返回 `false`（P2 验收 S-6）。
    ///
    /// token 用量取 Provider 的真实 usage：缺一项就留空（`NULL`），不构造估算值（D-P2-014 第 4 条）。
    pub async fn with_run_completion(
        &self,
        run_id: &str,
        assistant: &Message,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        now_ms: i64,
    ) -> Result<bool, DbError> {
        let status = RunStatus::Completed.to_string();
        let running = RunStatus::Running.to_string();
        let mut transaction = self.begin().await?;
        let affected = sqlx::query(
            "UPDATE runs SET status = ?, input_tokens = ?, output_tokens = ?, updated_at_ms = ?, finished_at_ms = ? \
             WHERE id = ? AND status = ?",
        )
        .bind(&status)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(now_ms)
        .bind(now_ms)
        .bind(run_id)
        .bind(&running)
        .execute(transaction.connection())
        .await?
        .rows_affected();
        if affected == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        self.insert_message_in(&mut transaction, assistant).await?;
        self.touch_session_in(&mut transaction, &assistant.session_id, now_ms)
            .await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// 把未结束的 Run 落成 `FAILED`；返回是否真的推进（0 行表示已终态）。
    pub async fn fail_run(&self, run_id: &str, error: Option<&str>, now_ms: i64) -> Result<bool, DbError> {
        let status = RunStatus::Failed.to_string();
        let sql = format!(
            "UPDATE runs SET status = ?, error = ?, updated_at_ms = ?, finished_at_ms = ? \
             WHERE id = ? AND status IN ({})",
            Self::open_statuses()
        );
        let affected = sqlx::query(&sql)
            .bind(&status)
            .bind(error)
            .bind(now_ms)
            .bind(now_ms)
            .bind(run_id)
            .execute(self.pool())
            .await?
            .rows_affected();
        Ok(affected > 0)
    }

    /// 把未结束的 Run 落成 `CANCELLED`；返回是否真的推进（0 行表示已终态）。
    ///
    /// 清空待确认列与收尾 `PENDING` 审计在同一事务里（P3-027 第 5 条）：不允许出现
    /// 「挂起列已清、审计还挂着 PENDING」的中间态。
    pub async fn cancel_run(&self, run_id: &str, error: Option<&str>, now_ms: i64) -> Result<bool, DbError> {
        let status = RunStatus::Cancelled.to_string();
        let sql = format!(
            "UPDATE runs SET status = ?, error = ?, updated_at_ms = ?, finished_at_ms = ?, \
             pending_tool_call_id = NULL, pending_tool_round = NULL \
             WHERE id = ? AND status IN ({})",
            Self::open_statuses()
        );
        let mut transaction = self.begin().await?;
        let affected = sqlx::query(&sql)
            .bind(&status)
            .bind(error)
            .bind(now_ms)
            .bind(now_ms)
            .bind(run_id)
            .execute(transaction.connection())
            .await?
            .rows_affected();
        if affected > 0 {
            self.finish_pending_tool_calls_in(&mut transaction, run_id).await?;
        }
        transaction.commit().await?;
        Ok(affected > 0)
    }

    /// 取消某个会话的全部未结束 Run，返回被取消的数量（释放占用时使用）。
    ///
    /// 与 [`Repository::cancel_run`] 同一口径：待确认列与 `PENDING` 审计一起收尾。
    pub async fn cancel_session_runs(
        &self,
        session_id: &str,
        error: Option<&str>,
        now_ms: i64,
    ) -> Result<usize, DbError> {
        let status = RunStatus::Cancelled.to_string();
        let sql = format!(
            "UPDATE runs SET status = ?, error = ?, updated_at_ms = ?, finished_at_ms = ?, \
             pending_tool_call_id = NULL, pending_tool_round = NULL \
             WHERE session_id = ? AND status IN ({})",
            Self::open_statuses()
        );
        let mut transaction = self.begin().await?;
        let affected = sqlx::query(&sql)
            .bind(&status)
            .bind(error)
            .bind(now_ms)
            .bind(now_ms)
            .bind(session_id)
            .execute(transaction.connection())
            .await?
            .rows_affected();
        if affected > 0 {
            self.finish_pending_tool_calls_of_session_in(&mut transaction, session_id)
                .await?;
        }
        transaction.commit().await?;
        Ok(usize::try_from(affected).unwrap_or(usize::MAX))
    }

    /// 统计执行中的 Run 数量（只数 `RUNNING`），供探活接口报告运行状态（P2-025 第 3 条）。
    ///
    /// 排队中的 `QUEUED` 不计入：它没有占用模型连接，把它算进「运行中」会让并发上限的观测失真。
    /// 可恢复中断态 `PAUSED` 同样不计入（D-P6-011）：进程里根本没有正在执行的 Run。
    pub async fn running_run_count(&self) -> Result<i64, DbError> {
        sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE status = ?")
            .bind(RunStatus::Running.to_string())
            .fetch_one(self.pool())
            .await
            .map_err(DbError::Query)
    }

    /// 挂起：把 `RUNNING` 的 Run 推进到 `WAITING`，并记下等待确认的调用（D-P3-027）。
    ///
    /// 条件更新以 `status = 'RUNNING'` 为前提：影响 0 行说明 Run 已被取消或已终态，
    /// 调用方据此放弃挂起（不再发确认事件）。
    pub async fn suspend_run(
        &self,
        run_id: &str,
        tool_call_id: &str,
        tool_round: u32,
        now_ms: i64,
    ) -> Result<bool, DbError> {
        let affected = sqlx::query(
            "UPDATE runs SET status = ?, pending_tool_call_id = ?, pending_tool_round = ?, updated_at_ms = ? \
             WHERE id = ? AND status = ?",
        )
        .bind(RunStatus::Waiting.to_string())
        .bind(tool_call_id)
        .bind(tool_round)
        .bind(now_ms)
        .bind(run_id)
        .bind(RunStatus::Running.to_string())
        .execute(self.pool())
        .await?
        .rows_affected();
        Ok(affected > 0)
    }

    /// 该 Run 正在等待确认的调用；不处于挂起态时为空。
    pub async fn pending_tool_call(&self, run_id: &str) -> Result<Option<PendingToolCall>, DbError> {
        let row = sqlx::query("SELECT pending_tool_call_id, pending_tool_round FROM runs WHERE id = ?")
            .bind(run_id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.and_then(|row| {
            let tool_call_id: Option<String> = row.get("pending_tool_call_id");
            let tool_call_id = tool_call_id?;
            Some(PendingToolCall {
                run_id: run_id.to_string(),
                tool_call_id,
                tool_round: row.get::<Option<i64>, _>("pending_tool_round")?.max(0) as u32,
            })
        }))
    }

    /// 某个会话里全部挂起中的 Run，按最后更新时间升序（重连补发与取消清理用）。
    pub async fn pending_tool_calls_of_session(&self, session_id: &str) -> Result<Vec<PendingToolCall>, DbError> {
        let rows = sqlx::query(
            "SELECT id, pending_tool_call_id, pending_tool_round FROM runs \
             WHERE session_id = ? AND pending_tool_call_id IS NOT NULL ORDER BY updated_at_ms, id",
        )
        .bind(session_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(|row| PendingToolCall {
                run_id: row.get("id"),
                tool_call_id: row.get("pending_tool_call_id"),
                tool_round: row
                    .get::<Option<i64>, _>("pending_tool_round")
                    .unwrap_or_default()
                    .max(0) as u32,
            })
            .collect())
    }
    /// 清空待确认标识（取消链路用：挂起的 Run 只能由确认 / 取消 / 断连 / 关闭收尾）。
    pub async fn clear_pending_tool_call(&self, run_id: &str, now_ms: i64) -> Result<bool, DbError> {
        let affected = sqlx::query(
            "UPDATE runs SET pending_tool_call_id = NULL, pending_tool_round = NULL, updated_at_ms = ? \
             WHERE id = ? AND pending_tool_call_id IS NOT NULL",
        )
        .bind(now_ms)
        .bind(run_id)
        .execute(self.pool())
        .await?
        .rows_affected();
        Ok(affected > 0)
    }

    /// 唤醒：待确认标识与请求一致时把 Run 退回 `QUEUED` 并清空待确认列；
    /// 驳回时同一事务把审计记录置 `DENIED`（D-P3-027）。
    ///
    /// 返回是否命中：并发两次唤醒只有一次命中（第二次匹配不到待确认标识）。
    pub async fn with_resume_run(
        &self,
        run_id: &str,
        tool_call_id: &str,
        decision: ResumeDecision,
        now_ms: i64,
    ) -> Result<bool, DbError> {
        let mut transaction = self.begin().await?;
        let affected = sqlx::query(
            "UPDATE runs SET status = ?, pending_tool_call_id = NULL, pending_tool_round = NULL, updated_at_ms = ? \
             WHERE id = ? AND pending_tool_call_id = ? AND status = ?",
        )
        .bind(RunStatus::Queued.to_string())
        .bind(now_ms)
        .bind(run_id)
        .bind(tool_call_id)
        .bind(RunStatus::Waiting.to_string())
        .execute(transaction.connection())
        .await?
        .rows_affected();
        if affected == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        if decision == ResumeDecision::Reject {
            self.set_tool_call_status_in(
                &mut transaction,
                run_id,
                tool_call_id,
                ToolCallStatus::Denied,
                Some(REJECTED_TEXT),
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(true)
    }

    /// 启动自愈：上一次进程残留的执行中 Run 落 `PAUSED`（可恢复中间态，D-P6-004）；
    /// 排队中与等确认的 Run 落 `CANCELLED`（D-P2-030）。
    ///
    /// `PAUSED` 只写中断原因，**不写** `finished_at_ms`：它不是终态，也没有「结局」。
    /// `RUNNING` 期间已写入的开始标记（`tool_calls` 的 `PENDING` 行）保持原样——
    /// 恢复入口靠它区分「从未开始」与「可能已执行」（D-P6-007）。
    /// 等确认（`WAITING`）也要清理：确认的发起方（客户端连接）随进程一起消失，留着挂起态只会永久占位。
    /// 三个分支都在同一个事务里完成，避免半自愈状态；返回被处理的 Run 数量。
    pub async fn with_orphan_healing(
        &self,
        interrupted_reason: &str,
        queued_error: &str,
        now_ms: i64,
    ) -> Result<usize, DbError> {
        let paused = RunStatus::Paused.to_string();
        let cancelled = RunStatus::Cancelled.to_string();
        let running = RunStatus::Running.to_string();
        let mut transaction = self.begin().await?;
        let healed_running =
            sqlx::query("UPDATE runs SET status = ?, interrupted_reason = ?, updated_at_ms = ? WHERE status = ?")
                .bind(&paused)
                .bind(interrupted_reason)
                .bind(now_ms)
                .bind(&running)
                .execute(transaction.connection())
                .await?
                .rows_affected();
        let healed_queued = sqlx::query(
            "UPDATE runs SET status = ?, error = ?, updated_at_ms = ?, finished_at_ms = ?, \
             pending_tool_call_id = NULL, pending_tool_round = NULL \
             WHERE status IN (?, ?)",
        )
        .bind(&cancelled)
        .bind(queued_error)
        .bind(now_ms)
        .bind(now_ms)
        .bind(RunStatus::Queued.to_string())
        .bind(RunStatus::Waiting.to_string())
        .execute(transaction.connection())
        .await?
        .rows_affected();
        transaction.commit().await?;
        Ok(usize::try_from(healed_running + healed_queued).unwrap_or(usize::MAX))
    }
}

/// 行 → Run。
fn run_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Run, DbError> {
    Ok(Run {
        id: row.try_get("id")?,
        session_id: row.try_get("session_id")?,
        status: decode_enum("status", &row.try_get::<String, _>("status")?)?,
        model: row.try_get("model")?,
        error: row.try_get("error")?,
        interrupted_reason: row.try_get("interrupted_reason")?,
        resume_count: row.try_get("resume_count")?,
        last_checkpoint_seq: row.try_get("last_checkpoint_seq")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        created_at_ms: row.try_get("created_at_ms")?,
        updated_at_ms: row.try_get("updated_at_ms")?,
        finished_at_ms: row.try_get("finished_at_ms")?,
    })
}
