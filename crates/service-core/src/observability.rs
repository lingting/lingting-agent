//! 检查点与恢复的进程内计数（P6A-014）。
//!
//! **只在 `metrics` 特性下采集**：正式构建里本模块的记账函数都是空实现，不占静态内存、
//! 不做原子操作；启用后只做内存累加（供 `manager-agent` 的快照读出），不打任何阈值日志。
//!
//! 这里只记「存储层能自己判定的事实」：检查点写入成功、终态清理删掉的行数、`PAUSED -> RUNNING`
//! 真的推进了几次。判定与拒绝由 `agent-runtime` / `manager-agent` 各自的层记，渲染只在
//! `manager-agent` 的快照里发生一次（`docs/workspace.md` 4.3 的单点口径）。

#[cfg(feature = "metrics")]
use std::sync::atomic::{AtomicU64, Ordering};

/// 检查点与恢复推进的累计量。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckpointSnapshot {
    /// 成功写入的检查点条数。
    pub checkpoints_written: u64,
    /// 终态清理删掉的检查点条数。
    pub checkpoints_pruned: u64,
    /// `PAUSED -> RUNNING` 真正推进成功的次数（条件更新命中一次记一次）。
    pub resume_succeeded: u64,
    /// 成功写入的检查点载荷总字节数（P6A-018）：平均载荷 = 本值 / `checkpoints_written`。
    pub checkpoint_payload_bytes: u64,
}

/// 成功写入的检查点条数。
#[cfg(feature = "metrics")]
static WRITTEN: AtomicU64 = AtomicU64::new(0);
/// 终态清理删掉的条数。
#[cfg(feature = "metrics")]
static PRUNED: AtomicU64 = AtomicU64::new(0);
/// 恢复推进成功的次数。
#[cfg(feature = "metrics")]
static RESUME_SUCCEEDED: AtomicU64 = AtomicU64::new(0);
/// 成功写入的检查点载荷总字节数。
#[cfg(feature = "metrics")]
static PAYLOAD_BYTES: AtomicU64 = AtomicU64::new(0);

/// 记一条检查点写入成功：`payload_bytes` 是该条载荷的字节数（P6A-018 的平均载荷来源）。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub fn record_checkpoint_written(payload_bytes: usize) {
    #[cfg(feature = "metrics")]
    {
        WRITTEN.fetch_add(1, Ordering::Relaxed);
        PAYLOAD_BYTES.fetch_add(u64::try_from(payload_bytes).unwrap_or(u64::MAX), Ordering::Relaxed);
    }
}

/// 记一次终态清理：`removed` 是实际删除的行数（一条都没删也记 0）。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub fn record_checkpoints_pruned(removed: usize) {
    #[cfg(feature = "metrics")]
    PRUNED.fetch_add(u64::try_from(removed).unwrap_or(u64::MAX), Ordering::Relaxed);
}

/// 记一次恢复推进成功（`PAUSED -> RUNNING` 的条件更新命中）。
pub fn record_resume_succeeded() {
    #[cfg(feature = "metrics")]
    RESUME_SUCCEEDED.fetch_add(1, Ordering::Relaxed);
}

/// 只读快照：调用方取一次即可，不重置。未启用采集时全部为 0。
pub fn snapshot() -> CheckpointSnapshot {
    #[cfg(feature = "metrics")]
    {
        CheckpointSnapshot {
            checkpoints_written: WRITTEN.load(Ordering::Relaxed),
            checkpoints_pruned: PRUNED.load(Ordering::Relaxed),
            resume_succeeded: RESUME_SUCCEEDED.load(Ordering::Relaxed),
            checkpoint_payload_bytes: PAYLOAD_BYTES.load(Ordering::Relaxed),
        }
    }
    #[cfg(not(feature = "metrics"))]
    {
        CheckpointSnapshot::default()
    }
}
