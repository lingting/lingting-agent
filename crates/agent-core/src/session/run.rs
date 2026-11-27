//! Run 状态机：合法转换的唯一判定点。
//!
//! 转换表来自 D-P2-009（P2-004 定稿），数据库条件更新是该表的执行者，本模块只做「是否允许」的
//! 领域判定：
//!
//! | 起点 | 终点 |
//! |---|---|
//! | `QUEUED` | `RUNNING` / `CANCELLED` |
//! | `RUNNING` | `COMPLETED` / `FAILED` / `CANCELLED`；`PAUSED`（仅启动自愈，D-P6-004） |
//! | `PAUSED` | `RUNNING`（仅「占用 + 发消息」，D-P6-009）/ `CANCELLED` |
//! | 终态（`COMPLETED` / `FAILED` / `CANCELLED`） | 无 |
//!
//! 契约类型本身在 `dto` 模块，本模块只放状态机。

use crate::error::AgentError;
use crate::session::RunStatus;

impl RunStatus {
    /// 终态集合：任何路径都不得改写已是终态的 Run（D-P2-030 / D-P6-005）。
    pub const TERMINAL: [Self; 3] = [Self::Completed, Self::Cancelled, Self::Failed];
    /// 未结束集合：可执行、可取消、可被启动自愈处理（D-P6-004）。
    pub const ACTIVE: [Self; 4] = [Self::Queued, Self::Running, Self::Waiting, Self::Paused];

    /// 是否已抵终态：终态不可改写，也不可恢复。
    pub fn is_terminal(self) -> bool {
        Self::TERMINAL.contains(&self)
    }

    /// 是否仍未结束：四个非终态都是「未结束」，但只有 `QUEUED` 与 `PAUSED` 能被执行队列拾取，
    /// `WAITING` 等的是客户端回传确认。
    pub fn is_active(self) -> bool {
        Self::ACTIVE.contains(&self)
    }

    /// 是否可被取消：四个非终态都能落 `CANCELLED`，终态一律不可取消（D-P6-005）。
    pub fn can_cancel(self) -> bool {
        self.is_active()
    }

    /// `next` 是否是当前状态允许的下一步转换；终态一律返回 `false`。
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Running)
                | (Self::Queued, Self::Cancelled)
                | (Self::Running, Self::Completed)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Cancelled)
                | (Self::Running, Self::Paused)
                | (Self::Paused, Self::Running)
                | (Self::Paused, Self::Cancelled)
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
