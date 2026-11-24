//! 工具执行错误：`Display` 只产出模型可读的固定文案。
//!
//! 五种失败在模型侧各有一句固定文案（P3-016 的断言对象），产点只在本文件的 `Display`：
//!
//! - 参数非法：`参数不合法：<字段与原因>`
//! - 权限拒绝：`工具未授权：<name>`
//! - 执行超时：`工具执行超时（> <ms> 毫秒）`
//! - 执行失败：`工具执行失败：<脱敏原因>`
//! - 需确认：`该工具需要用户确认后才能执行`
//!
//! 文本会直接回灌给模型，因此不携带完整本地路径、URL 与调用栈（D-P3-007）。

use std::fmt;

/// 一次工具调用的错误。
///
/// 变体与 README §4.4 的结局一一对应：参数非法、权限拒绝、需确认、超时、可重试失败、执行失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// 参数不合法：反序列化失败，文本只保留字段名与原因。
    InvalidArguments(String),
    /// 权限拒绝：携带工具名，工具不会被调用。
    Denied(String),
    /// 需要客户端确认后才能执行：不是失败，由 Loop 判定是否挂起。
    ConfirmRequired,
    /// 执行超时：不重试，`limit_ms` 是本次调用的超时上限。
    Timeout {
        /// 超时上限（毫秒）。
        limit_ms: u64,
    },
    /// 可重试的失败：只有工具自己声明可重试时才会被重试（D-P3-008）。
    Retryable(String),
    /// 执行失败：文本只说「失败了什么」，不含路径与密钥。
    Failed(String),
}

impl fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments(reason) => write!(formatter, "参数不合法：{reason}"),
            Self::Denied(tool) => write!(formatter, "工具未授权：{tool}"),
            Self::ConfirmRequired => formatter.write_str("该工具需要用户确认后才能执行"),
            Self::Timeout { limit_ms } => write!(formatter, "工具执行超时（> {limit_ms} 毫秒）"),
            Self::Retryable(reason) | Self::Failed(reason) => write!(formatter, "工具执行失败：{reason}"),
        }
    }
}

impl std::error::Error for ToolError {}
