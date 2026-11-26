//! MCP 侧的进程内指标（P4-013 / D-P4-019）：连接、工具与调用的累计量。
//!
//! **只在 `metrics` 特性下采集**：正式构建里本模块的累加函数是空实现，不留静态计数器、
//! 不引入额外依赖（与 `manager-agent` 的既有口径一致）。快照由 `manager-agent` 的
//! `MetricsSnapshot` 转写，经 `api/probe/metrics` 读出去。
//!
//! 计数口径（README §4.10）：
//!
//! - `connects` / `connect_failures`：一次连接尝试与它的失败；
//! - `reconnects`：进入退避之后的重连尝试（首次连接不算）；
//! - `calls` / `call_errors` / `call_timeouts`：MCP 工具调用总数、失败数与被执行器超时打断的次数；
//! - `shutdown_kills`：关闭时未在优雅超时内收尾、被强制终止的连接数。
//!
//! 状态计数（`servers` / `ready` / `failed` / `disabled` / `tools`）**不做累加**：由装配层在
//! 取快照时从 manager 的现状读出后传进 [`snapshot`]。

#[cfg(feature = "metrics")]
use std::sync::atomic::{AtomicU64, Ordering};

/// 取快照时刻的 server 与工具现状（由 manager 提供，不做累加）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
pub struct McpStateCounts {
    /// 配置里的 server 总数（含 `disabled`）。
    pub servers: u64,
    /// `READY` 的 server 数。
    pub ready: u64,
    /// `FAILED` 的 server 数。
    pub failed: u64,
    /// `DISABLED` 的 server 数。
    pub disabled: u64,
    /// 已注册的 MCP 工具数。
    pub tools: u64,
}

/// `metrics` 快照的 `mcp` 段：状态现状 + 自进程启动的累计量。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
pub struct McpMetrics {
    /// 配置里的 server 总数（含 `disabled`）。
    pub servers: u64,
    /// `READY` 的 server 数。
    pub ready: u64,
    /// `FAILED` 的 server 数。
    pub failed: u64,
    /// `DISABLED` 的 server 数。
    pub disabled: u64,
    /// 已注册的 MCP 工具数。
    pub tools: u64,
    /// 连接尝试次数。
    pub connects: u64,
    /// 连接失败次数。
    pub connect_failures: u64,
    /// 自动重连尝试次数。
    pub reconnects: u64,
    /// MCP 工具调用总数。
    pub calls: u64,
    /// 调用失败数（含 server 返回 `isError`）。
    pub call_errors: u64,
    /// 被超时打断的调用数（由执行器的超时判定）。
    pub call_timeouts: u64,
    /// 关闭时被强制终止的连接数。
    pub shutdown_kills: u64,
}

// 全部计数器的静态落点：只在 `metrics` 特性下存在。
#[cfg(feature = "metrics")]
static CONNECTS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static CONNECT_FAILURES: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static RECONNECTS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static CALLS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static CALL_ERRORS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static CALL_TIMEOUTS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static SHUTDOWN_KILLS: AtomicU64 = AtomicU64::new(0);

/// 只读快照：状态现状来自调用方（manager），其余取累计值；未启用采集时全部为 0。
pub fn snapshot(state: McpStateCounts) -> McpMetrics {
    let counters = counters();
    McpMetrics {
        servers: state.servers,
        ready: state.ready,
        failed: state.failed,
        disabled: state.disabled,
        tools: state.tools,
        connects: counters.0,
        connect_failures: counters.1,
        reconnects: counters.2,
        calls: counters.3,
        call_errors: counters.4,
        call_timeouts: counters.5,
        shutdown_kills: counters.6,
    }
}

/// 累计量的读取：未启用采集时是一份全 0，不读任何静态。
fn counters() -> (u64, u64, u64, u64, u64, u64, u64) {
    #[cfg(feature = "metrics")]
    {
        (
            CONNECTS.load(Ordering::Relaxed),
            CONNECT_FAILURES.load(Ordering::Relaxed),
            RECONNECTS.load(Ordering::Relaxed),
            CALLS.load(Ordering::Relaxed),
            CALL_ERRORS.load(Ordering::Relaxed),
            CALL_TIMEOUTS.load(Ordering::Relaxed),
            SHUTDOWN_KILLS.load(Ordering::Relaxed),
        )
    }
    #[cfg(not(feature = "metrics"))]
    {
        (0, 0, 0, 0, 0, 0, 0)
    }
}

/// 记一次连接尝试。
pub(crate) fn record_connect() {
    #[cfg(feature = "metrics")]
    CONNECTS.fetch_add(1, Ordering::Relaxed);
}

/// 记一次连接失败。
pub(crate) fn record_connect_failure() {
    #[cfg(feature = "metrics")]
    CONNECT_FAILURES.fetch_add(1, Ordering::Relaxed);
}

/// 记一次自动重连尝试。
pub(crate) fn record_reconnect() {
    #[cfg(feature = "metrics")]
    RECONNECTS.fetch_add(1, Ordering::Relaxed);
}

/// 记一次 MCP 工具调用。
pub(crate) fn record_call() {
    #[cfg(feature = "metrics")]
    CALLS.fetch_add(1, Ordering::Relaxed);
}

/// 记一次调用失败（传输错误或 server 返回 `isError`）。
pub(crate) fn record_call_error() {
    #[cfg(feature = "metrics")]
    CALL_ERRORS.fetch_add(1, Ordering::Relaxed);
}

/// 记一次被超时打断的调用。
pub(crate) fn record_call_timeout() {
    #[cfg(feature = "metrics")]
    CALL_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
}

/// 记一次关闭期强制终止。
pub(crate) fn record_shutdown_kill() {
    #[cfg(feature = "metrics")]
    SHUTDOWN_KILLS.fetch_add(1, Ordering::Relaxed);
}
