//! 用例共用的小工具：临时目录与轮询等待。
//!
//! 只放与业务无关的基础动作，避免各领域用例重复实现同样的等待与清理逻辑。

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use lib_core::time;
use tokio::time::sleep;

/// 单元用例临时目录的根：与端到端用例的运行目录同级，便于统一清理。
const UNIT_ROOT: &str = "lingting-agent-tests/units";
/// 轮询等待的默认超时：写者任务与断连处理链都在后台跑，只等待它们跑完。
const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
/// 轮询间隔。
const WAIT_INTERVAL: Duration = Duration::from_millis(10);

/// 为用例创建独立临时目录（每次调用都不同，避免用例之间互相污染）。
pub fn temp_directory(name: &str) -> Result<PathBuf> {
    let directory = std::env::temp_dir()
        .join(UNIT_ROOT)
        .join(format!("{name}-{}", time::now_millis()));
    std::fs::create_dir_all(&directory).with_context(|| format!("创建临时目录失败：{}", directory.display()))?;
    Ok(directory)
}

/// 轮询等待条件成立；超时返回 `false`，由调用方给出可读断言。
pub async fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        if condition() {
            return true;
        }
        sleep(WAIT_INTERVAL).await;
    }
    condition()
}
