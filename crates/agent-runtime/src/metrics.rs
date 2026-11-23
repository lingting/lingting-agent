//! Run 排队与执行并发度的进程内指标（P2.1-003）。
//!
//! **只在 `metrics` 特性下采集**：正式构建里本模块的所有函数都是空实现，不占静态内存、
//! 不做原子操作、不付日志判断的代价；`bin-tests` 与 `make test` 构建的 `bin-agent`
//! 打开该特性，运行结束后由测试报告输出一份快照。
//!
//! 采集只做内存累加：数据只经结构化快照读出去，不再打任何阈值日志（P2.2-004）。

#[cfg(feature = "metrics")]
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Duration;

/// 进入排队的次数。
#[cfg(feature = "metrics")]
static QUEUED: AtomicU64 = AtomicU64::new(0);
/// 排队成功后累计等待耗时（纳秒）。
#[cfg(feature = "metrics")]
static WAIT_NANOS: AtomicU64 = AtomicU64::new(0);
/// 单次最长排队耗时（纳秒）。
#[cfg(feature = "metrics")]
static WAIT_MAX_NANOS: AtomicU64 = AtomicU64::new(0);
/// 排队超时的次数。
#[cfg(feature = "metrics")]
static TIMEOUTS: AtomicU64 = AtomicU64::new(0);
/// 排队期间被取消的次数。
#[cfg(feature = "metrics")]
static CANCELLED: AtomicU64 = AtomicU64::new(0);
/// 当前正在排队（等许可）的 Run 数。
#[cfg(feature = "metrics")]
static QUEUED_NOW: AtomicI64 = AtomicI64::new(0);
/// 排队水位：观察到的同时在排队最大值。
#[cfg(feature = "metrics")]
static QUEUED_MAX: AtomicI64 = AtomicI64::new(0);
/// 当前持有许可（执行中）的 Run 数。
#[cfg(feature = "metrics")]
static RUNNING_NOW: AtomicI64 = AtomicI64::new(0);
/// 执行水位：观察到的同时在执行最大值。
#[cfg(feature = "metrics")]
static RUNNING_MAX: AtomicI64 = AtomicI64::new(0);

/// Run 排队与执行的只读快照。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetricsSnapshot {
    /// 进入过排队的 Run 数。
    pub queued: u64,
    /// 排队超时的 Run 数。
    pub timeouts: u64,
    /// 排队期间被取消的 Run 数。
    pub cancelled: u64,
    /// 排队成功后的累计等待耗时（毫秒）。
    pub wait_total_millis: u64,
    /// 单次最长排队耗时（毫秒）。
    pub wait_max_millis: u64,
    /// 观察到的同时在排队最大值。
    pub queued_max: i64,
    /// 观察到的同时在执行最大值。
    pub running_max: i64,
    /// 读取瞬间正在执行的 Run 数。
    pub running_now: i64,
}

/// 只读快照：调用方取一次即可，不重置。未启用采集时全部为 0。
pub fn snapshot() -> MetricsSnapshot {
    #[cfg(feature = "metrics")]
    {
        MetricsSnapshot {
            queued: QUEUED.load(Ordering::Relaxed),
            timeouts: TIMEOUTS.load(Ordering::Relaxed),
            cancelled: CANCELLED.load(Ordering::Relaxed),
            wait_total_millis: millis(WAIT_NANOS.load(Ordering::Relaxed)),
            wait_max_millis: millis(WAIT_MAX_NANOS.load(Ordering::Relaxed)),
            queued_max: QUEUED_MAX.load(Ordering::Relaxed),
            running_max: RUNNING_MAX.load(Ordering::Relaxed),
            running_now: RUNNING_NOW.load(Ordering::Relaxed),
        }
    }
    #[cfg(not(feature = "metrics"))]
    {
        MetricsSnapshot::default()
    }
}

/// 进入排队：计数 + 水位。
pub(crate) fn queued_enter() {
    #[cfg(feature = "metrics")]
    {
        QUEUED.fetch_add(1, Ordering::Relaxed);
        let now = QUEUED_NOW.fetch_add(1, Ordering::Relaxed) + 1;
        QUEUED_MAX.fetch_max(now, Ordering::Relaxed);
    }
}

/// 离开排队：无论是拿到许可、超时还是被取消，都要退出排队水位。
pub(crate) fn queued_leave() {
    #[cfg(feature = "metrics")]
    QUEUED_NOW.fetch_sub(1, Ordering::Relaxed);
}

/// 记一次排队成功：累计与最长等待。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub(crate) fn record_acquired_wait(elapsed: Duration) {
    #[cfg(feature = "metrics")]
    {
        let nanos = to_nanos(elapsed);
        WAIT_NANOS.fetch_add(nanos, Ordering::Relaxed);
        WAIT_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
    }
}

/// 排队超时：Run 落 `FAILED`（「执行排队超时」）。
pub(crate) fn record_timeout() {
    #[cfg(feature = "metrics")]
    TIMEOUTS.fetch_add(1, Ordering::Relaxed);
}

/// 排队期间被取消：不发终态事件之外的事件，也不落失败。
pub(crate) fn record_cancelled() {
    #[cfg(feature = "metrics")]
    CANCELLED.fetch_add(1, Ordering::Relaxed);
}

/// 开始占用一个并发位（拿到许可）：执行水位 +1。
pub(crate) fn run_started() {
    #[cfg(feature = "metrics")]
    {
        let now = RUNNING_NOW.fetch_add(1, Ordering::Relaxed) + 1;
        RUNNING_MAX.fetch_max(now, Ordering::Relaxed);
    }
}

/// 结束占用一个并发位（许可释放）：执行水位 -1。
pub(crate) fn run_finished() {
    #[cfg(feature = "metrics")]
    RUNNING_NOW.fetch_sub(1, Ordering::Relaxed);
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
