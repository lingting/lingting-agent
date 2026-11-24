//! Context 组装错误。
//!
//! 三个变体对应三类「组装阶段就没法继续」的情况：本次输入自身超预算、系统提示词读不出来、
//! 工具轨迹配对损坏。
//! `Display` 只输出可直接展示的结论，不含正文、估算明细以外的内部细节；原始 IO 错误经
//! [`std::error::Error::source`] 交给日志。

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;

/// Context 组装失败的原因。
#[derive(Debug)]
pub enum ContextError {
    /// 本次用户输入单条即超出预算：Run 落 `FAILED`，不发起上游请求（D-P2-013 第 4 条）。
    PromptTooLarge {
        /// 本次输入的估算 token 数。
        estimated_tokens: usize,
        /// 本次可用的 token 预算。
        available_tokens: usize,
    },
    /// 系统提示词文件存在但读取失败（权限、编码等）。
    SystemPrompt {
        /// 读取失败的位置：提示词文件或其所在目录。
        path: PathBuf,
        /// 原始 IO 错误，不对外。
        source: std::io::Error,
    },
    /// 工具轨迹配对损坏：工具结果消息的 `tool_call_id` 找不到对应的助手调用（D-P3-025）。
    ///
    /// 只可能来自被写坏的历史，不属于模型或用户的用法问题；调用方按 `FAILED` 收尾。
    ToolPairing {
        /// 出问题的调用标识；消息本身没有标识时为 `None`。
        tool_call_id: Option<String>,
    },
}

impl Display for ContextError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::PromptTooLarge { .. } => formatter.write_str("上下文超出模型预算"),
            Self::SystemPrompt { path, .. } => {
                write!(formatter, "读取系统提示词失败：{}", path.display())
            }
            Self::ToolPairing { .. } => formatter.write_str("工具轨迹配对损坏"),
        }
    }
}

impl Error for ContextError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PromptTooLarge { .. } | Self::ToolPairing { .. } => None,
            Self::SystemPrompt { source, .. } => Some(source),
        }
    }
}
