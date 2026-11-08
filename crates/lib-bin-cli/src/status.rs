//! `agent status`：探活并打印服务地址、健康状态、版本与运行中的 Run 数量。
//!
//! 三种结论互斥：连接失败判定「服务未启动」；超时、非 2xx 与响应解析失败判定「服务异常」；
//! 其余为「正常」。探活走生成的 `probe_health`，客户端不出现任何 path 字面量。

use crate::client::{Client, ClientError};
use crate::command::StatusArgs;
use crate::config;
use crate::result::CommandError;
use crate::runtime;
use crate::sdk::ApiClient;
use anyhow::{Result, anyhow};
use lib_core::logger;

/// 执行 `agent status`。
pub fn execute(args: &StatusArgs) -> Result<(), CommandError> {
    let config = config::resolve_client(args.server.as_deref()).map_err(CommandError::argument)?;
    let guard = logger::init_cli(config.log_level).map_err(CommandError::runtime)?;
    let client = Client::new(config.base_url).map_err(CommandError::runtime)?;

    let outcome = runtime::block_on(execute_async(&client));
    guard.flush();
    outcome.map_err(CommandError::runtime)?.map_err(CommandError::runtime)
}

/// 探活并输出状态。
async fn execute_async(client: &Client) -> Result<()> {
    let health = match client.probe_health().await.map_err(ClientError::from_call) {
        Ok(health) => health,
        Err(ClientError::Connection { .. }) => {
            return Err(anyhow!("服务未启动：{} 无法连接", client.base_url()));
        }
        Err(error) => return Err(anyhow!("服务异常：{error}")),
    };

    println!("服务地址：{}", client.base_url());
    println!("健康状态：正常");
    println!("服务版本：{}（协议 {}）", health.version, health.protocol_version);
    println!("运行时长：{}", format_duration(health.uptime_ms));
    println!("运行中 Run：{}", health.running_run_count);
    Ok(())
}

/// 毫秒时长转可读文本，最小单位为秒。
fn format_duration(millis: i64) -> String {
    let seconds = millis / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    match (hours, minutes) {
        (0, 0) => format!("{seconds} 秒"),
        (0, minutes) => format!("{minutes} 分 {seconds} 秒"),
        (hours, minutes) => format!("{hours} 小时 {minutes} 分 {seconds} 秒"),
    }
}
