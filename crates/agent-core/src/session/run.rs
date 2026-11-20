//! Run 状态机：合法转换的唯一判定点。
//!
//! 转换表来自 D-P2-009（P2-004 定稿），数据库条件更新是该表的执行者，本模块只做「是否允许」的
//! 领域判定：
//!
//! | 起点 | 终点 |
//! |---|---|
//! | `QUEUED` | `RUNNING` / `CANCELLED` |
//! | `RUNNING` | `COMPLETED` / `FAILED` / `CANCELLED` |
//! | 终态（`COMPLETED` / `FAILED` / `CANCELLED`） | 无 |
//!
//! 契约类型本身在 `dto` 模块，本模块只放状态机。

use crate::error::AgentError;
use crate::session::RunStatus;

impl RunStatus {
    /// `next` 是否是当前状态允许的下一步转换；终态一律返回 `false`。
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Running)
                | (Self::Queued, Self::Cancelled)
                | (Self::Running, Self::Completed)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Cancelled)
        )
    }

    /// 按状态机推进；非法转换返回 [`AgentError::InvalidTransition`]，当前状态不变。
    pub fn transition_to(self, next: Self) -> Result<Self, AgentError> {
        if self.can_transition_to(next) {
            return Ok(next);
        }
        Err(AgentError::InvalidTransition { from: self, to: next })
    }
}
