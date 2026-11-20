//! 会话表的读写：创建、按 ID 查询、列表（含消息数）与更新时间。

use agent_core::session::SessionSummary;
use lib_db::{DbError, Transaction};
use sqlx::Row;

use super::Repository;

/// 会话查询的公共投影：消息数用 `COUNT(*)` 子查询（D-P2-005），不落冗余计数列。
const SELECT_SESSION: &str = "\
SELECT s.id, s.title, s.model, s.created_at_ms, s.updated_at_ms,
       (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id) AS message_count
FROM sessions s";

impl Repository {
    /// 写入会话摘要。
    pub async fn insert_session(&self, session: &SessionSummary) -> Result<(), DbError> {
        sqlx::query("INSERT INTO sessions (id, title, model, created_at_ms, updated_at_ms) VALUES (?, ?, ?, ?, ?)")
            .bind(&session.id)
            .bind(&session.title)
            .bind(&session.model)
            .bind(session.created_at_ms)
            .bind(session.updated_at_ms)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// 按 ID 取会话摘要；不存在时为空。
    pub async fn session(&self, session_id: &str) -> Result<Option<SessionSummary>, DbError> {
        let sql = format!("{SELECT_SESSION} WHERE s.id = ?");
        let row = sqlx::query(&sql).bind(session_id).fetch_optional(self.pool()).await?;
        row.as_ref().map(session_from_row).transpose()
    }

    /// 会话是否存在：占用建立前的校验（不为不存在的会话开事件文件）。
    pub async fn has_session(&self, session_id: &str) -> Result<bool, DbError> {
        let found: Option<i64> = sqlx::query_scalar("SELECT 1 FROM sessions WHERE id = ?")
            .bind(session_id)
            .fetch_optional(self.pool())
            .await?;
        Ok(found.is_some())
    }

    /// 全部会话摘要，不排序（排序是业务规则，由 Service 决定）。
    pub async fn sessions(&self) -> Result<Vec<SessionSummary>, DbError> {
        let rows = sqlx::query(SELECT_SESSION).fetch_all(self.pool()).await?;
        rows.iter().map(session_from_row).collect()
    }

    /// 刷新会话的最后更新时间：只在已开启的事务里调用（`*_in` 约定）。
    pub(super) async fn touch_session_in(
        &self,
        transaction: &mut Transaction,
        session_id: &str,
        now_ms: i64,
    ) -> Result<(), DbError> {
        sqlx::query("UPDATE sessions SET updated_at_ms = ? WHERE id = ?")
            .bind(now_ms)
            .bind(session_id)
            .execute(transaction.connection())
            .await?;
        Ok(())
    }
}

/// 行 → 会话摘要。
fn session_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<SessionSummary, DbError> {
    Ok(SessionSummary {
        id: row.try_get("id")?,
        title: row.try_get("title")?,
        model: row.try_get("model")?,
        message_count: row.try_get("message_count")?,
        created_at_ms: row.try_get("created_at_ms")?,
        updated_at_ms: row.try_get("updated_at_ms")?,
    })
}
