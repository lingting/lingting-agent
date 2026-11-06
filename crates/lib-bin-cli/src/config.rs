//! Server 配置装载：命令行 > 环境变量 > 配置文件 > 默认值。
//!
//! 配置文件位于应用根目录下的 `config.toml`，形如：
//!
//! ```toml
//! [server]
//! addr = "127.0.0.1"
//! port = 20100
//! log_level = "info"
//! ```

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

/// 按优先级装载 Server 配置。
pub fn resolve_server(args: &ServerArgs) -> Result<ServerConfig> {
    let file = load_file()?;

    let cli_address = optional_address(args.addr.as_deref())?;
    let env_address = optional_address(env_value(ENV_HOST).as_deref())?;
    let file_address = optional_address(file.server.addr.as_deref())?;
    let env_port = match env_value(ENV_PORT) {
        None => None,
        Some(value) => Some(
            value
                .parse::<u16>()
                .map_err(|_| anyhow!("环境变量 {ENV_PORT} 不是有效端口：{value}"))?,
        ),
    };

    let host = cli_address
        .as_ref()
        .map(|address| address.host.clone())
        .or_else(|| env_address.as_ref().map(|address| address.host.clone()))
        .or_else(|| file_address.as_ref().map(|address| address.host.clone()))
        .unwrap_or_else(|| DEFAULT_HOST.to_owned());

    let port = args
        .port
        .or_else(|| cli_address.as_ref().and_then(|address| address.port))
        .or(env_port)
        .or_else(|| env_address.as_ref().and_then(|address| address.port))
        .or(file.server.port)
        .or_else(|| file_address.as_ref().and_then(|address| address.port))
        .unwrap_or(DEFAULT_PORT);

    if host.trim().is_empty() {
        return Err(anyhow!("监听地址不能为空"));
    }
    if port == 0 {
        return Err(anyhow!("监听端口不能为 0（0 会让服务端使用随机端口）"));
    }

    let env_level = env_value(ENV_LOG_LEVEL);
    let level = args
        .log_level
        .as_deref()
        .or(env_level.as_deref())
        .or(file.server.log_level.as_deref());
    let log_level = match level {
        Some(level) => LevelFilter::from_str(&level.trim().to_ascii_lowercase())
            .map_err(|_| anyhow!("日志级别无效：{level}（可选 off / error / warn / info / debug / trace）"))?,
        None => LevelFilter::INFO,
    };

    Ok(ServerConfig { host, port, log_level })
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
        return Err(anyhow!("监听地址不能为空：{value}"));
    }

    let port = port
        .map(|port| port.parse::<u16>().map_err(|_| anyhow!("监听端口无效：{port}")))
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
