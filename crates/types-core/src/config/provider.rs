//! Provider / Model 配置与密钥类型。
//!
//! `ProviderKind` 只声明 `openai-compatible` 一种形态（D-P2-001：P2 只接入 OpenAI 兼容协议）；
//! 未知 `kind` 由 serde 直接拒绝，装载方（P2-017）只需把错误转成可读文案。
//!
//! `api_key` 用 [`Secret`] 承载：`Debug` / `Display` 一律输出 `***`，取值只能经 [`Secret::expose`]，
//! 避免密钥随日志、错误或快照意外外泄（D-P2-017）。

use serde::Deserialize;

/// `context_window` 的缺省值（D-P2-012 / O-4）：只作兜底，实际预算由配置显式给出。
pub const DEFAULT_CONTEXT_WINDOW: usize = 8192;
/// `max_output_tokens` 的缺省值（D-P2-012 / O-4）。
pub const DEFAULT_MAX_OUTPUT_TOKENS: usize = 1024;

/// Provider 配置。
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    /// Provider 标识，用于日志与错误文案（不含密钥）。
    pub id: String,
    /// 协议种类；P2 只支持 OpenAI 兼容。
    pub kind: ProviderKind,
    /// 基地址，含版本段，例如 `https://api.openai.com/v1`。
    pub base_url: String,
    /// 密钥；缺省为空串（允许先启动、后补密钥）。
    #[serde(default)]
    pub api_key: Secret,
    /// 默认模型标识，必须出现在 `models` 中（由装载方校验）。
    pub default_model: String,
    /// 可用模型清单。
    #[serde(default)]
    pub models: Vec<ModelConfig>,
}

/// Provider 协议种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// OpenAI 兼容协议（P2 唯一支持的形态）。
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

/// 单个模型的配置。
#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    /// 模型标识，`default_model` 与 `message/create` 的 `model` 都指向它。
    pub id: String,
    /// 上下文窗口（token 数）。
    #[serde(default = "default_context_window")]
    pub context_window: usize,
    /// 单次输出上限（token 数）。
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: usize,
}

/// 密钥包装：`Debug` / `Display` 脱敏，取值只能走 [`Secret::expose`]。
#[derive(Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// 用明文构造，供环境变量覆盖与配置装载使用。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 取出明文。
    ///
    /// 这是唯一取值入口：只在构造上游请求时调用，不得写进日志、错误文案或任何响应。
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("***")
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("***")
    }
}

fn default_context_window() -> usize {
    DEFAULT_CONTEXT_WINDOW
}

fn default_max_output_tokens() -> usize {
    DEFAULT_MAX_OUTPUT_TOKENS
}
