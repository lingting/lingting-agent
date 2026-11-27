//! 轮次边界的检查点写入（P6A-005）：构造与追加的唯一入口。
//!
//! 顺序固定为「**先提交边界，再写检查点**」（D-P6-001）：检查点指向的进度只有在事务提交后
//! 才是真话，反过来会得到「进度已到但数据没到」的半批次状态。三个落点：
//!
//! - `MODEL_DONE`：`run_started` 之后一条（起点，`tool_round = 0`），以及每轮工具轨迹提交后一条；
//! - `TOOLS_DONE`：一轮工具批次的 assistant 调用与全部结果提交后一条；
//! - `AWAITING_CONFIRM`：`PENDING` 审计与 `RUNNING → WAITING` 提交后一条。
//!
//! 写失败一律返回固定文案（[`error::CHECKPOINT`]），由调用方把 Run 落 `FAILED`：可恢复性
//! 无声消失比失败更糟（D-P6-010）。载荷只放恢复所需的最小事实（[`ToolCallDigest`]），
//! 不放上下文全文、系统提示词与 Provider 原始响应（D-P6-003）。

use agent_core::session::{CheckpointPhase, Run, RunCheckpoint, ToolCallDigest};
use agent_tool::ToolCallRecord;
use lib_core::time;

use crate::error;
use crate::port::AgentRuntime;

/// 追加一条检查点：`seq_no` 由存储层分配（同一 Run 内从 1 连续），返回本次分配的序号。
///
/// `last_message_seq` 在这里统一取「会话已提交消息的上界」，调用点不再各自算进度。
pub(crate) async fn append(
    runtime: &AgentRuntime,
    run: &Run,
    phase: CheckpointPhase,
    tool_round: u32,
    tool_calls: Vec<ToolCallDigest>,
) -> Result<i64, String> {
    let last_message_seq = runtime
        .service
        .last_message_seq(&run.session_id)
        .await
        .map_err(|error| super::logged(error::STORAGE, error))?;
    let checkpoint = RunCheckpoint::new(phase, tool_round, last_message_seq, tool_calls, time::now_millis());
    runtime
        .service
        .append_checkpoint(&run.id, &checkpoint)
        .await
        .map_err(|error| super::logged(error::CHECKPOINT, error))
}

/// 一组调用的摘要（工具批次的载荷）。
pub(crate) fn digests(records: &[ToolCallRecord]) -> Vec<ToolCallDigest> {
    records.iter().map(digest).collect()
}

/// 单条调用的摘要：只有调用标识与落库状态。
pub(crate) fn digest(record: &ToolCallRecord) -> ToolCallDigest {
    ToolCallDigest {
        tool_call_id: record.tool_call_id.clone(),
        status: record.status,
    }
}
