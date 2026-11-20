//! Provider 配置的只读承载与模型解析（P2-017）。
//!
//! 装载与启动期校验收在启动方 `lib-bin-cli`（只有它读 `config.toml`），本模块只持有装载结果：
//! 已配置时把 `ProviderConfig` 交给 [`ModelRoute`]，模型查找仍然只有这一份实现；未配置时
//! 所有解析都返回 [`LlmError::Unconfigured`]，服务端照常启动、调用模型才失败。

use types_core::config::{ModelConfig, ProviderConfig};

use crate::error::LlmError;
use crate::model::ModelRoute;

/// Provider 配置服务：只读、进程内一份，由装配层持有（D-P2-032）。
#[derive(Debug, Clone)]
pub struct ProviderService {
    /// 已配置时为 `Some`；`None` 表示 `config.toml` 没有 `[provider]` 段。
    route: Option<ModelRoute>,
}

impl ProviderService {
    /// 用装载好的配置构造；`None` 表示未配置 Provider。
    pub fn new(provider: Option<ProviderConfig>) -> Self {
        Self {
            route: provider.map(ModelRoute::new),
        }
    }

    /// 是否配置了 Provider。
    pub fn configured(&self) -> bool {
        self.route.is_some()
    }

    /// Provider 配置本身：Adapter 取 `base_url` / `api_key` 用；未配置时为空。
    pub fn provider(&self) -> Option<&ProviderConfig> {
        self.route.as_ref().map(ModelRoute::config)
    }

    /// 模型路由表：需要 `default_model` 与按 id 查找时用它。
    pub fn route(&self) -> Option<&ModelRoute> {
        self.route.as_ref()
    }

    /// 默认模型（`provider.default_model`）；未配置 Provider 时返回 [`LlmError::Unconfigured`]。
    pub fn default_model(&self) -> Result<&ModelConfig, LlmError> {
        self.model(None)
    }

    /// 按 id 解析模型；`None` 表示用默认模型。
    pub fn model(&self, id: Option<&str>) -> Result<&ModelConfig, LlmError> {
        self.route.as_ref().ok_or(LlmError::Unconfigured)?.model(id)
    }
}
