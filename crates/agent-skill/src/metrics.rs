//! Skill 侧的进程内指标（P5-011 / D-P5-014）：发现、加载与索引的累计量。
//!
//! **只在 `metrics` 特性下采集**：正式构建里本模块的累加函数是空实现，不留静态计数器、
//! 不引入额外依赖（与 `agent-mcp` 的既有口径一致）。快照由 `manager-agent` 的
//! `MetricsSnapshot` 转写，经 `api/probe/metrics` 读出去。
//!
//! 计数口径：
//!
//! - `discovered` / `invalid` / `enabled`：**现状**，由装配层在取快照时从注册表读出后传进
//!   [`snapshot`]，不做累加；
//! - `loads` / `load_failures`：注册表的每次加载与它的失败；
//! - `sections_served`：随成功加载交出的章节数（章节表长度）；
//! - `index_chars`：最近一次索引渲染的字符数（`store` 语义，不是累加）；
//! - `reloads`：整体替换快照的次数（启动期那一次也算）。
//!
//! 本阶段不提供「按名字的分布」：那需要无界标签集，不属于进程内指标的形态（D-P5-014）。

#[cfg(feature = "metrics")]
use std::sync::atomic::{AtomicU64, Ordering};

/// 取快照时刻的 Skill 现状（由注册表提供，不做累加）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
pub struct SkillStateCounts {
    /// 快照里的条目总数（含 `INVALID`）。
    pub discovered: u64,
    /// 判 `INVALID` 的条目数。
    pub invalid: u64,
    /// `READY && enabled` 的条目数。
    pub enabled: u64,
}

/// `metrics` 快照的 `skills` 段：状态现状 + 自进程启动的累计量。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
pub struct SkillMetrics {
    /// 快照里的条目总数（含 `INVALID`）。
    pub discovered: u64,
    /// 判 `INVALID` 的条目数。
    pub invalid: u64,
    /// `READY && enabled` 的条目数。
    pub enabled: u64,
    /// 加载次数（成功）。
    pub loads: u64,
    /// 加载失败次数（未知名字 / 禁用 / `INVALID`）。
    pub load_failures: u64,
    /// 随成功加载交出的章节数。
    pub sections_served: u64,
    /// 最近一次索引渲染的字符数。
    pub index_chars: u64,
    /// 快照整体替换的次数（含启动期那一次）。
    pub reloads: u64,
}

// 全部计数器的静态落点：只在 `metrics` 特性下存在。
#[cfg(feature = "metrics")]
static LOADS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static LOAD_FAILURES: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static SECTIONS_SERVED: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static INDEX_CHARS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static RELOADS: AtomicU64 = AtomicU64::new(0);

/// 只读快照：状态现状来自调用方（注册表），其余取累计值；未启用采集时全部为 0。
pub fn snapshot(state: SkillStateCounts) -> SkillMetrics {
    let counters = counters();
    SkillMetrics {
        discovered: state.discovered,
        invalid: state.invalid,
        enabled: state.enabled,
        loads: counters.0,
        load_failures: counters.1,
        sections_served: counters.2,
        index_chars: counters.3,
        reloads: counters.4,
    }
}

/// 累计量的读取：未启用采集时是一份全 0，不读任何静态。
fn counters() -> (u64, u64, u64, u64, u64) {
    #[cfg(feature = "metrics")]
    {
        (
            LOADS.load(Ordering::Relaxed),
            LOAD_FAILURES.load(Ordering::Relaxed),
            SECTIONS_SERVED.load(Ordering::Relaxed),
            INDEX_CHARS.load(Ordering::Relaxed),
            RELOADS.load(Ordering::Relaxed),
        )
    }
    #[cfg(not(feature = "metrics"))]
    {
        (0, 0, 0, 0, 0)
    }
}

/// 记一次成功加载，并累加交出的章节数。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub(crate) fn record_load(sections: usize) {
    #[cfg(feature = "metrics")]
    {
        LOADS.fetch_add(1, Ordering::Relaxed);
        SECTIONS_SERVED.fetch_add(u64::try_from(sections).unwrap_or(u64::MAX), Ordering::Relaxed);
    }
}

/// 记一次加载失败。
pub(crate) fn record_load_failure() {
    #[cfg(feature = "metrics")]
    LOAD_FAILURES.fetch_add(1, Ordering::Relaxed);
}

/// 记一次索引渲染的字符数（取最近一次，不是累加）。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub(crate) fn record_index_chars(chars: usize) {
    #[cfg(feature = "metrics")]
    INDEX_CHARS.store(u64::try_from(chars).unwrap_or(u64::MAX), Ordering::Relaxed);
}

/// 记一次快照整体替换。
pub(crate) fn record_reload() {
    #[cfg(feature = "metrics")]
    RELOADS.fetch_add(1, Ordering::Relaxed);
}
