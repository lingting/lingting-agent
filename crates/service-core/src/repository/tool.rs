//! 工具审计表的读写：挂起（`PENDING`）插入、收尾（终态）与按 Run 查询。
//!
//! 一次调用的两个写入点共用 `ON CONFLICT (run_id, tool_call_id)`：
//!
//! - 插入用 `DO NOTHING`：挂起时的 `PENDING` 记录是事实源，重复插入不覆盖它；
//! - 收尾用 `DO UPDATE`：确认批准 / 驳回、执行结束都落在同一条记录上（D-P3-023）。
//!
//! `created_at_ms` 与 `tool_round` 属于「调用发生时的既定事实」，收尾时不重写。

use agent_tool::{ToolCallRecord, ToolCallStatus};
use lib_db::{DbError, Transaction};
use sqlx::Row;

use super::{Repository, decode_enum};

/// 审计查询的公共投影。
const SELECT_TOOL_CALL: &str = "\
SELECT tool_call_id, tool_name, file_access, status, arguments, result, duration_ms, tool_round, created_at_ms \
FROM tool_calls";

impl Repository {
    /// 插入一条审计记录（`PENDING` 或直接是终态）；已存在时保持不变。
    pub async fn insert_tool_call(
        &self,
        id: &str,
        run_id: &str,
        session_id: &str,
        record: &ToolCallRecord,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO tool_calls \
             (id, run_id, session_id, tool_call_id, tool_name, file_access, status, arguments, result, duration_ms, \
              tool_round, created_at_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (run_id, tool_call_id) DO NOTHING",
        )
        .bind(id)
        .bind(run_id)
        .bind(session_id)
        .bind(&record.tool_call_id)
        .bind(&record.tool_name)
        .bind(record.file_access.to_string())
        .bind(record.status.to_string())
        .bind(&record.arguments)
        .bind(&record.result)
        .bind(millis(record.duration_ms))
        .bind(record.tool_round)
        .bind(record.created_at_ms)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// 收尾一条审计记录：状态、结果、耗时与参数按最新值覆盖。
    ///
    /// 用 upsert 而不是纯 `UPDATE`：挂起记录插入失败时，收尾仍能留下完整的一条（宁可多一条，
    /// 也不要「执行过但没有审计」）。
    pub async fn finish_tool_call(
        &self,
        id: &str,
        run_id: &str,
        session_id: &str,
        record: &ToolCallRecord,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO tool_calls \
             (id, run_id, session_id, tool_call_id, tool_name, file_access, status, arguments, result, duration_ms, \
              tool_round, created_at_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (run_id, tool_call_id) DO UPDATE SET \
                 status = excluded.status, arguments = excluded.arguments, result = excluded.result, \
                 duration_ms = excluded.duration_ms, file_access = excluded.file_access",
        )
        .bind(id)
        .bind(run_id)
        .bind(session_id)
        .bind(&record.tool_call_id)
        .bind(&record.tool_name)
        .bind(record.file_access.to_string())
        .bind(record.status.to_string())
        .bind(&record.arguments)
        .bind(&record.result)
        .bind(millis(record.duration_ms))
        .bind(record.tool_round)
        .bind(record.created_at_ms)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// 在已开启的事务里改状态与结果（确认驳回等只要换状态的场景）。
    ///
    /// 与 [`Repository::finish_tool_call`] 的分工：这里只改状态列，不动参数与耗时。
    pub(super) async fn set_tool_call_status_in(
        &self,
        transaction: &mut Transaction,
        run_id: &str,
        tool_call_id: &str,
        status: ToolCallStatus,
        result: Option<&str>,
    ) -> Result<bool, DbError> {
        let affected =
            sqlx::query("UPDATE tool_calls SET status = ?, result = ? WHERE run_id = ? AND tool_call_id = ?")
                .bind(status.to_string())
                .bind(result)
                .bind(run_id)
                .bind(tool_call_id)
                .execute(transaction.connection())
                .await?
                .rows_affected();
        Ok(affected > 0)
    }

    /// 某个 Run 的全部审计记录：按产生时间、ID 升序（同一毫秒内的顺序也是稳定的）。
    pub async fn tool_calls(&self, run_id: &str) -> Result<Vec<ToolCallRecord>, DbError> {
        let sql = format!("{SELECT_TOOL_CALL} WHERE run_id = ? ORDER BY created_at_ms, id");
        let rows = sqlx::query(&sql).bind(run_id).fetch_all(self.pool()).await?;
        rows.iter().map(tool_call_from_row).collect()
    }
}

/// 行 → 审计记录。
fn tool_call_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ToolCallRecord, DbError> {
    Ok(ToolCallRecord {
        tool_call_id: row.try_get("tool_call_id")?,
        tool_name: row.try_get("tool_name")?,
        file_access: decode_enum("file_access", &row.try_get::<String, _>("file_access")?)?,
        status: decode_enum("status", &row.try_get::<String, _>("status")?)?,
        arguments: row.try_get("arguments")?,
        result: row.try_get("result")?,
        duration_ms: u64::try_from(row.try_get::<i64, _>("duration_ms")?).unwrap_or_default(),
        tool_round: row.try_get("tool_round")?,
        created_at_ms: row.try_get("created_at_ms")?,
    })
}

/// 耗时列是 `INTEGER`（`i64`）：`u64` 转 `i64` 饱和，真实耗时不可能到 2^63 毫秒。
fn millis(duration_ms: u64) -> i64 {
    i64::try_from(duration_ms).unwrap_or(i64::MAX)
}
