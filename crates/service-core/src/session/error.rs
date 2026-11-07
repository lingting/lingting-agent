//! 会话业务错误：只表达业务语义，HTTP 状态码由 `lib-web` 映射。

use std::fmt::{Display, Formatter};

/// 会话业务错误。
#[derive(Debug)]
pub enum SessionError {
    /// 目标对象不存在（对外 404）。
    NotFound(String),
    /// 请求参数或当前状态不满足业务规则（对外 400）。
    Invalid(String),
    /// 服务内部错误（对外 500）；消息只进日志，不对外返回。
    Internal(String),
}

impl SessionError {
    /// 对外错误消息。
    pub fn message(&self) -> &str {
        match self {
            Self::NotFound(message) | Self::Invalid(message) | Self::Internal(message) => message,
        }
    }
}

impl Display for SessionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for SessionError {}
