//! MCP server 的连接状态机（README §4.3）。
//!
//! 状态只有一个写入者——[`crate::McpClient`]：迁移只发生在 connect（`Disabled` /
//! `Failed` → `Connecting`）、握手成功（`Connecting` → `Ready`）、握手失败或读循环结束
//! （→ `Failed`）、关闭（→ `Stopped`）。对外只读，不提供 setter。

/// 一个 server 的连接状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpState {
    /// 配置存在但未启用（管理接口仍能看到它）。
    Disabled,
    /// 正在建 transport / 握手。
    Connecting,
    /// 已握手成功：可以列工具、可以调用。
    Ready,
    /// 连接不可用：原因已经是可读文本（不含 URL、请求头、命令行）。
    Failed {
        /// 可读原因，与 `lastError` 同源。
        reason: String,
    },
    /// 服务已关闭：拒绝新动作与新调用。
    Stopped,
}

impl McpState {
    /// 是否处于可用状态（只有 `Ready` 能发请求）。
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// 接口与日志使用的状态标签（与管理接口的枚举取值一一对应）。
    pub fn label(&self) -> &'static str {
        match self {
            Self::Disabled => "DISABLED",
            Self::Connecting => "CONNECTING",
            Self::Ready => "READY",
            Self::Failed { .. } => "FAILED",
            Self::Stopped => "STOPPED",
        }
    }

    /// 状态自带的原因（只有 `Failed` 有）。
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Failed { reason } => Some(reason),
            _ => None,
        }
    }
}
