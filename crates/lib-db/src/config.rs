//! 连接参数：默认值与 D-P2-004 定稿的连接属性。

use std::time::Duration;

/// 连接池上限（D-P2-004）。
pub const DEFAULT_MAX_CONNECTIONS: u32 = 5;
/// 取连接的等待上限：池被占满时在这个时间内排队，超时按打开失败处理。
const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);
/// `busy_timeout` 默认值（毫秒）：WAL 下写锁争用只等待、不立即报 `SQLITE_BUSY`。
pub const DEFAULT_BUSY_TIMEOUT_MS: u64 = 5_000;

/// 数据库连接配置。
///
/// 只有两个可以调整的参数：连接池上限与 `busy_timeout`；`journal_mode=WAL` 与
/// `foreign_keys=ON` 是 D-P2-004 定稿的固定属性，不做可选化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DbConfig {
    /// 连接池上限。
    pub max_connections: u32,
    /// 写锁等待上限（毫秒）。
    pub busy_timeout_ms: u64,
}

impl DbConfig {
    /// 取连接的等待上限。
    pub(crate) fn acquire_timeout(&self) -> Duration {
        DEFAULT_ACQUIRE_TIMEOUT
    }
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            max_connections: DEFAULT_MAX_CONNECTIONS,
            busy_timeout_ms: DEFAULT_BUSY_TIMEOUT_MS,
        }
    }
}
