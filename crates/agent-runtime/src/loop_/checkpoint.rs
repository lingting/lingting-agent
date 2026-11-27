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

use agent_core::session::{
    CheckpointDecodeError, CheckpointPhase, LlmToolCallVO, ResumePoint, Run, RunCheckpoint, ToolCallDigest,
    ToolCallStatus,
};
use agent_route::LlmToolCall;
use agent_tool::ToolCallRecord;
use lib_core::time;
use service_core::session::{CheckpointError, ResumePlan, SessionService};

use crate::error;
use crate::port::AgentRuntime;

use super::trace;

/// 追加一条检查点：`seq_no` 由存储层分配（同一 Run 内从 1 连续），返回本次分配的序号。
///
/// `last_message_seq` 在这里统一取「会话已提交消息的上界」，调用点不再各自算进度。
/// `assistant_message_id` 只有起点检查点（`MODEL_DONE` + `tool_round = 0`）传值，其余边界为 `None`。
pub(crate) async fn append(
    runtime: &AgentRuntime,
    run: &Run,
    phase: CheckpointPhase,
    tool_round: u32,
    tool_calls: Vec<ToolCallDigest>,
    assistant_message_id: Option<String>,
) -> Result<i64, String> {
    let last_message_seq = runtime
        .service
        .last_message_seq(&run.session_id)
        .await
        .map_err(|error| super::logged(error::STORAGE, error))?;
    let checkpoint = RunCheckpoint::new(
        phase,
        tool_round,
        last_message_seq,
        tool_calls,
        assistant_message_id,
        time::now_millis(),
    );
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

/// 恢复前判定：读最近检查点与已提交轨迹，产出确定恢复点或不可恢复原因（D-P6-007）。
///
/// 三项判定结果互斥：可恢复（恢复点唯一）、不可恢复（原因是对外固定文案，调用方直接落终态）、
/// 没有任何检查点（升级前的旧 Run，保持中断态，既不恢复也不判死，`R-7`）。
///
/// 返回的 `Err` 只表示「判定所需的读库失败」：它不是数据损坏，不能用来把 Run 判死。
pub(crate) async fn plan(service: &SessionService, run: &Run) -> Result<ResumePlan, String> {
    let checkpoints = match service.checkpoints_of_run(&run.id).await {
        Ok(checkpoints) => checkpoints,
        // 解码失败是数据问题：不猜、不降级，直接判不可恢复（D-P6-003）。
        Err(CheckpointError::Decode(failure)) => return Ok(ResumePlan::Unrecoverable(decode_text(failure))),
        // 读库失败是环境问题：Err 表示「这次判定不成」，装配层保留中断态等下一次触发。
        Err(CheckpointError::Storage(failure)) => return Err(super::logged(error::STORAGE, failure)),
    };
    let Some(latest) = checkpoints.last() else {
        return Ok(ResumePlan::NoCheckpoint);
    };
    // ① 序号必须连续，且 `runs.last_checkpoint_seq` 指向最近一条：序号是「已提交到哪」的唯一定位。
    if gap(run, &checkpoints, latest) {
        return Ok(ResumePlan::Unrecoverable(error::RESUME_GAP.to_string()));
    }
    // ② 上界那条消息必须仍在：边界缺失说明进度指向的消息已经不在了。
    let bounded = service
        .message_seq_exists(&run.session_id, latest.last_message_seq)
        .await
        .map_err(|failure| super::logged(error::STORAGE, failure))?;
    if !bounded {
        return Ok(ResumePlan::Unrecoverable(error::RESUME_MESSAGES_LOST.to_string()));
    }
    // ③ 配对规则：只有最后一个工具组允许缺结果（沿用 D-P3-023），更早的组缺结果或孤儿结果即损坏。
    let recovered = match trace::recover(service, run).await {
        Ok(recovered) => recovered,
        Err(_) => return Ok(ResumePlan::Unrecoverable(error::RESUME_TRACE_BROKEN.to_string())),
    };
    let rounds = recovered.trace.rounds();
    // ⑤ 检查点阶段与轨迹形态一致：检查点记录的轮次必须等于已提交的工具组数。
    if latest.tool_round != rounds {
        return Ok(ResumePlan::Unrecoverable(error::RESUME_PHASE_MISMATCH.to_string()));
    }
    // ④ `PENDING` 开始标记只允许出现在最后检查组（或尚未提交的那一轮）：出现在更早的组，
    // 说明「有开始无结束」的调用比恢复点还旧，轨迹与检查点已经对不上。
    let pending = service
        .tool_calls_of_run(&run.id)
        .await
        .map_err(|failure| super::logged(error::STORAGE, failure))?
        .into_iter()
        .filter(|record| record.status == ToolCallStatus::Pending)
        .any(|record| record.tool_round != rounds && record.tool_round != rounds.saturating_add(1));
    if pending {
        return Ok(ResumePlan::Unrecoverable(error::RESUME_PENDING_LEAK.to_string()));
    }
    Ok(ResumePlan::Recoverable(ResumePoint {
        checkpoint_seq: latest.seq_no,
        phase: latest.phase,
        round: latest.tool_round,
        unpaired: recovered.unpaired.iter().map(call_of).collect(),
        // 起点检查点才带助手消息 ID；恢复点指向的是最近一条，因此要回到第 1 条去取
        // （第 1 条就是起点：序号从 1 连续，起点必然存在，`gap` 已保证连续）。
        assistant_message_id: checkpoints
            .first()
            .and_then(|start| start.payload.assistant_message_id.clone()),
    }))
}

/// ① 的判定：序号从 1 连续递增，且指针指向最近一条。
fn gap(run: &Run, checkpoints: &[RunCheckpoint], latest: &RunCheckpoint) -> bool {
    let continuous = checkpoints
        .iter()
        .enumerate()
        .all(|(index, checkpoint)| checkpoint.seq_no == index as i64 + 1);
    !continuous || run.last_checkpoint_seq != Some(latest.seq_no)
}

/// 解码失败 → 判定结论里的固定文案：未知版本与内容损坏各有独立文本，都不含载荷内容与环境路径。
fn decode_text(failure: CheckpointDecodeError) -> String {
    match failure {
        CheckpointDecodeError::Unsupported { .. } => error::CHECKPOINT_UNSUPPORTED.to_string(),
        CheckpointDecodeError::Corrupt => error::CHECKPOINT_CORRUPT.to_string(),
    }
}

/// 未配对调用 → 契约形态：恢复点只承载「发生过哪次调用」这个事实，不含参数以外的任何内容。
fn call_of(call: &LlmToolCall) -> LlmToolCallVO {
    LlmToolCallVO {
        id: call.id.clone(),
        name: call.name.clone(),
        arguments: call.arguments.clone(),
    }
}
