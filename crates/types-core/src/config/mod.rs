//! 配置类型：Provider / Model / Agent（P2-009）。
//!
//! 这些是**内部配置类型**，不参与 SDK 生成（不加 `auto_*` 宏）；装载、环境变量覆盖与校验由
//! `lib-bin-cli` 的配置装载（P2-017）负责，本模块只定义结构与缺省值，避免各处重复写常量。
//!
//! 配置文件形态见 `.steps/p2/README.md` 的「配置面」：
//!
//! ```toml
//! [provider]
//! id = "default"
//! kind = "openai-compatible"
//! base_url = "https://api.openai.com/v1"
//! api_key = ""
//! default_model = "gpt-4o-mini"
//!
//! [[provider.models]]
//! id = "gpt-4o-mini"
//! context_window = 128000
//! max_output_tokens = 4096
//!
//! [agent]
//! max_concurrent_runs = 4
//! ```

pub mod agent;
pub mod provider;

pub use agent::AgentConfig;
pub use provider::{ModelConfig, ProviderConfig, ProviderKind, Secret};
