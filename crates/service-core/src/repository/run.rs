//! Run 表的读写：创建、出队、终态条件更新、启动自愈与统计。
//!
//! 需要多语句原子性的操作以 `with_xxx` 命名，事务经 [`Repository::begin`] 取得（D-P2-034）；
//! 单语句操作直接走连接池。

use agent_core::session::{Message, Run, RunStatus};
use lib_db::{DbError, Transaction};
use sqlx::Row;

use super::{Repository, decode_enum};

/// Run 查询的公共投影。
const SELECT_RUN: &str = "\
SELECT id, session_id, status, model, error, input_tokens, output_tokens, \
       created_at_ms, updated_at_ms, finished_at_ms \
FROM runs";

impl Repository {
    /// 未结束的 Run 状态取值：终态更新与统计都只针对这两者（`status` 列存 wire 值）。
    fn open_statuses() -> String {
        format!("'{}','{}'", RunStatus::Queued, RunStatus::Running)
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
    pub async fn cancel_run(&self, run_id: &str, error: Option<&str>, now_ms: i64) -> Result<bool, DbError> {
        let status = RunStatus::Cancelled.to_string();
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

    /// 取消某个会话的全部未结束 Run，返回被取消的数量（释放占用时使用）。
    pub async fn cancel_session_runs(
        &self,
        session_id: &str,
        error: Option<&str>,
        now_ms: i64,
    ) -> Result<usize, DbError> {
        let status = RunStatus::Cancelled.to_string();
        let sql = format!(
            "UPDATE runs SET status = ?, error = ?, updated_at_ms = ?, finished_at_ms = ? \
             WHERE session_id = ? AND status IN ({})",
            Self::open_statuses()
        );
        let affected = sqlx::query(&sql)
            .bind(&status)
            .bind(error)
            .bind(now_ms)
            .bind(now_ms)
            .bind(session_id)
            .execute(self.pool())
            .await?
            .rows_affected();
        Ok(usize::try_from(affected).unwrap_or(usize::MAX))
    }

    /// 统计执行中的 Run 数量（只数 `RUNNING`），供探活接口报告运行状态（P2-025 第 3 条）。
    ///
    /// 排队中的 `QUEUED` 不计入：它没有占用模型连接，把它算进「运行中」会让并发上限的观测失真。
    pub async fn running_run_count(&self) -> Result<i64, DbError> {
        sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE status = ?")
            .bind(RunStatus::Running.to_string())
            .fetch_one(self.pool())
            .await
            .map_err(DbError::Query)
    }

    /// 启动自愈：上一次进程残留的执行中 Run 落 `FAILED`，排队中的 Run 落 `CANCELLED`（D-P2-030）。
    ///
    /// 两者在同一个事务里完成，避免半自愈状态；返回被处理的 Run 数量。
    pub async fn with_orphan_healing(
        &self,
        running_error: &str,
        queued_error: &str,
        now_ms: i64,
    ) -> Result<usize, DbError> {
        let failed = RunStatus::Failed.to_string();
        let cancelled = RunStatus::Cancelled.to_string();
        let running = RunStatus::Running.to_string();
        let queued = RunStatus::Queued.to_string();
        let mut transaction = self.begin().await?;
        let healed_running = sqlx::query(
            "UPDATE runs SET status = ?, error = ?, updated_at_ms = ?, finished_at_ms = ? WHERE status = ?",
        )
        .bind(&failed)
        .bind(running_error)
        .bind(now_ms)
        .bind(now_ms)
        .bind(&running)
        .execute(transaction.connection())
        .await?
        .rows_affected();
        let healed_queued = sqlx::query(
            "UPDATE runs SET status = ?, error = ?, updated_at_ms = ?, finished_at_ms = ? WHERE status = ?",
        )
        .bind(&cancelled)
        .bind(queued_error)
        .bind(now_ms)
        .bind(now_ms)
        .bind(&queued)
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
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        created_at_ms: row.try_get("created_at_ms")?,
        updated_at_ms: row.try_get("updated_at_ms")?,
        finished_at_ms: row.try_get("finished_at_ms")?,
    })
}
