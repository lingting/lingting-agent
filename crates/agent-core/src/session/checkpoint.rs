//! 检查点领域类型：一次 Run 在「已提交边界」上留下的进度标记（D-P6-001 / D-P6-003）。
//!
//! 检查点只落在三种边界上（`phase` 取值固定），`seq_no` 由存储层分配、同一 Run 内从 1 开始
//! 连续递增，是「已提交到哪」的唯一定位信息。检查点只进 SQLite（D-P6-002）：不进事件文件，
//! 也不参与客户端重放。
//!
//! 载荷只放**恢复所需的最小事实**（工具组的调用标识与落库状态）：不放上下文全文、系统提示词、
//! `AGENTS.md` 内容与 Provider 原始响应，恢复时 Context 重新组装（D-P6-003）。

use framework_proc_auto::auto_enum;
use serde::{Deserialize, Serialize};

use super::{LlmToolCallVO, ToolCallStatus};

/// 检查点格式版本：全仓只在这里定义一次，写入时同时进 `format_version` 列与载荷（D-P6-003）。
pub const CHECKPOINT_FORMAT_VERSION: u32 = 1;

/// Run 落终态时保留的检查点条数（缺省保留策略，见验收 `C-4` / `P-5`）。
pub const CHECKPOINT_KEEP_LAST: usize = 20;

/// 检查点落在哪一种已提交的边界上（D-P6-001）。
#[auto_enum]
pub enum CheckpointPhase {
    /// 模型轮次已落库：`run_started` 之后一条，以及每轮模型收尾事务提交后一条。
    ModelDone,
    /// 工具批次已落库：一轮工具轨迹与审计行提交后一条。
    ToolsDone,
    /// Run 停在工具确认点：挂起记录与 `RUNNING -> WAITING` 提交后一条。
    AwaitingConfirm,
}

/// 一次工具调用在检查点里的摘要：只有标识与落库状态，不含参数与结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallDigest {
    /// 模型给出的调用标识（与 TOOL 结果消息的 `tool_call_id` 配对）。
    pub tool_call_id: String,
    /// 落库状态：`PENDING` 表示「有开始、无结束」，恢复时一律不重放（D-P6-007）。
    pub status: ToolCallStatus,
}

/// 检查点载荷：恢复所需的最小事实（D-P6-001 / D-P6-003）。
///
/// 解码按严格规则：未知字段忽略（向前扩展留余地），缺字段报错（不猜默认值）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointPayload {
    /// 格式版本：与 `run_checkpoints.format_version` 列同时写入，两处必须一致。
    pub format_version: u32,
    /// 本批工具组的摘要；模型边界没有工具组时为空。
    pub tool_calls: Vec<ToolCallDigest>,
}

/// 一条检查点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCheckpoint {
    /// 单调序号：同一 Run 内从 1 开始连续递增，由存储层在追加时分配。
    pub seq_no: i64,
    /// 落在哪一种边界上。
    pub phase: CheckpointPhase,
    /// 轮次序号：起点检查点为 0，其后每轮加一。
    pub tool_round: u32,
    /// 已提交消息的 `seq` 上界：恢复时「从这条之后继续」。
    pub last_message_seq: i64,
    /// 载荷：只含恢复所需的最小事实。
    pub payload: CheckpointPayload,
    /// 格式版本：与载荷里的版本一并落库。
    pub format_version: u32,
    /// 记录产生时间（Unix 毫秒）。
    pub created_at_ms: i64,
}

impl RunCheckpoint {
    /// 构造一条待追加的检查点：格式版本只从 [`CHECKPOINT_FORMAT_VERSION`] 取。
    ///
    /// `seq_no` 由存储层在追加时分配（这里固定为 0），以追加返回值或读回值为准。
    pub fn new(
        phase: CheckpointPhase,
        tool_round: u32,
        last_message_seq: i64,
        tool_calls: Vec<ToolCallDigest>,
        created_at_ms: i64,
    ) -> Self {
        Self {
            seq_no: 0,
            phase,
            tool_round,
            last_message_seq,
            payload: CheckpointPayload {
                format_version: CHECKPOINT_FORMAT_VERSION,
                tool_calls,
            },
            format_version: CHECKPOINT_FORMAT_VERSION,
            created_at_ms,
        }
    }
}
/// 一次恢复的确定恢复点：一致性校验通过后由判定层产出（D-P6-007）。
///
/// 字段与最近一条检查点逐字段对应：恢复执行时轮次与轨迹必须与这里一致。`unpaired` 里的调用
/// **一次都不重跑**，只按固定通知文本回灌给模型（D-P6-006）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumePoint {
    /// 恢复依据的检查点序号：恢复后追加的检查点从这里接着递增，不重置。
    pub checkpoint_seq: i64,
    /// 该检查点落在哪一种已提交边界上。
    pub phase: CheckpointPhase,
    /// 已提交的工具轮次：恢复后的轮次从这里继续数。
    pub round: u32,
    /// 最后一个工具组里没有配对结果的调用：只用于回灌中断通知，绝不执行。
    pub unpaired: Vec<LlmToolCallVO>,
}

/// 检查点不可解释的两种情形：都判「不可恢复」，既不猜测也不降级成从头执行（D-P6-003）。
///
/// 变体不带库里的原始文本：固定文案在运行时侧统一产出，不把载荷内容或环境路径带进响应
/// （验收 `E-3` / `E-4`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointDecodeError {
    /// 版本高于当前代码支持的版本。
    Unsupported {
        /// 库里的版本号。
        found: i64,
        /// 当前代码支持的版本号。
        supported: u32,
    },
    /// 版本为 0 或缺失，或载荷缺字段、版本与列不一致。
    Corrupt,
}
