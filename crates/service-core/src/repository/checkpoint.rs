//! 检查点表的读写：追加（插行 + 指针）、读回与终态清理。
//!
//! 这里是检查点唯一的读写入口：其它层不拼 SQL（`docs/workspace.md` 4.3），也不自己算序号。
//!
//! - **序号由存储层给出**：`MAX(seq_no) + 1`，同一 Run 内从 1 开始连续；`UNIQUE (run_id, seq_no)`
//!   让并发写入直接报错，而不是静默覆盖。
//! - **指针同事务**：插行与 `runs.last_checkpoint_seq` 在同一个事务里，插入失败时指针保持原值
//!   （D-P6-001）。
//! - **载荷编解码只在本模块**：写入时序列化，读回时按 `format_version` 分类——`> 当前版本` 是
//!   `Unsupported`，`0` / 缺失 / 结构缺字段 / 与列不一致是 `Corrupt`，两类都判「不可恢复」
//!   （D-P6-003）。
//! - **载荷必须是恢复所需的最小事实**：只有工具组的调用标识与落库状态，不放上下文全文、系统
//!   提示词、`AGENTS.md` 内容与 Provider 原始响应。

use agent_core::session::{CHECKPOINT_FORMAT_VERSION, CheckpointDecodeError, CheckpointPayload, RunCheckpoint};
use lib_db::DbError;
use sqlx::Row;
use std::fmt::{Display, Formatter};

use super::{Repository, decode_enum};

/// 检查点查询的公共投影。
const SELECT_CHECKPOINT: &str = "\
SELECT seq_no, phase, tool_round, last_message_seq, payload, format_version, created_at_ms \
FROM run_checkpoints";

/// 检查点读写的失败。
///
/// 两者对外都只有固定文案，但语义不同：`Storage` 是环境问题（磁盘、锁），`Decode` 是数据问题
/// （版本 / 结构）；判「不可恢复」的只有后者（D-P6-003）。`Display` 只用于日志与测试输出，
/// 对外文案由执行层按变体统一产出，不把载荷内容或环境路径带进响应。
#[derive(Debug)]
pub enum CheckpointError {
    /// 读 / 写库失败；细节只进日志。
    Storage(DbError),
    /// 解码失败：检查点不可解释。
    Decode(CheckpointDecodeError),
}

impl From<DbError> for CheckpointError {
    fn from(error: DbError) -> Self {
        Self::Storage(error)
    }
}

impl From<sqlx::Error> for CheckpointError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(DbError::from(error))
    }
}

impl From<CheckpointDecodeError> for CheckpointError {
    fn from(error: CheckpointDecodeError) -> Self {
        Self::Decode(error)
    }
}

impl Display for CheckpointError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "检查点读写失败：{error}"),
            Self::Decode(CheckpointDecodeError::Unsupported { found, supported }) => {
                write!(formatter, "检查点格式版本不被支持：{found} > {supported}")
            }
            Self::Decode(CheckpointDecodeError::Corrupt) => formatter.write_str("检查点载荷无法解释"),
        }
    }
}

impl std::error::Error for CheckpointError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Decode(_) => None,
        }
    }
}

impl Repository {
    /// 追加一条检查点并推进 `runs.last_checkpoint_seq`，返回本次分配的 `seq_no`。
    ///
    /// 序号取「该 Run 当前最大序号 + 1」，**在插入语句里现算**：事务的第一条语句就是写，
    /// 因此写锁由 `busy_timeout` 等待；先 `SELECT` 再 `INSERT` 会让事务以读快照起步，
    /// 期间别的连接一旦提交，升级成写锁会直接得到 `SQLITE_BUSY_SNAPSHOT`（不等超时），
    /// 并发 Run 的检查点就会被误判成写失败。冲突仍由 `UNIQUE (run_id, seq_no)` 报错，不重试。
    pub async fn append_checkpoint(&self, run_id: &str, checkpoint: &RunCheckpoint) -> Result<i64, CheckpointError> {
        let payload = encode_payload(checkpoint)?;
        // 字节数在 `bind` 之前取：载荷随后被移交给 SQL，指标要的是落库的那份载荷大小（P6A-018）。
        let payload_bytes = payload.len();
        let mut transaction = self.begin().await?;
        // 主键与序号同构（`<run_id>:<seq_no>`）：两者由同一个子查询给出，排障时直接可读。
        let seq_no: i64 = sqlx::query_scalar(
            "INSERT INTO run_checkpoints \
             (id, run_id, seq_no, phase, tool_round, last_message_seq, payload, format_version, created_at_ms) \
             VALUES (? || ':' || (SELECT COALESCE(MAX(seq_no), 0) + 1 FROM run_checkpoints WHERE run_id = ?), \
                     ?, \
                     (SELECT COALESCE(MAX(seq_no), 0) + 1 FROM run_checkpoints WHERE run_id = ?), \
                     ?, ?, ?, ?, ?, ?) \
             RETURNING seq_no",
        )
        .bind(run_id)
        .bind(run_id)
        .bind(run_id)
        .bind(run_id)
        .bind(checkpoint.phase.to_string())
        .bind(checkpoint.tool_round)
        .bind(checkpoint.last_message_seq)
        .bind(payload)
        .bind(checkpoint.format_version)
        .bind(checkpoint.created_at_ms)
        .fetch_one(transaction.connection())
        .await?;
        sqlx::query("UPDATE runs SET last_checkpoint_seq = ? WHERE id = ?")
            .bind(seq_no)
            .bind(run_id)
            .execute(transaction.connection())
            .await?;
        transaction.commit().await?;
        crate::observability::record_checkpoint_written(payload_bytes);
        Ok(seq_no)
    }

    /// 某 Run 的最近一条检查点（按 `seq_no` 倒序取一条）；没有检查点时为空。
    pub async fn latest_checkpoint(&self, run_id: &str) -> Result<Option<RunCheckpoint>, CheckpointError> {
        let sql = format!("{SELECT_CHECKPOINT} WHERE run_id = ? ORDER BY seq_no DESC LIMIT 1");
        let row = sqlx::query(&sql).bind(run_id).fetch_optional(self.pool()).await?;
        row.as_ref().map(checkpoint_from_row).transpose()
    }

    /// 某 Run 的全部检查点，按 `seq_no` 升序（序号缺口由调用方按「不连续即损坏」判定）。
    pub async fn checkpoints_of_run(&self, run_id: &str) -> Result<Vec<RunCheckpoint>, CheckpointError> {
        let sql = format!("{SELECT_CHECKPOINT} WHERE run_id = ? ORDER BY seq_no");
        let rows = sqlx::query(&sql).bind(run_id).fetch_all(self.pool()).await?;
        rows.iter().map(checkpoint_from_row).collect()
    }

    /// 只保留某 Run 最近 `keep_last` 条检查点，返回被删除的条数。
    ///
    /// 是否「该清理」由调用方判断（只在 Run 落终态时清理）；`keep_last` 大于已有条数时一条都不删。
    pub async fn prune_checkpoints(&self, run_id: &str, keep_last: usize) -> Result<usize, CheckpointError> {
        let keep_last = i64::try_from(keep_last).unwrap_or(i64::MAX);
        let affected = sqlx::query(
            "DELETE FROM run_checkpoints \
             WHERE run_id = ? \
               AND seq_no <= (SELECT COALESCE(MAX(seq_no), 0) FROM run_checkpoints WHERE run_id = ?) - ?",
        )
        .bind(run_id)
        .bind(run_id)
        .bind(keep_last)
        .execute(self.pool())
        .await?
        .rows_affected();
        let removed = usize::try_from(affected).unwrap_or(usize::MAX);
        crate::observability::record_checkpoints_pruned(removed);
        Ok(removed)
    }
}

/// 载荷 → 列值。
fn encode_payload(checkpoint: &RunCheckpoint) -> Result<String, DbError> {
    serde_json::to_string(&checkpoint.payload).map_err(|error| DbError::from(sqlx::Error::Encode(Box::new(error))))
}

/// 行 → 检查点；列里的 `format_version` 决定这条记录能不能解释。
fn checkpoint_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<RunCheckpoint, CheckpointError> {
    let format_version: i64 = row.try_get("format_version")?;
    let payload = decode_payload(format_version, &row.try_get::<String, _>("payload")?)?;
    Ok(RunCheckpoint {
        seq_no: row.try_get("seq_no")?,
        phase: decode_enum("phase", &row.try_get::<String, _>("phase")?)?,
        tool_round: row.try_get::<i64, _>("tool_round")?.max(0) as u32,
        last_message_seq: row.try_get("last_message_seq")?,
        // 解码通过即等于当前版本（见 [`decode_payload`]）。
        format_version: CHECKPOINT_FORMAT_VERSION,
        payload,
        created_at_ms: row.try_get("created_at_ms")?,
    })
}

/// 列值 → 载荷：严格按版本分类，不猜测、也不为未来版本留兜底。
fn decode_payload(format_version: i64, text: &str) -> Result<CheckpointPayload, CheckpointDecodeError> {
    if format_version <= 0 {
        return Err(CheckpointDecodeError::Corrupt);
    }
    if format_version > i64::from(CHECKPOINT_FORMAT_VERSION) {
        return Err(CheckpointDecodeError::Unsupported {
            found: format_version,
            supported: CHECKPOINT_FORMAT_VERSION,
        });
    }
    // 未知字段由 serde 默认忽略（向前扩展留余地），缺字段直接报错（不猜默认值）。
    let payload: CheckpointPayload = serde_json::from_str(text).map_err(|_| CheckpointDecodeError::Corrupt)?;
    if payload.format_version != CHECKPOINT_FORMAT_VERSION {
        return Err(CheckpointDecodeError::Corrupt);
    }
    Ok(payload)
}
