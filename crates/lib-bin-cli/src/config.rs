//! 配置装载：命令行 > 配置文件 > 默认值。
//!
//! 配置文件位于应用根目录下的 `config.toml`，形如：
//!
//! ```toml
//! [server]
//! addr = "127.0.0.1"
//! port = 20100
//! log_level = "info"
//!
//! [runtime.events]
//! # 保留的会话目录数上限；0 表示不限制
//! max_sessions = 200
//! # data/events 下总字节数上限；0 表示不限制
//! max_bytes = 536870912
//!
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
//! max_parallel_calls = 8
//! max_retries = 0
//!
//! [tools.permissions]
//! http_request = "deny"
//!
//! [tools.file]
//! allow_dirs = ["D:/code/lingting/lingting-agent"]
//! max_bytes = 1048576
//!
//! [tools.http]
//! allow_hosts = []
//! max_bytes = 262144
//! ```
//!
//! 配置只来自命令行与这个文件（应用根目录本身由 `AGENT_HOME` 决定），不为任何配置项读环境变量。
//!
//! 服务端与客户端共用同一份地址来源（D-012）：`agent server` 用它决定监听地址，
//! `agent tui` / `agent run` / `agent status` 用它决定要连哪个服务。
//!
//! `[provider]` 段可以整体缺省：此时视为「Provider 未配置」，服务端照常启动，首次调用模型才失败。
//! 段一旦存在，缺字段、未知 `kind` 与非法取值都是启动期硬错误（退出码 2）。
//!
//! `[tools]` 段同理：整段可缺省（取 `types-core` 的缺省值），写下的每一项都要自洽——
//! 未知工具名、`0` 上限、相对路径与空 host 都是启动期硬错误（P3-005）。
//!
//! MCP 的配置**不在** `config.toml` 里：它独立成应用根目录下的 `mcp.toml`（与 `config.toml` 同级，
//! P4-004），由 [`resolve_server`] 一并装载、一并无条件校验。文件缺失等价于「没有 MCP 配置」，
//! 文件存在但为空、字段混用、`id` 不合规与非法枚举值都是启动期硬错误（退出码 2）。客户端命令不读它。
use crate::command::ServerArgs;
use anyhow::{Context, Result, anyhow};
use lib_core::directorys::directorys;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::str::FromStr;
use tracing_subscriber::filter::LevelFilter;
use types_core::config::tools::BUILTIN_TOOL_NAMES;
use types_core::config::{
    AgentConfig, Concurrency, McpConfig, McpServerConfig, McpTransportConfig, ProviderConfig, ToolConfig,
};
use url::Url;

/// 默认监听地址。
const DEFAULT_HOST: &str = "127.0.0.1";
/// 默认监听端口（固定端口，见 D-022）：release 构建用服务端端口 20100。
#[cfg(not(debug_assertions))]
const DEFAULT_PORT: u16 = 20100;
/// 默认监听端口（固定端口，见 D-022）：debug 构建用开发环境服务端端口 20102，与前端 Vite 的代理目标一致。
#[cfg(debug_assertions)]
const DEFAULT_PORT: u16 = 20102;

/// Server 运行配置。
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// 监听地址。
    pub host: String,
    /// 监听端口。
    pub port: u16,
    /// 日志级别。
    pub log_level: LevelFilter,
    /// 事件文件轮转配置（`[runtime.events]`）。
    pub runtime_events: RuntimeEventsConfig,
    /// Provider / Agent 配置（`[provider]` / `[agent]`）。
    pub llm: LlmConfig,
    /// MCP server 配置（独立文件 `mcp.toml`，P4-004）；文件缺失等价于「没有 MCP」。
    pub mcp: McpConfig,
}

/// 事件文件轮转配置：`None` 表示取服务端默认值，`Some(0)` 表示该维度不限制。
#[derive(Debug, Clone, Copy, Default)]
pub struct RuntimeEventsConfig {
    /// 保留的会话目录数上限。
    pub max_sessions: Option<u64>,
    /// `data/events` 下总字节数上限。
    pub max_bytes: Option<u64>,
}

/// Provider / Agent 配置：只从 `config.toml` 装载，没有命令行参数或环境变量覆盖。
///
/// `provider` 为 `None` 表示 `[provider]` 整段缺省：服务端照常启动，首次调用模型才失败。
#[derive(Debug, Clone, Default)]
pub struct LlmConfig {
    /// Provider 配置。
    pub provider: Option<ProviderConfig>,
    /// Agent 运行参数；缺省值见 `types-core`。
    pub agent: AgentConfig,
    /// 工具执行参数与权限（`[tools]`）；缺省值见 `types-core`。
    pub tools: ToolConfig,
}

/// 客户端连接配置。
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// 服务基地址，形如 `http://127.0.0.1:20100`。
    pub base_url: String,
    /// 客户端日志级别。
    pub log_level: LevelFilter,
}

/// 按优先级装载 Server 配置。
///
/// `--async` 不参与配置解析：它只决定「派生后台进程还是前台常驻」，由调用方读取
/// [`ServerArgs::asynchronous`] 自行分支（P2.2-011）。
pub fn resolve_server(args: &ServerArgs) -> Result<ServerConfig> {
    let file = load_file()?;
    let (host, port) = AddressSources::load(args.addr.as_deref(), args.port, &file)?.resolve()?;
    Ok(ServerConfig {
        host,
        port,
        log_level: resolve_level(args.log_level.as_deref(), &file)?,
        runtime_events: RuntimeEventsConfig {
            max_sessions: file.runtime.events.max_sessions,
            max_bytes: file.runtime.events.max_bytes,
        },
        llm: llm_from(&file)?,
        mcp: load_mcp()?,
    })
}

/// 按优先级装载客户端配置：`address` 是客户端命令的 `--server`。
///
/// 客户端只关心“连哪里”，因此与服务端共用地址来源，但把结果拼成基地址；
/// 端口为 0 同样拒绝（服务端会改用随机端口，客户端无从寻址，见 D-022）。
pub fn resolve_client(address: Option<&str>) -> Result<ClientConfig> {
    let file = load_file()?;
    let (host, port) = AddressSources::load(address, None, &file)?.resolve()?;
    Ok(ClientConfig {
        base_url: format!("http://{host}:{port}"),
        log_level: resolve_level(None, &file)?,
    })
}

/// 地址的三个来源层级。
///
/// `cli_port` 只有 `server --port` 会传入，客户端命令没有独立的端口参数。
struct AddressSources {
    cli: Option<Address>,
    cli_port: Option<u16>,
    file: Option<Address>,
    file_port: Option<u16>,
}

impl AddressSources {
    /// 读取配置文件中的地址，与命令行参数一起组成三个层级。
    fn load(cli: Option<&str>, cli_port: Option<u16>, file: &FileConfig) -> Result<Self> {
        Ok(Self {
            cli: optional_address(cli)?,
            cli_port,
            file: optional_address(file.server.addr.as_deref())?,
            file_port: file.server.port,
        })
    }

    /// 按优先级取出地址与端口；命令行里的地址允许自带端口，其优先级高于独立的端口参数下一级。
    fn resolve(self) -> Result<(String, u16)> {
        let host = self
            .cli
            .as_ref()
            .map(|address| address.host.clone())
            .or_else(|| self.file.as_ref().map(|address| address.host.clone()))
            .unwrap_or_else(|| DEFAULT_HOST.to_owned());

        let port = self
            .cli_port
            .or_else(|| self.cli.as_ref().and_then(|address| address.port))
            .or(self.file_port)
            .or_else(|| self.file.as_ref().and_then(|address| address.port))
            .unwrap_or(DEFAULT_PORT);

        if host.trim().is_empty() {
            return Err(anyhow!("地址不能为空"));
        }
        if port == 0 {
            return Err(anyhow!("端口不能为 0（0 会让服务端使用随机端口）"));
        }
        Ok((host, port))
    }
}

/// 日志级别：命令行 > 配置文件 > 默认 `info`。
fn resolve_level(cli: Option<&str>, file: &FileConfig) -> Result<LevelFilter> {
    let level = cli.or(file.server.log_level.as_deref());
    match level {
        Some(level) => LevelFilter::from_str(&level.trim().to_ascii_lowercase())
            .map_err(|_| anyhow!("日志级别无效：{level}（可选 off / error / warn / info / debug / trace）")),
        None => Ok(LevelFilter::INFO),
    }
}

/// 一个地址层级解析出的监听地址；`addr` 允许携带端口。
#[derive(Debug)]
struct Address {
    host: String,
    port: Option<u16>,
}

fn optional_address(value: Option<&str>) -> Result<Option<Address>> {
    value.map(parse_address).transpose()
}

/// 解析地址文本：支持 `host`、`host:port` 与 `http://host:port` 三种写法。
fn parse_address(value: &str) -> Result<Address> {
    let value = value.trim();
    let value = value
        .split_once("://")
        .map_or(value, |(_, rest)| rest)
        .trim_end_matches('/');
    let (host, port) = match value.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|item| item.is_ascii_digit()) => (host, Some(port)),
        _ => (value, None),
    };

    if host.trim().is_empty() {
        return Err(anyhow!("地址不能为空：{value}"));
    }

    let port = port
        .map(|port| port.parse::<u16>().map_err(|_| anyhow!("端口无效：{port}")))
        .transpose()?;

    Ok(Address {
        host: host.to_owned(),
        port,
    })
}

fn load_file() -> Result<FileConfig> {
    let path = directorys()?.config_file.clone();
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("解析配置文件失败：{}", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(FileConfig::default()),
        Err(error) => Err(error).with_context(|| format!("读取配置文件失败：{}", path.display())),
    }
}

/// 配置文件结构，未出现的层级全部取默认值。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileConfig {
    server: FileServer,
    runtime: FileRuntime,
    /// `[provider]` 整段可缺省：缺省表示未配置 Provider。
    provider: Option<ProviderConfig>,
    /// `[agent]`：整段与单个字段都可缺省，缺的取 `AgentConfig::default`。
    agent: AgentConfig,
    /// `[tools]`：整段与单个字段都可缺省，缺的取 `ToolConfig::default`。
    tools: ToolConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileServer {
    addr: Option<String>,
    port: Option<u16>,
    log_level: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileRuntime {
    events: FileRuntimeEvents,
}

/// 事件文件轮转配置：字段缺省时由服务端取默认值。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileRuntimeEvents {
    max_sessions: Option<u64>,
    max_bytes: Option<u64>,
}

/// 解析并校验一份配置文本：用例用它验证装载规则，不读写文件系统。
pub fn parse_llm(text: &str) -> Result<LlmConfig> {
    let file: FileConfig = toml::from_str(text).context("解析配置文本失败")?;
    llm_from(&file)
}

/// 从已解析的文件配置里取出并校验 Provider / Agent / Tools 段。
fn llm_from(file: &FileConfig) -> Result<LlmConfig> {
    if let Some(provider) = file.provider.as_ref() {
        validate_provider(provider)?;
    }
    validate_agent(&file.agent)?;
    validate_tools(&file.tools)?;
    Ok(LlmConfig {
        provider: file.provider.clone(),
        agent: file.agent.clone(),
        tools: file.tools.clone(),
    })
}

/// `file_access` 的合法取值：与 `agent-tool` 的 `ToolFileAccess` 逐项对应。
const FILE_ACCESS_KINDS: [&str; 4] = ["none", "read", "write", "read-write"];

/// 装载应用根目录下的 `mcp.toml`：文件缺失等价于「没有 MCP 配置」（零 server）。
///
/// 与 `config.toml` 分开装载：MCP 的 server 会随使用增长，且改一次就要重启服务；
/// 客户端命令也不读它（只有服务端需要认识 MCP）。
fn load_mcp() -> Result<McpConfig> {
    let path = directorys()?.mcp_file.clone();
    match fs::read_to_string(&path) {
        Ok(text) => parse_mcp(&text).with_context(|| format!("解析 MCP 配置文件失败：{}", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(McpConfig::default()),
        Err(error) => Err(error).with_context(|| format!("读取 MCP 配置文件失败：{}", path.display())),
    }
}

/// 解析并校验一份 `mcp.toml` 文本：用例用它验证装载规则，不读写文件系统。
///
/// 先按 `toml::Value` 做结构与枚举校验（错误文案才能给出 server `id` 与字段名），再反序列化成
/// 类型；文件存在但内容为空是硬错误——写了个空文件等于「我本想配置但没写成」，不能静默当成零 server。
pub fn parse_mcp(text: &str) -> Result<McpConfig> {
    if text.trim().is_empty() {
        return Err(anyhow!("mcp.toml 不能为空：不启用 MCP 请删除该文件"));
    }
    let mut table = match toml::from_str::<toml::Value>(text).context("解析 mcp.toml 失败")? {
        toml::Value::Table(table) => table,
        _ => return Err(anyhow!("mcp.toml 的顶层必须是配置项（键值对）")),
    };
    let servers = match table.remove("servers") {
        Some(value) => parse_servers(value)?,
        None => Vec::new(),
    };
    let mut config = McpConfig::deserialize(toml::Value::Table(table)).context("mcp.toml 的顶层配置不合法")?;
    config.servers = servers;
    validate_mcp(&config)?;
    Ok(config)
}

/// 逐条反序列化 server：每条先做 `transport` 的结构校验，错误文案都带上 server `id`。
fn parse_servers(value: toml::Value) -> Result<Vec<McpServerConfig>> {
    let toml::Value::Array(items) = value else {
        return Err(anyhow!("mcp.toml 的 servers 必须是数组（用 [[servers]] 声明）"));
    };
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let toml::Value::Table(table) = &item else {
                return Err(anyhow!(
                    "mcp.toml 的第 {} 个 server 必须是配置表（用 [[servers]] 声明）",
                    index + 1
                ));
            };
            let label = match table.get("id").and_then(toml::Value::as_str) {
                Some(id) if !id.trim().is_empty() => format!("server {id}"),
                _ => format!("第 {} 个 server", index + 1),
            };
            validate_transport_fields(table, &label)?;
            McpServerConfig::deserialize(item).with_context(|| format!("mcp.toml 的 {label} 配置不合法"))
        })
        .collect()
}

/// `transport` 判别键与两种 transport 的字段互斥、必填字段与枚举取值。
///
/// 这些检查必须按原文做：反序列化只报「取值不在枚举里」，报不出 `id` 与字段名；
/// 而字段混用（stdio 写 `url`）会被 serde 静默丢掉，等于让配置「看起来生效了」。
fn validate_transport_fields(table: &toml::Table, label: &str) -> Result<()> {
    let transport = table
        .get("transport")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| anyhow!("mcp.toml 的 {label} 缺少 transport（stdio 或 http）"))?;
    let (required, forbidden): (&[&str], &[&str]) = match transport {
        "stdio" => (&["command"], &["url", "headers"]),
        "http" => (&["url"], &["command", "args", "env", "cwd"]),
        other => {
            return Err(anyhow!(
                "mcp.toml 的 {label} 的 transport 不合法：{other}（可选 stdio / http）"
            ));
        }
    };
    for field in forbidden {
        if table.contains_key(*field) {
            return Err(anyhow!("mcp.toml 的 {label} 是 {transport} transport，不能写 {field}"));
        }
    }
    for field in required {
        match table.get(*field) {
            Some(toml::Value::String(value)) if !value.trim().is_empty() => {}
            Some(_) => return Err(anyhow!("mcp.toml 的 {label} 的 {field} 必须是非空字符串")),
            None => return Err(anyhow!("mcp.toml 的 {label} 缺少 {field}")),
        }
    }

    validate_enum_field(table, label, "permission", &["allow", "deny", "confirm"])?;
    validate_enum_field(table, label, "concurrency", &["parallel", "exclusive"])?;
    validate_file_access(table, label)
}

/// 枚举字段的原文校验：取值必须落在给定集合里。
fn validate_enum_field(table: &toml::Table, label: &str, field: &str, allowed: &[&str]) -> Result<()> {
    let Some(value) = table.get(field) else {
        return Ok(());
    };
    let Some(text) = value.as_str() else {
        return Err(anyhow!("mcp.toml 的 {label} 的 {field} 必须是字符串"));
    };
    if allowed.contains(&text) {
        return Ok(());
    }
    Err(anyhow!(
        "mcp.toml 的 {label} 的 {field} 取值不合法：{text}（可选 {}）",
        allowed.join(" / ")
    ))
}

/// `file_access` 的原文校验：必须是「工具名或 `*` 模式 → 枚举值」的表，键不能为空。
fn validate_file_access(table: &toml::Table, label: &str) -> Result<()> {
    let Some(value) = table.get("file_access") else {
        return Ok(());
    };
    let Some(patterns) = value.as_table() else {
        return Err(anyhow!(
            "mcp.toml 的 {label} 的 file_access 必须是表（工具名或 * 模式 → {}）",
            FILE_ACCESS_KINDS.join(" / ")
        ));
    };
    for (pattern, access) in patterns {
        if pattern.trim().is_empty() {
            return Err(anyhow!("mcp.toml 的 {label} 的 file_access 不能有空模式"));
        }
        let text = access
            .as_str()
            .ok_or_else(|| anyhow!("mcp.toml 的 {label} 的 file_access[{pattern}] 必须是字符串"))?;
        if !FILE_ACCESS_KINDS.contains(&text) {
            return Err(anyhow!(
                "mcp.toml 的 {label} 的 file_access[{pattern}] 取值不合法：{text}（可选 {}）",
                FILE_ACCESS_KINDS.join(" / ")
            ));
        }
    }
    Ok(())
}

/// MCP 配置的语义校验：`id` 规则与唯一性、各 transport 的必填项与非空键、超时非 0。
fn validate_mcp(config: &McpConfig) -> Result<()> {
    require_positive("mcp.toml connect_timeout_ms", config.connect_timeout_ms)?;
    require_positive(
        "mcp.toml reconnect_initial_backoff_ms",
        config.reconnect_initial_backoff_ms,
    )?;
    // `reconnect_max_backoff_ms = 0` 是「关闭自动重连」，属于合法取值，不校验。

    let mut ids = HashSet::new();
    for server in &config.servers {
        let id = server.id.trim();
        if !valid_server_id(id) {
            return Err(anyhow!(
                "mcp.toml 的 server id 不合法：{}（规则 ^[a-z][a-z0-9_]{{0,31}}$）",
                server.id
            ));
        }
        if !ids.insert(id.to_owned()) {
            return Err(anyhow!("mcp.toml 的 server id 重复：{id}"));
        }
        match &server.transport {
            McpTransportConfig::Stdio { command, args, env, .. } => {
                if command.trim().is_empty() {
                    return Err(anyhow!("mcp.toml 的 server {id} 的 command 不能为空"));
                }
                if args.iter().any(|argument| argument.is_empty()) {
                    return Err(anyhow!("mcp.toml 的 server {id} 的 args 不能包含空串"));
                }
                if env.keys().any(|key| key.trim().is_empty()) {
                    return Err(anyhow!("mcp.toml 的 server {id} 的 env 不能有空键"));
                }
            }
            McpTransportConfig::Http { url, headers } => {
                validate_http_url(url, id)?;
                if headers.keys().any(|key| key.trim().is_empty()) {
                    return Err(anyhow!("mcp.toml 的 server {id} 的 headers 不能有空键"));
                }
            }
        }
        if server.file_access.keys().any(|pattern| pattern.trim().is_empty()) {
            return Err(anyhow!("mcp.toml 的 server {id} 的 file_access 不能有空模式"));
        }
    }
    Ok(())
}

/// server id 规则：`^[a-z][a-z0-9_]{0,31}$`（工具名前缀的来源，必须能安全拼进工具名）。
fn valid_server_id(id: &str) -> bool {
    match id.as_bytes().split_first() {
        Some((first, rest)) => {
            first.is_ascii_lowercase()
                && rest.len() <= 31
                && rest
                    .iter()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        }
        None => false,
    }
}

/// HTTP transport 的地址校验：只接受 `http` / `https`，且必须能解析出 host。
fn validate_http_url(url: &str, id: &str) -> Result<()> {
    let parsed = Url::parse(url.trim()).map_err(|_| anyhow!("mcp.toml 的 server {id} 的 url 不是有效地址：{url}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(anyhow!("mcp.toml 的 server {id} 的 url 只支持 http / https：{url}"));
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(anyhow!("mcp.toml 的 server {id} 的 url 缺少主机名：{url}"));
    }
    Ok(())
}

/// Provider 校验：写了 `[provider]` 段就要求整段自洽，任何非法取值都是启动期硬错误。
fn validate_provider(provider: &ProviderConfig) -> Result<()> {
    if provider.id.trim().is_empty() {
        return Err(anyhow!("[provider] id 不能为空"));
    }
    if provider.base_url.trim().is_empty() {
        return Err(anyhow!("[provider] base_url 不能为空"));
    }
    Url::parse(provider.base_url.trim())
        .map_err(|_| anyhow!("[provider] base_url 不是有效 URL：{}", provider.base_url))?;
    if provider.models.is_empty() {
        return Err(anyhow!("[provider] models 不能为空：至少要配置一个模型"));
    }

    // 模型 id 既要有值，也要能唯一定位：重复会让查找结果取决于配置顺序。
    let mut ids = HashSet::new();
    for model in &provider.models {
        if model.id.trim().is_empty() {
            return Err(anyhow!("[provider] 模型 id 不能为空"));
        }
        if !ids.insert(model.id.as_str()) {
            return Err(anyhow!("[provider] 模型 id 重复：{}", model.id));
        }
    }
    if !ids.contains(provider.default_model.as_str()) {
        return Err(anyhow!(
            "[provider] default_model 不在 models 中：{}",
            provider.default_model
        ));
    }

    // 预算来源就是这两个数（D-P2-012 / O-4）：0 无法使用，输出上限不能超过上下文窗口。
    for model in &provider.models {
        require_positive(
            &format!("模型 {} 的 context_window", model.id),
            model.context_window as u64,
        )?;
        require_positive(
            &format!("模型 {} 的 max_output_tokens", model.id),
            model.max_output_tokens as u64,
        )?;
        if model.max_output_tokens > model.context_window {
            return Err(anyhow!(
                "模型 {} 的 max_output_tokens 不能大于 context_window（{} > {}）",
                model.id,
                model.max_output_tokens,
                model.context_window
            ));
        }
    }
    Ok(())
}

/// Agent 参数校验：全部是正数，写 0 会让并发、超时或预算不可用。
fn validate_agent(agent: &AgentConfig) -> Result<()> {
    validate_max_concurrent_runs(agent.max_concurrent_runs)?;
    require_positive("[agent] run_queue_timeout_ms", agent.run_queue_timeout_ms)?;
    require_positive("[agent] llm_idle_timeout_ms", agent.llm_idle_timeout_ms)?;
    require_positive("[agent] context_max_messages", agent.context_max_messages as u64)?;
    require_positive("[agent] context_safety_tokens", agent.context_safety_tokens as u64)?;
    require_positive("[agent] max_tool_rounds", agent.max_tool_rounds as u64)
}

/// 工具配置校验：数值上限都要大于 0，白名单要么是绝对目录，要么是 host（P3-005）。
fn validate_tools(tools: &ToolConfig) -> Result<()> {
    require_positive("[tools] timeout_ms", tools.timeout_ms)?;
    require_positive("[tools] max_result_chars", tools.max_result_chars as u64)?;
    require_positive("[tools] max_parallel_calls", tools.max_parallel_calls as u64)?;
    // `max_retries = 0` 是「不重试」，属于缺省行为，不校验。

    validate_permissions(tools)?;

    for directory in &tools.file.allow_dirs {
        if !directory.is_absolute() {
            return Err(anyhow!(
                "[tools.file] allow_dirs 必须是绝对路径：{}",
                directory.display()
            ));
        }
    }
    require_positive("[tools.file] max_bytes", tools.file.max_bytes)?;

    for host in &tools.http.allow_hosts {
        validate_allow_host(host)?;
    }
    require_positive("[tools.http] max_bytes", tools.http.max_bytes)
}

/// 权限表校验：未知工具名不会被静默忽略，否则「我明明配了 deny」会变成一句空话。
fn validate_permissions(tools: &ToolConfig) -> Result<()> {
    for name in tools.permissions.keys() {
        if !BUILTIN_TOOL_NAMES.contains(&name.as_str()) {
            return Err(anyhow!("[tools.permissions] 未知的工具名：{name}"));
        }
    }
    Ok(())
}

/// 白名单 host 校验：只接受 `host` 或 `host:port`，不写协议、端口与路径。
fn validate_allow_host(host: &str) -> Result<()> {
    let host = host.trim();
    if host.is_empty() {
        return Err(anyhow!("[tools.http] allow_hosts 不能包含空 host"));
    }
    if host.contains("://") || host.contains('/') {
        return Err(anyhow!("[tools.http] allow_hosts 只能写 host 或 host:port：{host}"));
    }
    Ok(())
}

/// 并发上限校验：绝对值与核数倍数都要求正数，且不超过 `Semaphore` 的可用上限（P2.1-015）。
///
/// 倍数的合法值只看倍数本身（真实核数装配时才确定）；最终许可数由装配层按真实核数
/// [`Concurrency::resolve`] 算出，乘积溢出在上限处饱和，不会 panic。
fn validate_max_concurrent_runs(value: Concurrency) -> Result<()> {
    let factor = value.factor();
    require_positive("[agent] max_concurrent_runs", factor as u64)?;
    if factor > Concurrency::MAX_PERMITS {
        return Err(anyhow!(
            "[agent] max_concurrent_runs 超过可用上限 {}（{}）",
            Concurrency::MAX_PERMITS,
            factor
        ));
    }
    Ok(())
}

/// 数值校验：只接受大于 0 的取值。
fn require_positive(field: &str, value: u64) -> Result<()> {
    if value == 0 {
        return Err(anyhow!("{field} 必须大于 0"));
    }
    Ok(())
}
