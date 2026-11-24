//! 工具执行错误：`Display` 只产出模型可读文本。

use std::fmt;

/// 一次工具调用的错误。
///
/// 变体与 README §4.4 的结局一一对应：参数非法、权限拒绝、需确认、超时、可重试失败、执行失败。
/// 文本会直接回灌给模型，因此不携带路径细节、URL 与调用栈（D-P3-007）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// 参数不合法：反序列化失败，文本已去掉 serde 的位置噪声。
    InvalidArguments(String),
    /// 权限拒绝：工具不会被调用。
    Denied(String),
    /// 需要客户端确认后才能执行：不是失败，由 Loop 判定是否挂起。
    ConfirmRequired,
    /// 执行超时：不重试。
    Timeout,
    /// 可重试的失败：只有工具自己声明可重试时才会被重试（D-P3-008）。
    Retryable(String),
    /// 执行失败。
    Failed(String),
}

impl fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments(reason) | Self::Denied(reason) | Self::Retryable(reason) => {
                formatter.write_str(reason)
            }
            Self::Failed(reason) => formatter.write_str(reason),
            Self::ConfirmRequired => formatter.write_str("该工具需要用户确认后才能执行。"),
            Self::Timeout => formatter.write_str("工具执行超时。"),
        }
    }
}

impl std::error::Error for ToolError {}
