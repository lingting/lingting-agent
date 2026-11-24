//! 消息表的读写：带 `seq` 分配的插入、按序读取与用户提示词查询。
//!
//! 工具轨迹的三列（`tool_call_id` / `tool_name` / `tool_calls`）只对工具消息有意义（D-P3-003）：
//! 普通消息写入时三者为 `NULL`；`tool_calls` 存助手调用清单的 JSON 文本，读取时还原成结构体。

use agent_core::session::{LlmToolCallVO, Message, MessageRole};
use lib_db::{DbError, Transaction};
use sqlx::Row;

use super::{Repository, decode_enum};

/// 消息查询的公共投影：所有读取按 `seq` 升序（D-P2-005）。
const SELECT_MESSAGE: &str = "\
SELECT id, session_id, run_id, role, content, tool_call_id, tool_name, tool_calls, model, input_tokens, \
       output_tokens, created_at_ms \
FROM messages";

impl Repository {
    /// 在已开启的事务里写入一条消息：`seq` 在写入语句内按 `MAX(seq)+1` 分配，
    /// 与写入同一条语句完成，因此并发写会被 `UNIQUE(session_id, seq)` 挡住（D-P2-005）。
    pub(super) async fn insert_message_in(
        &self,
        transaction: &mut Transaction,
        message: &Message,
    ) -> Result<(), DbError> {
        let tool_calls = encode_tool_calls(message)?;
        sqlx::query(
            "INSERT INTO messages \
             (id, session_id, run_id, role, content, tool_call_id, tool_name, tool_calls, model, input_tokens, \
              output_tokens, seq, created_at_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
                     COALESCE((SELECT MAX(seq) FROM messages WHERE session_id = ?), 0) + 1, ?)",
        )
        .bind(&message.id)
        .bind(&message.session_id)
        .bind(&message.run_id)
        .bind(message.role.to_string())
        .bind(&message.content)
        .bind(&message.tool_call_id)
        .bind(&message.tool_name)
        .bind(tool_calls)
        .bind(&message.model)
        .bind(message.input_tokens)
        .bind(message.output_tokens)
        .bind(&message.session_id)
        .bind(message.created_at_ms)
        .execute(transaction.connection())
        .await?;
        Ok(())
    }

    /// 在已开启的事务里按给定顺序写入一组消息（工具轮次的边界落库，D-P3-024）。
    ///
    /// 逐条插入而不是一条多行 `INSERT`：`seq` 由子查询按会话递增，同一事务内后一条能看到
    /// 前一条写入的 `seq`，顺序因此与切片顺序一致。
    pub(super) async fn insert_messages_in(
        &self,
        transaction: &mut Transaction,
        messages: &[Message],
    ) -> Result<(), DbError> {
        for message in messages {
            self.insert_message_in(transaction, message).await?;
        }
        Ok(())
    }

    /// 一轮工具轨迹的原子写入（D-P3-024）：assistant `tool_calls` 与全部 TOOL 结果同一事务。
    ///
    /// 进入下一轮之前必须已经提交：恢复时靠「最后一个 tool_call 没有配对结果」认出挂起点。
    pub async fn with_tool_round(&self, messages: &[Message]) -> Result<(), DbError> {
        let mut transaction = self.begin().await?;
        self.insert_messages_in(&mut transaction, messages).await?;
        transaction.commit().await
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

    /// 某个 Run 之前的历史消息：按 `seq` 升序，**不含**本次输入（D-P2-013 的历史来源）。
    ///
    /// `seq` 由子查询定位本次输入，因此「同一会话里更早的消息」与「本次输入之前」是同一件事；
    /// 找不到对应的用户消息时子查询为空，结果同样为空（调用方按无历史处理）。
    pub async fn history_before_run(&self, session_id: &str, run_id: &str) -> Result<Vec<Message>, DbError> {
        let sql = format!(
            "{SELECT_MESSAGE} WHERE session_id = ? AND seq < \
             (SELECT seq FROM messages WHERE run_id = ? AND role = ? ORDER BY seq LIMIT 1) \
             ORDER BY seq"
        );
        let rows = sqlx::query(&sql)
            .bind(session_id)
            .bind(run_id)
            .bind(MessageRole::User.to_string())
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(message_from_row).collect()
    }
}

/// 调用清单 → 列值：`NULL` 表示没有工具调用。
///
/// 序列化失败按查询错误返回（`sqlx::Error::Encode`）：工具轨迹写不进去时宁可让本轮失败，
/// 也不要把 `NULL` 写进库，否则「助手请求调用」这一事实会凭空消失（D-P3-024）。
fn encode_tool_calls(message: &Message) -> Result<Option<String>, DbError> {
    let Some(calls) = message.tool_calls.as_ref() else {
        return Ok(None);
    };
    serde_json::to_string(calls)
        .map(Some)
        .map_err(|error| DbError::from(sqlx::Error::Encode(Box::new(error))))
}

/// 行 → 消息。
fn message_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Message, DbError> {
    Ok(Message {
        id: row.try_get("id")?,
        session_id: row.try_get("session_id")?,
        run_id: row.try_get("run_id")?,
        role: decode_enum("role", &row.try_get::<String, _>("role")?)?,
        content: row.try_get("content")?,
        tool_call_id: row.try_get("tool_call_id")?,
        tool_name: row.try_get("tool_name")?,
        tool_calls: decode_tool_calls(row.try_get("tool_calls")?)?,
        model: row.try_get("model")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        created_at_ms: row.try_get("created_at_ms")?,
    })
}

/// 列值 → 调用清单：`NULL` 表示没有工具调用。
fn decode_tool_calls(value: Option<String>) -> Result<Option<Vec<LlmToolCallVO>>, DbError> {
    let Some(text) = value else {
        return Ok(None);
    };
    serde_json::from_str(&text).map(Some).map_err(|error| {
        DbError::Query(sqlx::Error::ColumnDecode {
            index: "tool_calls".to_string(),
            source: Box::new(error),
        })
    })
}
