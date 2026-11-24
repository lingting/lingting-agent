//! 工具配置（`[tools]` 段，P3-005）。
//!
//! 全部字段有缺省值：`[tools]` 段整体缺省时按 [`Default`] 取 `.steps/p3/README.md` §4.10 的值，
//! 段存在但只写了部分字段时，缺的字段同样回落到 `Default`（`#[serde(default)]` 作用于整个结构）。
//!
//! 装载与校验在 `lib-bin-cli::config`：未知工具名、负数、`0` 上限、相对路径与空 host 都是
//! 启动期硬错误（退出码 2）。
//!
//! 本段**没有**「确认超时」：确认没有时限，挂起的 Run 只能由确认 / 取消 / 释放占用 / 服务关闭
//! 四条路径收尾（D-P3-023）。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

/// 单次工具调用超时的缺省值（毫秒）。
pub const DEFAULT_TIMEOUT_MS: u64 = 15_000;
/// 单次工具结果回灌给模型的最大字符数的缺省值。
pub const DEFAULT_MAX_RESULT_CHARS: usize = 8_000;
/// 一轮内并行执行上限的缺省值。
pub const DEFAULT_MAX_PARALLEL_CALLS: usize = 8;
/// 可重试错误的重试次数上限的缺省值（默认不重试，D-P3-008）。
pub const DEFAULT_MAX_RETRIES: u32 = 0;
/// 文件类工具单次读取上限的缺省值（字节）。
pub const DEFAULT_FILE_MAX_BYTES: u64 = 1_048_576;
/// HTTP 工具单次响应上限的缺省值（字节）。
pub const DEFAULT_HTTP_MAX_BYTES: u64 = 262_144;

/// 已知的内置工具名（P3-021 ~ P3-025 的实现落在 `agent-tool::builtin`）。
///
/// 只有一个用途：`[tools.permissions]` 的装载校验。写错工具名不会被静默忽略——那会让
/// 「我明明配了 deny」变成一句空话。
pub const BUILTIN_TOOL_NAMES: [&str; 5] = ["calculator", "current_time", "file_read", "file_search", "http_request"];

/// 单个工具的权限（`[tools.permissions]` 的取值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolPermissionKind {
    /// 允许：进模型可见清单，可执行。
    Allow,
    /// 拒绝：不进模型可见清单，执行期二次判定为 `DENIED`。
    Deny,
    /// 需确认：进模型可见清单，执行前挂起 Run 等客户端确认（D-P3-004）。
    Confirm,
}

/// 工具执行参数与权限。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ToolConfig {
    /// 单次工具调用超时（毫秒）：超时视为一次工具失败，不重试。
    pub timeout_ms: u64,
    /// 单次工具结果回灌给模型的最大字符数（按字符计）。
    pub max_result_chars: usize,
    /// 一轮内并行执行上限：超出部分排队。
    pub max_parallel_calls: usize,
    /// 可重试错误的重试次数上限；`0` 表示不重试（D-P3-008）。
    pub max_retries: u32,
    /// 按工具名给出的权限；未配置的工具按风险等级的缺省等级判定（README §4.3）。
    pub permissions: BTreeMap<String, ToolPermissionKind>,
    /// 文件类工具的白名单与上限（`[tools.file]`）。
    pub file: ToolFileConfig,
    /// HTTP 工具的白名单与上限（`[tools.http]`）。
    pub http: ToolHttpConfig,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_result_chars: DEFAULT_MAX_RESULT_CHARS,
            max_parallel_calls: DEFAULT_MAX_PARALLEL_CALLS,
            max_retries: DEFAULT_MAX_RETRIES,
            permissions: BTreeMap::new(),
            file: ToolFileConfig::default(),
            http: ToolHttpConfig::default(),
        }
    }
}

/// 文件类工具的安全边界（`[tools.file]`）。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ToolFileConfig {
    /// 允许读取的目录；空表示只允许应用根目录（`AGENT_HOME`）。必须是绝对路径。
    pub allow_dirs: Vec<PathBuf>,
    /// 单次读取的字节上限。
    pub max_bytes: u64,
}

impl Default for ToolFileConfig {
    fn default() -> Self {
        Self {
            allow_dirs: Vec::new(),
            max_bytes: DEFAULT_FILE_MAX_BYTES,
        }
    }
}

/// HTTP 工具的安全边界（`[tools.http]`）。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ToolHttpConfig {
    /// 允许访问的 host（可写 `host` 或 `host:port`）；空表示一律拒绝。
    pub allow_hosts: Vec<String>,
    /// 单次响应的字节上限。
    pub max_bytes: u64,
}

impl Default for ToolHttpConfig {
    fn default() -> Self {
        Self {
            allow_hosts: Vec::new(),
            max_bytes: DEFAULT_HTTP_MAX_BYTES,
        }
    }
}
