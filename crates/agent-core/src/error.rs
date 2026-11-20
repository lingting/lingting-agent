//! Agent 领域错误。
//!
//! 只覆盖领域层能判定的错误：状态机拒绝的转换、目标不存在、入参非法与内部错误。
//! `Display` 供对外文案使用，只写可读结论，不携带内部细节（不含 SQL、URL、栈信息）；内部原因
//! 只出现在 `Debug` 里，由调用方决定是否记日志（D-P2-017 的脱敏规则同样适用于此）。

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::session::RunStatus;

/// Agent 领域错误。
#[derive(Debug)]
pub enum AgentError {
    /// 状态机不允许的转换：当前状态已是终态，或跳过了必经的中间状态。
    InvalidTransition {
        /// 转换前的状态。
        from: RunStatus,
        /// 请求的目标状态。
        to: RunStatus,
    },
    /// 目标不存在：`what` 是领域对象的中文名，`id` 是请求里给出的标识。
    NotFound {
        /// 领域对象名称，例如「会话」。
        what: &'static str,
        /// 请求里给出的标识。
        id: String,
    },
    /// 入参非法：文案由领域层自己产生，可直接对外。
    Invalid {
        /// 可直接展示的原因。
        reason: String,
    },
    /// 内部错误：对外只暴露固定文案，原因只进 `Debug`。
    Internal {
        /// 内部原因，不对外。
        reason: String,
    },
}

impl Display for AgentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { from, to } => {
                write!(formatter, "Run 状态不能从 {from} 变为 {to}")
            }
            Self::NotFound { what, id } => write!(formatter, "{what}不存在：{id}"),
            Self::Invalid { reason } => write!(formatter, "{reason}"),
            Self::Internal { .. } => write!(formatter, "内部错误，请稍后重试"),
        }
    }
}

impl Error for AgentError {}
