//! `llm.security`：密钥不出现在 `Debug` / `Display` / 错误文案里（P2-017）。
//!
//! 用 canary 密钥构造 `ProviderService`，检索所有可输出文本。服务端日志文件的
//! canary 检索归端到端验收 Z-1（P2-032）。

use crate::case::CaseFuture;
use crate::context::TestContext;
use anyhow::{Context, ensure};
use manager_agent::ProviderService;
use types_core::config::provider::{DEFAULT_CONTEXT_WINDOW, DEFAULT_MAX_OUTPUT_TOKENS};
use types_core::config::{ModelConfig, ProviderConfig, ProviderKind, Secret};

/// canary：只要出现在任何可输出文本里就算泄漏。
const CANARY: &str = "sk-canary-8b7f3d0c1e4a";

pub(super) fn security(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let provider = ProviderService::new(Some(canary_provider()));
        ensure!(provider.configured(), "写了 [provider] 段时应视为已配置");

        let config = provider.provider().context("已配置时必须能取到 Provider 配置")?;
        let missing = provider
            .model(Some("no-such-model"))
            .err()
            .context("未知模型必须解析失败")?;

        let rendered = [
            format!("{provider:?}"),
            format!("{config:?}"),
            format!("{}", config.api_key),
            format!("{:?}", config.api_key),
            format!("{missing}"),
            format!("{missing:?}"),
            missing.public_message(),
        ];
        for text in &rendered {
            ensure!(!text.contains(CANARY), "密钥出现在可输出文本里：{text}");
        }

        // 未配置 Provider：文案可读，同样不携带任何密钥。
        let unconfigured = ProviderService::new(None);
        ensure!(!unconfigured.configured(), "没有 [provider] 段时应视为未配置");
        let message = unconfigured
            .model(None)
            .err()
            .context("未配置 Provider 时解析模型必须失败")?
            .public_message();
        ensure!(
            message == "调用模型失败：未配置 Provider",
            "未配置 Provider 的文案不符：{message}"
        );

        Ok(vec![
            format!("已检索 {} 处可输出文本，canary 未出现", rendered.len()),
            format!("未配置 Provider：{message}"),
        ])
    })
}

/// 带 canary 密钥的 Provider 配置：密钥确实进了结构体，检索才有意义。
fn canary_provider() -> ProviderConfig {
    ProviderConfig {
        id: "canary".to_string(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.example.com/v1".to_string(),
        api_key: Secret::new(CANARY),
        default_model: "canary-model".to_string(),
        models: vec![ModelConfig {
            id: "canary-model".to_string(),
            context_window: DEFAULT_CONTEXT_WINDOW,
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
        }],
    }
}
