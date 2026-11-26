//! MCP server 的连接侧配置：装配层把 `mcp.toml` 翻译成这里的类型再喂进来。
//!
//! 本 Crate 不认识 `types-core`（D-P4-001）：`id` 的正则、URL scheme、枚举取值都在
//! `lib-bin-cli::config` 的装载期校验过，这里只承载「已经可信的连接参数」。运行时仍做最小
//! 防御（请求头 / 子进程建不起来时归一成 [`McpError::ServerUnavailable`]），但不重复整套校验。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use agent_tool::{ToolConcurrency, ToolPermission};

use crate::file_access::FileAccessRules;

/// 握手超时的缺省值（毫秒）：连接 + `initialize` 的总上限。
pub const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 10_000;

/// 一个 MCP server 的连接参数与工具声明。
///
/// `permission` / `concurrency` / `file_access` 是配置里对「这个 server 的工具」的声明：
/// 权限进注册表的条目默认权限（D-P4-007），并发与文件操作声明进 `Tool` 的自述（D-P4-006）。
#[derive(Debug, Clone)]
pub struct McpServerSpec {
    /// 全局唯一标识：模型可见工具名前缀的来源。
    pub id: String,
    /// 传输方式与它的字段。
    pub transport: McpTransport,
    /// 握手超时（连接 + `initialize` 的总上限）。
    pub connect_timeout: Duration,
    /// server 级权限：该 server 全部工具的条目默认权限。
    pub permission: ToolPermission,
    /// server 级并发：未声明写操作的工具取它（写声明恒串行）。
    pub concurrency: ToolConcurrency,
    /// 本地文件操作声明：按 server 报告的工具原名解析。
    pub file_access: FileAccessRules,
}

/// 传输方式：stdio（本地子进程）或 http（远程 streamable HTTP）。
#[derive(Debug, Clone)]
pub enum McpTransport {
    /// 本地子进程，走 stdin / stdout。
    Stdio {
        /// 可执行命令。
        command: String,
        /// 命令参数。
        args: Vec<String>,
        /// 追加到子进程的环境变量（不覆盖继承的环境）。
        env: BTreeMap<String, String>,
        /// 工作目录；`None` 表示不切换。
        cwd: Option<PathBuf>,
    },
    /// 远程 server，走 streamable HTTP。
    Http {
        /// server 地址（装载期已校验为 `http` / `https`）。
        url: String,
        /// 每个请求附加的静态请求头。
        headers: BTreeMap<String, String>,
    },
}

impl McpTransport {
    /// 日志与接口里使用的传输方式标签。
    pub fn label(&self) -> &'static str {
        match self {
            Self::Stdio { .. } => "stdio",
            Self::Http { .. } => "http",
        }
    }
}

impl McpServerSpec {
    /// 以 stdio 建一份最小配置：其余字段取缺省值（超时 10s、`allow`、`parallel`、无声明）。
    pub fn stdio(id: impl Into<String>, command: impl Into<String>) -> Self {
        Self::new(
            id,
            McpTransport::Stdio {
                command: command.into(),
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
            },
        )
    }

    /// 以 http 建一份最小配置：其余字段取缺省值（同 [`McpServerSpec::stdio`]）。
    pub fn http(id: impl Into<String>, url: impl Into<String>) -> Self {
        Self::new(
            id,
            McpTransport::Http {
                url: url.into(),
                headers: BTreeMap::new(),
            },
        )
    }

    /// 只给 id 与 transport，其余取缺省值。
    pub fn new(id: impl Into<String>, transport: McpTransport) -> Self {
        Self {
            id: id.into(),
            transport,
            connect_timeout: Duration::from_millis(DEFAULT_CONNECT_TIMEOUT_MS),
            permission: ToolPermission::Allow,
            concurrency: ToolConcurrency::Parallel,
            file_access: FileAccessRules::default(),
        }
    }
}
