//! 取连接等待的观测（P2.1-003）。
//!
//! **只在 `metrics` 特性下采集**：正式构建里 [`record_acquire`] 是空实现，本 Crate 也不编
//! `tracing` 依赖。启用后本模块做两件事：内存累加（供 `manager-agent` 的快照读出）与
//! 阈值日志（先比较再构造参数，取连接通常亚毫秒，正常路径不付格式化开销）。

use std::time::Duration;

#[cfg(feature = "metrics")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "metrics")]
use std::time::Instant;
#[cfg(feature = "metrics")]
use tracing::{debug, warn};

/// 取连接等待达到这个阈值记 `debug`：连接池够用时不会出现。
#[cfg(feature = "metrics")]
const ACQUIRE_DEBUG_MS: u128 = 100;
/// 取连接等待达到这个阈值记 `warn`：连接池已饱和或事务过长。
#[cfg(feature = "metrics")]
const ACQUIRE_WARN_MS: u128 = 1000;

/// 取连接的累计统计（`Database::begin` 的等待时长）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AcquireSnapshot {
    /// 取连接的次数。
    pub calls: u64,
    /// 累计等待耗时（毫秒）。
    pub total_millis: u64,
    /// 单次最长等待耗时（毫秒）。
    pub max_millis: u64,
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

/// 记一次取连接等待：只累加 + 按阈值打日志，不做无谓的格式化。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub fn record_acquire(elapsed: Duration) {
    #[cfg(feature = "metrics")]
    {
        let nanos = elapsed.as_nanos().min(u64::MAX as u128) as u64;
        CALLS.fetch_add(1, Ordering::Relaxed);
        TOTAL_NANOS.fetch_add(nanos, Ordering::Relaxed);
        MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
        let waited_ms = elapsed.as_millis();
        if waited_ms >= ACQUIRE_WARN_MS {
            warn!(waited_ms, "取数据库连接过久，连接池已饱和");
        } else if waited_ms >= ACQUIRE_DEBUG_MS {
            debug!(waited_ms, "取数据库连接等待超过阈值");
        }
    }
}

/// 只读快照：调用方取一次即可，不重置。未启用采集时全部为 0。
pub fn snapshot() -> AcquireSnapshot {
    #[cfg(feature = "metrics")]
    {
        AcquireSnapshot {
            calls: CALLS.load(Ordering::Relaxed),
            total_millis: TOTAL_NANOS.load(Ordering::Relaxed) / 1_000_000,
            max_millis: MAX_NANOS.load(Ordering::Relaxed) / 1_000_000,
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
