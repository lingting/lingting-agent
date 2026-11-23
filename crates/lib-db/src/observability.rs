//! 取连接等待与写锁争用的观测（P2.1-003 / P2.2-003）。
//!
//! **只在 `metrics` 特性下采集**：正式构建里本模块的记账函数都是空实现。启用后只做内存累加
//! （供 `manager-agent` 的快照读出），不打任何阈值日志——数据只走结构化快照（P2.2-004）。

use std::time::Duration;

#[cfg(feature = "metrics")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "metrics")]
use std::time::Instant;

/// 取连接的累计统计（`Database::begin` 的等待时长）与写锁争用错误数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AcquireSnapshot {
    /// 取连接的次数。
    pub calls: u64,
    /// 累计等待耗时（毫秒）。
    pub total_millis: u64,
    /// 单次最长等待耗时（毫秒）。
    pub max_millis: u64,
    /// 写锁争用类错误（`database is locked` / `SQLITE_BUSY`）的次数。
    pub errors: u64,
}

/// 取连接次数。
#[cfg(feature = "metrics")]
static CALLS: AtomicU64 = AtomicU64::new(0);
/// 累计等待耗时（纳秒）。
#[cfg(feature = "metrics")]
static TOTAL_NANOS: AtomicU64 = AtomicU64::new(0);
/// 单次最长等待耗时（纳秒）。
#[cfg(feature = "metrics")]
static MAX_NANOS: AtomicU64 = AtomicU64::new(0);
/// 写锁争用类错误次数。
#[cfg(feature = "metrics")]
static ERRORS: AtomicU64 = AtomicU64::new(0);

/// 记一次取连接等待：只累加原子。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub fn record_acquire(elapsed: Duration) {
    #[cfg(feature = "metrics")]
    {
        let nanos = elapsed.as_nanos().min(u64::MAX as u128) as u64;
        CALLS.fetch_add(1, Ordering::Relaxed);
        TOTAL_NANOS.fetch_add(nanos, Ordering::Relaxed);
        MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
    }
}

/// 记一次数据库错误：只有写锁争用类错误才累加，其余错误不计入。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub(crate) fn record_error(error: &sqlx::Error) {
    #[cfg(feature = "metrics")]
    {
        if is_lock_contention(error) {
            ERRORS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// 写锁争用判定：主结果码 `SQLITE_BUSY`(5) / `SQLITE_LOCKED`(6)，或错误文案直达关键串。
#[cfg(feature = "metrics")]
fn is_lock_contention(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(database) = error else {
        return false;
    };
    matches!(database.code().as_deref(), Some("5") | Some("6"))
        || database.message().contains("database is locked")
        || database.message().contains("SQLITE_BUSY")
}

/// 只读快照：调用方取一次即可，不重置。未启用采集时全部为 0。
pub fn snapshot() -> AcquireSnapshot {
    #[cfg(feature = "metrics")]
    {
        AcquireSnapshot {
            calls: CALLS.load(Ordering::Relaxed),
            total_millis: TOTAL_NANOS.load(Ordering::Relaxed) / 1_000_000,
            max_millis: MAX_NANOS.load(Ordering::Relaxed) / 1_000_000,
            errors: ERRORS.load(Ordering::Relaxed),
        }
    }
    #[cfg(not(feature = "metrics"))]
    {
        AcquireSnapshot::default()
    }
}

/// 计时把手：取连接前调用 [`AcquireTimer::start`]，拿到连接后 [`AcquireTimer::finish`]。
///
/// 只有启用采集时才真的存时刻；未启用时是个零大小的占位。
#[derive(Debug, Clone, Copy)]
pub struct AcquireTimer {
    /// 起始时刻：未启用采集时不存在。
    #[cfg(feature = "metrics")]
    started: Instant,
}

impl AcquireTimer {
    /// 开始计时。
    pub fn start() -> Self {
        Self {
            #[cfg(feature = "metrics")]
            started: Instant::now(),
        }
    }

    /// 结束计时并记账。
    pub fn finish(self) {
        #[cfg(feature = "metrics")]
        record_acquire(self.started.elapsed());
    }
}
