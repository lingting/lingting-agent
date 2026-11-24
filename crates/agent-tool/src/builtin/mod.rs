//! 五个内置工具（P3-021 ~ P3-025）：每个工具只实现 `Tool` trait，不新增机制。
//!
//! 装配口径：`[tools.file]` / `[tools.http]` 由装配层读成 [`BuiltinOptions`]（本 Crate 不认识
//! `types-core` 的配置类型），再由 [`register_all`] 一次性注册进 `ToolRegistry`。权限仍由
//! `[tools.permissions]` 与 `ToolRisk` 决定——注册表只负责过滤模型可见清单（D-P3-005），
//! 内置工具自己不做权限判定。
//!
//! 边界（README §4.8）：
//!
//! | 工具 | 本地文件操作 | 风险等级 | 默认权限 | 安全边界 |
//! |---|---|---|---|---|
//! | `calculator` | `None` | `ReadOnly` | allow | 自研递归下降，只支持四则、括号、取模与一元符号 |
//! | `current_time` | `None` | `ReadOnly` | allow | 只读系统时钟，不支持时区名 |
//! | `file_read` | `Read` | `ReadOnly` | allow | 白名单 + `canonicalize`，二进制与超限文件拒绝 |
//! | `file_search` | `Read` | `ReadOnly` | allow | 同一白名单，遍历期剪枝固定排除目录 |
//! | `http_request` | `None` | `Network` | **deny** | host 白名单 + 禁重定向 + 响应上限 |

mod calculator;
mod current_time;
mod file_read;
mod file_search;
mod http_request;
mod path;

use std::path::PathBuf;
use std::sync::Arc;

pub use calculator::Calculator;
pub use current_time::CurrentTime;
pub use file_read::FileRead;
pub use file_search::FileSearch;
pub use http_request::HttpRequestTool;

use crate::registry::{RegistryError, ToolRegistry};

/// 文件类工具单次读取上限的缺省值（字节），与 `[tools.file] max_bytes` 的缺省值一致。
pub const DEFAULT_FILE_MAX_BYTES: u64 = 1_048_576;
/// HTTP 工具单次响应上限的缺省值（字节），与 `[tools.http] max_bytes` 的缺省值一致。
pub const DEFAULT_HTTP_MAX_BYTES: u64 = 262_144;

/// 内置工具的构造参数：只放「安全边界」，不放执行参数（超时、并发、截断由执行器管）。
///
/// 与 `types-core::config` 的同名段一一对应，但不复用它的类型：配置类型不进 `agent-tool`
/// （P3-005 的依赖口径）。
#[derive(Debug, Clone)]
pub struct BuiltinOptions {
    /// 文件类工具的白名单根目录；空表示只允许应用根目录（`AGENT_HOME`）。
    pub file_allow_dirs: Vec<PathBuf>,
    /// 文件类工具单次读取上限（字节）。
    pub file_max_bytes: u64,
    /// HTTP 工具允许访问的 host（`host` 或 `host:port`）；空表示一律拒绝。
    pub http_allow_hosts: Vec<String>,
    /// HTTP 工具单次响应上限（字节）。
    pub http_max_bytes: u64,
}

impl Default for BuiltinOptions {
    /// 缺省即安全侧缺省：白名单为空（只允许应用根目录 / 一律拒绝网络）。
    fn default() -> Self {
        Self {
            file_allow_dirs: Vec::new(),
            file_max_bytes: DEFAULT_FILE_MAX_BYTES,
            http_allow_hosts: Vec::new(),
            http_max_bytes: DEFAULT_HTTP_MAX_BYTES,
        }
    }
}

/// 注册全部五个内置工具。
///
/// 名字与实现都是本 Crate 的常量，重名与非法名字属于装配期错误（不静默忽略）：
/// 调用方拿到 `Err` 就应当让启动失败，而不是带着半个工具表跑起来。
pub fn register_all(registry: &mut ToolRegistry, options: &BuiltinOptions) -> Result<(), RegistryError> {
    registry.register(Arc::new(Calculator))?;
    registry.register(Arc::new(CurrentTime))?;
    registry.register(Arc::new(FileRead::new(
        options.file_allow_dirs.clone(),
        options.file_max_bytes,
    )))?;
    registry.register(Arc::new(FileSearch::new(options.file_allow_dirs.clone())))?;
    registry.register(Arc::new(HttpRequestTool::new(
        options.http_allow_hosts.clone(),
        options.http_max_bytes,
    )))?;
    Ok(())
}
