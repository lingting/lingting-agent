//! 配置装载：命令行 > 环境变量 > 配置文件 > 默认值。
//!
//! 配置文件位于应用根目录下的 `config.toml`，形如：
//!
//! ```toml
//! [server]
//! addr = "127.0.0.1"
//! port = 20100
//! log_level = "info"
//! ```
//!
//! 服务端与客户端共用同一份地址来源（D-012）：`agent server` 用它决定监听地址，
//! `agent tui` / `agent run` / `agent status` 用它决定要连哪个服务。

use crate::command::ServerArgs;
use anyhow::{Context, Result, anyhow};
use lib_core::directorys::directorys;
use serde::Deserialize;
use std::env;
use std::fs;
use std::io;
use std::str::FromStr;
use tracing_subscriber::filter::LevelFilter;

/// 默认监听地址。
const DEFAULT_HOST: &str = "127.0.0.1";
/// 默认监听端口（固定端口，见 D-022）。
const DEFAULT_PORT: u16 = 20100;
/// 监听地址环境变量。
const ENV_HOST: &str = "AGENT_SERVER_ADDR";
/// 监听端口环境变量。
const ENV_PORT: &str = "AGENT_SERVER_PORT";
/// 日志级别环境变量。
const ENV_LOG_LEVEL: &str = "AGENT_LOG_LEVEL";

/// Server 运行配置。
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// 监听地址。
    pub host: String,
    /// 监听端口。
    pub port: u16,
    /// 日志级别。
    pub log_level: LevelFilter,
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

/// 地址的四个来源层级。
///
/// `cli_port` 只有 `server --port` 会传入，客户端命令没有独立的端口参数。
struct AddressSources {
    cli: Option<Address>,
    cli_port: Option<u16>,
    env: Option<Address>,
    env_port: Option<u16>,
    file: Option<Address>,
    file_port: Option<u16>,
}

impl AddressSources {
    /// 读取环境变量与配置文件，与命令行参数一起组成四个层级。
    fn load(cli: Option<&str>, cli_port: Option<u16>, file: &FileConfig) -> Result<Self> {
        Ok(Self {
            cli: optional_address(cli)?,
            cli_port,
            env: optional_address(env_value(ENV_HOST).as_deref())?,
            env_port: match env_value(ENV_PORT) {
                None => None,
                Some(value) => Some(
                    value
                        .parse::<u16>()
                        .map_err(|_| anyhow!("环境变量 {ENV_PORT} 不是有效端口：{value}"))?,
                ),
            },
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
            .or_else(|| self.env.as_ref().map(|address| address.host.clone()))
            .or_else(|| self.file.as_ref().map(|address| address.host.clone()))
            .unwrap_or_else(|| DEFAULT_HOST.to_owned());

        let port = self
            .cli_port
            .or_else(|| self.cli.as_ref().and_then(|address| address.port))
            .or(self.env_port)
            .or_else(|| self.env.as_ref().and_then(|address| address.port))
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

/// 日志级别：命令行 > 环境变量 > 配置文件 > 默认 `info`。
fn resolve_level(cli: Option<&str>, file: &FileConfig) -> Result<LevelFilter> {
    let env_level = env_value(ENV_LOG_LEVEL);
    let level = cli.or(env_level.as_deref()).or(file.server.log_level.as_deref());
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

fn env_value(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
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
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileServer {
    addr: Option<String>,
    port: Option<u16>,
    log_level: Option<String>,
}
