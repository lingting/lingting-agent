//! 工具确认的挂起与唤醒（D-P3-023）：Run 退回 `QUEUED`、让出并发许可，等客户端决定后重新排队。
//!
//! 两件事只在这里定义：挂起态的可读形状（[`PendingToolCall`]）与客户端的决定
//! （[`ResumeDecision`]）。状态迁移本身是条件更新，入口在 [`crate::session::SessionService`]：
//! 挂起以 `status = 'RUNNING'` 为前提，唤醒以待确认标识一致为前提，因此并发重复请求只有一次命中。

/// 挂起中的一次待确认调用：`runs.pending_tool_call_id` 非空即存在。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingToolCall {
    /// 挂起的 Run。
    pub run_id: String,
    /// 等待确认的调用标识（与 `tool_calls.tool_call_id`、确认事件同值）。
    pub tool_call_id: String,
    /// 该调用属于第几轮工具调用（从 1 开始）。
    pub tool_round: u32,
}

/// 客户端对挂起调用的决定。
///
/// 只有两个取值，且没有「总是允许此工具」的记忆：一次确认只对一次调用有效（D-P3-023）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeDecision {
    /// 批准：恢复后执行这次调用。
    Approve,
    /// 驳回：恢复后不再执行，把固定文案回灌给模型。
    Reject,
}
