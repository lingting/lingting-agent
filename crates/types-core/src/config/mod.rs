//! 配置类型：Provider / Model / Agent / Tools（P2-009、P3-005）。
//!
//! 这些是**内部配置类型**，不参与 SDK 生成（不加 `auto_*` 宏）；装载、环境变量覆盖与校验由
//! `lib-bin-cli` 的配置装载（P2-017）负责，本模块只定义结构与缺省值，避免各处重复写常量。
//!
//! 配置文件形态见 `.steps/p2/README.md` 与 `.steps/p3/README.md` §4.10 的「配置面」：
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
//! max_concurrent_runs = "3C"
//! max_tool_rounds = 8
//!
//! [tools]
//! timeout_ms = 15000
//! max_result_chars = 8000
//!
//! [tools.permissions]
//! http_request = "deny"
//!
//! [tools.file]
//! allow_dirs = ["D:/code/lingting/lingting-agent"]
//!
//! [tools.http]
//! allow_hosts = []
//! ```

pub mod agent;
pub mod provider;
pub mod tools;

pub use agent::{AgentConfig, Concurrency};
pub use provider::{ModelConfig, ProviderConfig, ProviderKind, Secret};
pub use tools::{ToolConfig, ToolFileConfig, ToolHttpConfig, ToolPermissionKind};
