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
//! ```
//!
//! 配置只来自命令行与这个文件（应用根目录本身由 `AGENT_HOME` 决定），不为任何配置项读环境变量。
//!
//! 服务端与客户端共用同一份地址来源（D-012）：`agent server` 用它决定监听地址，
//! `agent tui` / `agent run` / `agent status` 用它决定要连哪个服务。
//!
//! `[provider]` 段可以整体缺省：此时视为「Provider 未配置」，服务端照常启动，首次调用模型才失败。
//! 段一旦存在，缺字段、未知 `kind` 与非法取值都是启动期硬错误（退出码 2）。
use crate::command::ServerArgs;
use anyhow::{Context, Result, anyhow};
use lib_core::directorys::directorys;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::str::FromStr;
use tracing_subscriber::filter::LevelFilter;
use types_core::config::{AgentConfig, Concurrency, ProviderConfig};
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

/// 从已解析的文件配置里取出并校验 Provider / Agent 段。
fn llm_from(file: &FileConfig) -> Result<LlmConfig> {
    if let Some(provider) = file.provider.as_ref() {
        validate_provider(provider)?;
    }
    validate_agent(&file.agent)?;
    Ok(LlmConfig {
        provider: file.provider.clone(),
        agent: file.agent.clone(),
    })
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
    require_positive("[agent] context_safety_tokens", agent.context_safety_tokens as u64)
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
