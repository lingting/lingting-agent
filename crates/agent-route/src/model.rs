//! Provider / Model 解析：配置里「有哪些模型」与「本次用哪个」。
//!
//! 配置的装载与校验归 `service-core`（P2-017）；本模块只按 `types-core` 里的配置结构查询，
//! 未知模型一律 [`LlmError::UnknownModel`]（D-P2-015：由调用方映射 400 `模型不可用：<id>`）。

use types_core::config::{ModelConfig, ProviderConfig};

use crate::error::LlmError;

/// 模型路由表：包住 Provider 配置，提供默认模型与按 id 查找。
#[derive(Debug, Clone)]
pub struct ModelRoute {
    /// 装载好的 Provider 配置。
    config: ProviderConfig,
}

impl ModelRoute {
    /// 用装载好的 Provider 配置构造。
    pub fn new(config: ProviderConfig) -> Self {
        Self { config }
    }

    /// Provider 配置本身：Adapter 取 `base_url` / `api_key` 用。
    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    /// 默认模型（`provider.default_model`）。
    pub fn default_model(&self) -> Result<&ModelConfig, LlmError> {
        self.model(None)
    }

    /// 按 id 解析模型；`None` 表示用 `default_model`。
    pub fn model(&self, id: Option<&str>) -> Result<&ModelConfig, LlmError> {
        let id = id.unwrap_or(&self.config.default_model);
        self.config
            .models
            .iter()
            .find(|model| model.id == id)
            .ok_or_else(|| LlmError::UnknownModel { id: id.to_owned() })
    }
}
