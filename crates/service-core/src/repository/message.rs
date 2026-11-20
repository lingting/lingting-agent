//! 消息表的读写：带 `seq` 分配的插入、按序读取与用户提示词查询。

use agent_core::session::{Message, MessageRole};
use lib_db::{DbError, Transaction};
use sqlx::Row;

use super::{Repository, decode_enum};

/// 消息查询的公共投影：所有读取按 `seq` 升序（D-P2-005）。
const SELECT_MESSAGE: &str = "\
SELECT id, session_id, run_id, role, content, model, input_tokens, output_tokens, created_at_ms
FROM messages";

impl Repository {
    /// 在已开启的事务里写入一条消息：`seq` 在写入语句内按 `MAX(seq)+1` 分配，
    /// 与写入同一条语句完成，因此并发写会被 `UNIQUE(session_id, seq)` 挡住（D-P2-005）。
    pub(super) async fn insert_message_in(
        &self,
        transaction: &mut Transaction,
        message: &Message,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO messages \
             (id, session_id, run_id, role, content, model, input_tokens, output_tokens, seq, created_at_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, \
                     COALESCE((SELECT MAX(seq) FROM messages WHERE session_id = ?), 0) + 1, ?)",
        )
        .bind(&message.id)
        .bind(&message.session_id)
        .bind(&message.run_id)
        .bind(message.role.to_string())
        .bind(&message.content)
        .bind(&message.model)
        .bind(message.input_tokens)
        .bind(message.output_tokens)
        .bind(&message.session_id)
        .bind(message.created_at_ms)
        .execute(transaction.connection())
        .await?;
        Ok(())
    }

    /// 某个会话的全部消息，按 `seq` 升序。
    pub async fn messages(&self, session_id: &str) -> Result<Vec<Message>, DbError> {
        let sql = format!("{SELECT_MESSAGE} WHERE session_id = ? ORDER BY seq");
        let rows = sqlx::query(&sql).bind(session_id).fetch_all(self.pool()).await?;
        rows.iter().map(message_from_row).collect()
    }

    /// 取某个 Run 对应的用户消息正文：执行 Run 时作为 prompt。
    pub async fn user_prompt(&self, session_id: &str, run_id: &str) -> Result<Option<String>, DbError> {
        let user = MessageRole::User.to_string();
        let sql = format!("{SELECT_MESSAGE} WHERE session_id = ? AND run_id = ? AND role = ? ORDER BY seq LIMIT 1");
        let row = sqlx::query(&sql)
            .bind(session_id)
            .bind(run_id)
            .bind(user)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|row| row.get::<String, _>("content")))
    }
}

/// 行 → 消息。
fn message_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Message, DbError> {
    Ok(Message {
        id: row.try_get("id")?,
        session_id: row.try_get("session_id")?,
        run_id: row.try_get("run_id")?,
        role: decode_enum("role", &row.try_get::<String, _>("role")?)?,
        content: row.try_get("content")?,
        model: row.try_get("model")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        created_at_ms: row.try_get("created_at_ms")?,
    })
}
