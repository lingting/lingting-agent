//! 命令执行结果：库只报告失败类别，退出码由 Binary 决定。
//!
//! 分类对应 `.steps/p1/tasks.md` 的退出码约定：`0` 正常、`1` 运行错误、`2` 参数与配置错误。

use crate::client::ClientError;
use std::fmt::Display;

/// 命令失败原因。
#[derive(Debug)]
pub enum CommandError {
    /// 参数或配置错误，对应退出码 2。
    Argument(String),
    /// 运行期错误，对应退出码 1。
    Runtime(String),
}

impl CommandError {
    /// 参数或配置错误。
    pub fn argument(error: impl Display) -> Self {
        Self::Argument(format!("{error:#}"))
    }

    /// 运行期错误。
    pub fn runtime(error: impl Display) -> Self {
        Self::Runtime(format!("{error:#}"))
    }

    /// 对外消息，保证为一行。
    pub fn message(&self) -> &str {
        match self {
            Self::Argument(message) | Self::Runtime(message) => message,
        }
    }
}

impl From<ClientError> for CommandError {
    fn from(error: ClientError) -> Self {
        Self::runtime(error)
    }
}

impl From<anyhow::Error> for CommandError {
    fn from(error: anyhow::Error) -> Self {
        Self::runtime(error)
    }
}
