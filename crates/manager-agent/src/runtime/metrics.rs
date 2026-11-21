//! 运行时的进程内指标（P2.1-003）：断连清理链、会话停止、事件落盘、下行丢弃与会话锁表。
//!
//! **只在 `metrics` 特性下采集**：正式构建里本模块的所有函数都是空实现，不占静态内存、
//! 不做原子操作、不付日志判断的代价；`bin-tests` 与 `make test` 构建的 `bin-agent`
//! 打开该特性，运行结束后由测试报告输出一份快照。
//!
//! 阈值一律先比较再构造日志参数，热路径（每帧事件落盘、每次转发）只付几个原子加。

use std::time::Duration;

#[cfg(feature = "metrics")]
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
#[cfg(feature = "metrics")]
use std::time::Instant;
#[cfg(feature = "metrics")]
#[cfg(feature = "metrics")]
use tracing::warn;

/// 断连清理链单次耗时达到这个阈值记 `warn`：正常清理是毫秒级。
#[cfg(feature = "metrics")]
const CHAIN_WARN_MS: u128 = 500;
/// 会话停止（四个阶段合计）达到这个阈值记 `warn`：收尾等待会把占用 / 释放拖住。
#[cfg(feature = "metrics")]
const STOP_WARN_MS: u128 = 500;
/// 单帧事件落盘达到这个阈值记 `warn`：磁盘或写锁出了状况。
#[cfg(feature = "metrics")]
const APPEND_WARN_MS: u128 = 50;

/// 断连清理链指标。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChainMetrics {
    /// 处理链被唤醒执行的次数。
    pub runs: u64,
    /// 累计耗时（毫秒），含等 `CHAIN` 锁的时间。
    pub total_millis: u64,
    /// 单次最长耗时（毫秒）。
    pub max_millis: u64,
    /// 观察到的同时进入处理链最大值（全局串行时为 1，收窄为按客户端并行后会大于 1）。
    pub concurrent_max: i64,
    /// 累计清理的客户端数。
    pub clients: u64,
}

/// 会话停止指标：四个阶段各自的累计耗时。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StopMetrics {
    /// 停止次数。
    pub calls: u64,
    /// 累计总耗时（毫秒）。
    pub total_millis: u64,
    /// 单次最长总耗时（毫秒）。
    pub max_millis: u64,
    /// 数据库落终态的累计耗时（毫秒）。
    pub database_millis: u64,
    /// 执行队列收尾的累计耗时（毫秒）。
    pub executor_millis: u64,
    /// 写者收尾与 flush 的累计耗时（毫秒）。
    pub writer_millis: u64,
}

/// 事件落盘指标。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JournalMetrics {
    /// 落盘帧数。
    pub appends: u64,
    /// 累计写入耗时（毫秒）。
    pub total_millis: u64,
    /// 单帧最长写入耗时（毫秒）。
    pub max_millis: u64,
}

/// 会话锁表指标。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionLockMetrics {
    /// 最近一次取锁时表内条目数。
    pub entries: i64,
    /// 观察到的条目数最大值。
    pub entries_max: i64,
}

/// 运行时指标的全量快照。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetricsSnapshot {
    /// 断连清理链。
    pub chain: ChainMetrics,
    /// 会话停止。
    pub stop: StopMetrics,
    /// 事件落盘。
    pub journal: JournalMetrics,
    /// 下行通道满 / 已断导致事件被丢弃的次数。
    pub downlink_dropped: u64,
    /// 会话锁表。
    pub session_locks: SessionLockMetrics,
    /// 数据库取连接等待。
    pub database: DatabaseAcquire,
}

/// 取连接等待的统计：转写 `lib-db` 的快照，避免本模块的公开类型依赖 `lib-db`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DatabaseAcquire {
    /// 取连接次数。
    pub calls: u64,
    /// 累计等待耗时（毫秒）。
    pub total_millis: u64,
    /// 单次最长等待耗时（毫秒）。
    pub max_millis: u64,
}

// 断连清理链。
#[cfg(feature = "metrics")]
static CHAIN_RUNS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static CHAIN_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static CHAIN_MAX_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static CHAIN_NOW: AtomicI64 = AtomicI64::new(0);
#[cfg(feature = "metrics")]
static CHAIN_MAX: AtomicI64 = AtomicI64::new(0);
#[cfg(feature = "metrics")]
static CHAIN_CLIENTS: AtomicU64 = AtomicU64::new(0);
// 会话停止。
#[cfg(feature = "metrics")]
static STOP_CALLS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static STOP_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static STOP_MAX_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static STOP_DATABASE_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static STOP_EXECUTOR_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static STOP_WRITER_NANOS: AtomicU64 = AtomicU64::new(0);
// 事件落盘。
#[cfg(feature = "metrics")]
static APPEND_CALLS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static APPEND_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static APPEND_MAX_NANOS: AtomicU64 = AtomicU64::new(0);
// 下行丢弃与会话锁表。
#[cfg(feature = "metrics")]
static DOWNLINK_DROPPED: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static SESSION_LOCKS: AtomicI64 = AtomicI64::new(0);
#[cfg(feature = "metrics")]
static SESSION_LOCKS_MAX: AtomicI64 = AtomicI64::new(0);

/// 只读快照：调用方取一次即可，不重置。未启用采集时全部为 0。
pub fn snapshot() -> MetricsSnapshot {
    #[cfg(feature = "metrics")]
    {
        let database = lib_db::observability::snapshot();
        MetricsSnapshot {
            chain: ChainMetrics {
                runs: CHAIN_RUNS.load(Ordering::Relaxed),
                total_millis: millis(CHAIN_NANOS.load(Ordering::Relaxed)),
                max_millis: millis(CHAIN_MAX_NANOS.load(Ordering::Relaxed)),
                concurrent_max: CHAIN_MAX.load(Ordering::Relaxed),
                clients: CHAIN_CLIENTS.load(Ordering::Relaxed),
            },
            stop: StopMetrics {
                calls: STOP_CALLS.load(Ordering::Relaxed),
                total_millis: millis(STOP_NANOS.load(Ordering::Relaxed)),
                max_millis: millis(STOP_MAX_NANOS.load(Ordering::Relaxed)),
                database_millis: millis(STOP_DATABASE_NANOS.load(Ordering::Relaxed)),
                executor_millis: millis(STOP_EXECUTOR_NANOS.load(Ordering::Relaxed)),
                writer_millis: millis(STOP_WRITER_NANOS.load(Ordering::Relaxed)),
            },
            journal: JournalMetrics {
                appends: APPEND_CALLS.load(Ordering::Relaxed),
                total_millis: millis(APPEND_NANOS.load(Ordering::Relaxed)),
                max_millis: millis(APPEND_MAX_NANOS.load(Ordering::Relaxed)),
            },
            downlink_dropped: DOWNLINK_DROPPED.load(Ordering::Relaxed),
            session_locks: SessionLockMetrics {
                entries: SESSION_LOCKS.load(Ordering::Relaxed),
                entries_max: SESSION_LOCKS_MAX.load(Ordering::Relaxed),
            },
            database: DatabaseAcquire {
                calls: database.calls,
                total_millis: database.total_millis,
                max_millis: database.max_millis,
            },
        }
    }
    #[cfg(not(feature = "metrics"))]
    {
        MetricsSnapshot::default()
    }
}

/// 处理链的在场守卫：进入 +1、离开 -1，并累计耗时。
///
/// 用守卫而不是在函数末尾记账，是为了让异常路径（`?` 提前返回、panic 展开）也把水位回落。
#[cfg(feature = "metrics")]
pub(crate) struct ChainSpan {
    /// 进入时刻：包含等 `CHAIN` 锁的时间。
    started: Instant,
    /// 进入时的并发数：用于 `warn` 里判断是不是在排队。
    concurrent: i64,
}

/// 未启用采集时的占位守卫：没有需要回落的水位。
#[cfg(not(feature = "metrics"))]
pub(crate) struct ChainSpan;

#[cfg(feature = "metrics")]
impl Drop for ChainSpan {
    fn drop(&mut self) {
        CHAIN_NOW.fetch_sub(1, Ordering::Relaxed);
        let elapsed = self.started.elapsed();
        let nanos = to_nanos(elapsed);
        CHAIN_RUNS.fetch_add(1, Ordering::Relaxed);
        CHAIN_NANOS.fetch_add(nanos, Ordering::Relaxed);
        CHAIN_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
        if elapsed.as_millis() >= CHAIN_WARN_MS {
            warn!(
                elapsed_ms = elapsed.as_millis(),
                concurrent = self.concurrent,
                "断连清理链单次耗时过长"
            );
        }
    }
}

/// 进入处理链。
pub(crate) fn chain_enter() -> ChainSpan {
    #[cfg(feature = "metrics")]
    {
        let concurrent = CHAIN_NOW.fetch_add(1, Ordering::Relaxed) + 1;
        CHAIN_MAX.fetch_max(concurrent, Ordering::Relaxed);
        ChainSpan {
            started: Instant::now(),
            concurrent,
        }
    }
    #[cfg(not(feature = "metrics"))]
    ChainSpan
}

/// 记一次处理链清掉的客户端数。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub(crate) fn record_chain_clients(clients: usize) {
    #[cfg(feature = "metrics")]
    CHAIN_CLIENTS.fetch_add(clients as u64, Ordering::Relaxed);
}

/// 记一次会话停止：四个阶段分别累计，超过阈值打 `warn`。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub(crate) fn record_stop(session_id: &str, total: Duration, database: Duration, executor: Duration, writer: Duration) {
    #[cfg(feature = "metrics")]
    {
        let nanos = to_nanos(total);
        STOP_CALLS.fetch_add(1, Ordering::Relaxed);
        STOP_NANOS.fetch_add(nanos, Ordering::Relaxed);
        STOP_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
        STOP_DATABASE_NANOS.fetch_add(to_nanos(database), Ordering::Relaxed);
        STOP_EXECUTOR_NANOS.fetch_add(to_nanos(executor), Ordering::Relaxed);
        STOP_WRITER_NANOS.fetch_add(to_nanos(writer), Ordering::Relaxed);
        if total.as_millis() >= STOP_WARN_MS {
            warn!(
                session_id,
                total_ms = total.as_millis(),
                database_ms = database.as_millis(),
                executor_ms = executor.as_millis(),
                writer_ms = writer.as_millis(),
                "停止运行时会话耗时过长"
            );
        }
    }
}

/// 记一帧事件落盘：累计与最长写入耗时，超过阈值打 `warn`。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub(crate) fn record_append(elapsed: Duration, path: &std::path::Path) {
    #[cfg(feature = "metrics")]
    {
        let nanos = to_nanos(elapsed);
        APPEND_CALLS.fetch_add(1, Ordering::Relaxed);
        APPEND_NANOS.fetch_add(nanos, Ordering::Relaxed);
        APPEND_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
        if elapsed.as_millis() >= APPEND_WARN_MS {
            // 只有慢帧才构造路径文本，正常帧不付格式化开销。
            warn!(
                elapsed_ms = elapsed.as_millis(),
                file = %path.display(),
                "事件文件单帧写入过慢"
            );
        }
    }
}

/// 记一次下行事件被丢弃：通道已断或队列满（慢客户端），按「流已废」处理。
pub(crate) fn record_downlink_dropped() {
    #[cfg(feature = "metrics")]
    DOWNLINK_DROPPED.fetch_add(1, Ordering::Relaxed);
}

/// 记一次取会话锁时表内的条目数：锁表只增不减会在这里体现。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub(crate) fn record_session_locks(entries: usize) {
    #[cfg(feature = "metrics")]
    {
        let entries = entries as i64;
        SESSION_LOCKS.store(entries, Ordering::Relaxed);
        SESSION_LOCKS_MAX.fetch_max(entries, Ordering::Relaxed);
    }
}

/// 时长转纳秒：饱和到 `u64`，不 panic。
#[cfg(feature = "metrics")]
fn to_nanos(elapsed: Duration) -> u64 {
    elapsed.as_nanos().min(u64::MAX as u128) as u64
}

/// 纳秒转毫秒。
#[cfg(feature = "metrics")]
fn millis(nanos: u64) -> u64 {
    nanos / 1_000_000
}
