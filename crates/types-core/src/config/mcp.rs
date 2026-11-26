//! MCP 服务器配置（`mcp.toml`，P4-004）。
//!
//! MCP 的配置**独立成文件**（应用根目录下的 `mcp.toml`），不写进 `config.toml`：它随 server
//! 数量增长，且改一次就要重启服务，与主配置的装载节奏不同。文件内容就是这一段配置本身：
//!
//! ```toml
//! connect_timeout_ms = 10000
//! reconnect_initial_backoff_ms = 1000
//! reconnect_max_backoff_ms = 30000
//! concurrency = "parallel"
//!
//! [[servers]]
//! id = "filesystem"
//! enabled = false
//! transport = "stdio"
//! command = "npx"
//! args = ["-y", "@modelcontextprotocol/server-filesystem", "D:/code"]
//! env = { LOG_LEVEL = "info" }
//! cwd = ""
//! permission = "allow"
//! concurrency = "parallel"
//! file_access = { "*" = "read", "write*" = "write" }
//!
//! [[servers]]
//! id = "remote"
//! enabled = true
//! transport = "http"
//! url = "https://example.com/mcp"
//! headers = { Authorization = "Bearer xxx" }
//! permission = "confirm"
//! file_access = { "*" = "none" }
//! ```
//!
//! 所有顶层字段有缺省值；`servers` 缺省为空（= 不启用任何 MCP server）。文件缺失等价于
//! 「没有 MCP 配置」，文件存在但内容为空则是启动期硬错误（不静默降级）。
//!
//! 装载与校验在 `lib-bin-cli::config`：`id` 规则与唯一性、transport 字段互斥与必填、
//! 空 `command` / 空 `args` 项 / 空 `env` 键 / 非法 URL / 空 `headers` 键、`0` 超时、
//! 非法枚举值与空 `file_access` 键都是启动期硬错误（退出码 2），错误文案给出 server `id` 与字段名。

use std::collections::BTreeMap;

use serde::Deserialize;

/// 单 server 握手超时的缺省值（毫秒）：连接 + initialize 的总上限。
pub const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 10_000;
/// 失败后自动重连的初始退避（毫秒）。
pub const DEFAULT_RECONNECT_INITIAL_BACKOFF_MS: u64 = 1_000;
/// 自动重连退避的上限（毫秒）；写 `0` 关闭自动重连。
pub const DEFAULT_RECONNECT_MAX_BACKOFF_MS: u64 = 30_000;

/// `mcp.toml` 的顶层配置。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    /// 单 server 握手超时（毫秒）：连接 + `initialize` 的总上限，超时即 `FAILED`（不阻塞服务启动）。
    pub connect_timeout_ms: u64,
    /// 失败后的自动重连初始退避（毫秒）。
    pub reconnect_initial_backoff_ms: u64,
    /// 自动重连退避上限（毫秒）；`0` 表示关闭自动重连（只能手动 `reconnect`）。
    pub reconnect_max_backoff_ms: u64,
    /// 未声明写操作的工具的并发缺省值（server 可覆盖）。
    pub concurrency: McpConcurrencyKind,
    /// 全部 server：按 `id` 唯一，`enabled = false` 的照常出现在管理接口里。
    pub servers: Vec<McpServerConfig>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            connect_timeout_ms: DEFAULT_CONNECT_TIMEOUT_MS,
            reconnect_initial_backoff_ms: DEFAULT_RECONNECT_INITIAL_BACKOFF_MS,
            reconnect_max_backoff_ms: DEFAULT_RECONNECT_MAX_BACKOFF_MS,
            concurrency: McpConcurrencyKind::default(),
            servers: Vec::new(),
        }
    }
}

/// 单个 MCP server 的配置。
#[derive(Debug, Clone, Deserialize)]
pub struct McpServerConfig {
    /// 唯一标识（`^[a-z][a-z0-9_]{0,31}$`）：模型可见工具名前缀的来源。
    pub id: String,
    /// 是否在启动时连接；缺省 `false`——只写配置不会启动子进程。
    #[serde(default)]
    pub enabled: bool,
    /// 传输方式与它的字段（`transport` 是判别键，两种 transport 的字段互斥）。
    #[serde(flatten)]
    pub transport: McpTransportConfig,
    /// server 级权限：MCP 工具的条目默认权限，缺省 `allow`。
    #[serde(default)]
    pub permission: McpPermissionKind,
    /// server 级并发覆盖；缺省取顶层 `concurrency`。
    #[serde(default)]
    pub concurrency: Option<McpConcurrencyKind>,
    /// 本地文件操作声明：键是 server 报告的工具原名或含 `*` 的模式，缺省 `none`。
    #[serde(default)]
    pub file_access: BTreeMap<String, McpFileAccessKind>,
}

/// 传输方式：stdio（本地子进程）或 http（远程 streamable HTTP）。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "transport", rename_all = "kebab-case")]
pub enum McpTransportConfig {
    /// 本地子进程，走 stdin / stdout。
    Stdio {
        /// 可执行命令；必填且不能为空。
        command: String,
        /// 命令参数；缺省为空，不允许空串项。
        #[serde(default)]
        args: Vec<String>,
        /// 追加到子进程环境变量（不覆盖继承的环境）；缺省为空。
        #[serde(default)]
        env: BTreeMap<String, String>,
        /// 工作目录，相对应用根目录；空表示不切换。
        #[serde(default)]
        cwd: String,
    },
    /// 远程 server，走 streamable HTTP（只支持静态请求头，不做 OAuth）。
    Http {
        /// server 地址；只接受 `http` / `https`。
        url: String,
        /// 每个请求附加的静态请求头（如 `Authorization`）；缺省为空。
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

/// 工具并发语义的配置取值。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpConcurrencyKind {
    /// 可并发执行（缺省）。
    #[default]
    Parallel,
    /// 独占执行：与该轮其它调用串行。
    Exclusive,
}

/// server 级权限的配置取值。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpPermissionKind {
    /// 允许（缺省）：进模型可见清单，可执行。
    #[default]
    Allow,
    /// 拒绝：不进清单，执行期二次判定为 `DENIED`。
    Deny,
    /// 需确认：执行前挂起 Run 等客户端确认。
    Confirm,
}

/// 本地文件操作声明的配置取值（与 `agent-tool` 的 `ToolFileAccess` 逐项对应）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpFileAccessKind {
    /// 不操作本地文件（缺省）。
    #[default]
    None,
    /// 只读。
    Read,
    /// 只写。
    Write,
    /// 读写。
    ReadWrite,
}
