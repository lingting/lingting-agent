//! LLM 调用错误：对外文案固定脱敏，原始错误只进 `source`（D-P2-017）。
//!
//! `Display` 与 [`LlmError::public_message`] 只输出可直接展示的结论，不含 URL、密钥、
//! 上游响应体与内部细节；原始错误经 [`Error::source`] 交给日志。Adapter 构造 `source` 时
//! 不得放入含密钥的结构（请求头不在错误里出现）。

use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// LLM 调用错误。
#[derive(Debug)]
pub enum LlmError {
    /// 配置里没有这个模型；文案带 id，由调用方映射成 400（D-P2-015）。
    UnknownModel {
        /// 请求里给出的模型标识。
        id: String,
    },
    /// 没有配置 Provider（`config.toml` 缺 `[provider]` 段）：服务可以启动，但调用不了模型。
    Unconfigured,
    /// 内部请求不合法（例如没有任何消息）：属于调用方缺陷，不是上游错误。
    InvalidRequest {
        /// 内部原因，只进 `Debug`。
        reason: &'static str,
    },
    /// 认证失败：密钥缺失、无效或被上游拒绝。
    Authentication {
        /// 原始错误，不对外。
        source: Box<dyn Error + Send + Sync>,
    },
    /// 上游失败：非 2xx、错误信封、请求被拒绝等。
    Upstream {
        /// 原始错误，不对外。
        source: Box<dyn Error + Send + Sync>,
    },
    /// 网络层失败：建连失败或传输中途断开。
    Network {
        /// 原始错误，不对外。
        source: Box<dyn Error + Send + Sync>,
    },
    /// 响应被截断：上游没有结束这段回复就断了。
    Truncated {
        /// 原始错误，不对外。
        source: Box<dyn Error + Send + Sync>,
    },
    /// 响应无法解析：非法 JSON 或协议不符。
    InvalidResponse {
        /// 原始错误，不对外。
        source: Box<dyn Error + Send + Sync>,
    },
    /// 流式读取空闲超时（`agent.llm_idle_timeout_ms`）。
    IdleTimeout {
        /// 原始错误，不对外。
        source: Box<dyn Error + Send + Sync>,
    },
}

impl LlmError {
    /// 可直接展示的文案：固定脱敏，未知模型附带请求里的 id。
    pub fn public_message(&self) -> String {
        match self {
            Self::UnknownModel { id } => format!("模型不可用：{id}"),
            Self::Unconfigured => "调用模型失败：未配置 Provider".to_owned(),
            Self::InvalidRequest { .. } => "调用模型失败：请求无效".to_owned(),
            Self::Authentication { .. } => "调用模型失败：认证失败".to_owned(),
            Self::Upstream { .. } => "调用模型失败：上游服务错误".to_owned(),
            Self::Network { .. } => "调用模型失败：网络异常".to_owned(),
            Self::Truncated { .. } => "调用模型失败：响应不完整".to_owned(),
            Self::InvalidResponse { .. } => "调用模型失败：响应无法解析".to_owned(),
            Self::IdleTimeout { .. } => "调用模型失败：响应超时".to_owned(),
        }
    }
}

impl Display for LlmError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.public_message())
    }
}

impl Error for LlmError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Authentication { source }
            | Self::Upstream { source }
            | Self::Network { source }
            | Self::Truncated { source }
            | Self::InvalidResponse { source }
            | Self::IdleTimeout { source } => Some(source.as_ref()),
            Self::UnknownModel { .. } | Self::Unconfigured | Self::InvalidRequest { .. } => None,
        }
    }
}
