//! `ProviderError` → [`LlmError`]：分类与脱敏。
//!
//! 只有分类在对外文案上体现；Rig 的原始错误整条塞进 `source`，供日志读取，不进响应、
//! 不进 `Display`（D-P2-017）。

use std::error::Error;

use rig_core::error::ProviderError;

use crate::LlmError;

/// 把 Rig 的错误归类成项目错误。
pub(super) fn map(error: ProviderError) -> LlmError {
    let source = |error: ProviderError| -> Box<dyn Error + Send + Sync> { Box::new(error) };
    match error {
        // 401 / 403：密钥缺失、无效或被上游拒绝。
        error @ ProviderError::InvalidAuthentication(_) => LlmError::Authentication { source: source(error) },
        // 传输层失败：建连失败、连接重置、读取中断。
        error @ ProviderError::Http(_) => LlmError::Network { source: source(error) },
        // 非法 JSON：解码失败或协议不符。
        error @ ProviderError::Json(_) => LlmError::InvalidResponse { source: source(error) },
        // 上游没有结束这段回复，流就断了。
        error @ ProviderError::Truncated => LlmError::Truncated { source: source(error) },
        // 空闲超时：首帧前或帧间超过 `agent.llm_idle_timeout_ms`。
        error @ ProviderError::Stalled(_) => LlmError::IdleTimeout { source: source(error) },
        // 其余都归上游失败：`ProviderResponse`（非 2xx 或 2xx 错误信封）、`Provider`、
        // `Response`、`Request`、`Url`、`CacheExpired`、`MismatchedDimensions`、`Relayed`、
        // `UnsupportedOption`，以及 Rig 后续新增的变体。
        error => LlmError::Upstream { source: source(error) },
    }
}
