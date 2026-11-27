//! 运行时的进程内指标（P2.1-003 / P2.2-001）：清理链、会话停止、事件落盘、下行丢弃、锁表、
//! 限流与日志审计。
//!
//! 本模块是**唯一聚合 DTO**：各层各自在自己的 Crate 里累加原子量，这里只做转写与聚合，
//! 子进程与测试进程读的是同一个 [`MetricsSnapshot`]（P2.2-001）。
//!
//! **只在 `metrics` 特性下采集**：正式构建里本模块的所有函数都是空实现，不占静态内存、
//! 不做原子操作；`bin-tests` 与 `make test` 构建的 `bin-agent` 打开该特性，运行结束后由
//! `GET api/probe/metrics` 输出一份快照（P2.2-002）。
//!
//! **不打阈值日志**：数据只经快照读出去，采集型日志已按 D-P2.2-010 全部删除（P2.2-004）。

use std::time::Duration;

#[cfg(feature = "metrics")]
use std::sync::OnceLock;
#[cfg(feature = "metrics")]
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
#[cfg(feature = "metrics")]
use std::time::Instant;

/// 断连清理链指标。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
pub struct ChainMetrics {
    /// 按客户端执行的清理次数（P2.1-005 收窄串行单位后，每个客户端记一次）。
    pub runs: u64,
    /// 累计耗时（毫秒），含等 `CHAIN` 锁的时间。
    pub total_millis: u64,
    /// 单次最长耗时（毫秒）。
    pub max_millis: u64,
    /// 观察到的同时进入处理链最大值（全局串行时恒为 1，收窄为按客户端并行后会出现大于 1）。
    pub concurrent_max: i64,
    /// 累计清理的客户端数。
    pub clients: u64,
}

/// 会话停止指标：四个阶段各自的累计耗时。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
pub struct SessionLockMetrics {
    /// 最近一次取锁时表内条目数。
    pub entries: i64,
    /// 观察到的条目数最大值。
    pub entries_max: i64,
}

/// 会话锁持有时间指标（P2.1-006）：只覆盖临界区，不含锁外的收尾等待。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
pub struct SessionLockHold {
    /// 进入临界区的次数。
    pub calls: u64,
    /// 累计持有时间（毫秒）。
    pub total_millis: u64,
    /// 单次最长持有时间（毫秒）。
    pub max_millis: u64,
}

/// 注册表整表锁指标（P2.1-008.1）：判断整表 `RwLock` 是否在热路径上制造排队。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
pub struct RegistryLockMetrics {
    /// 取读锁次数。
    pub reads: u64,
    /// 读锁累计等待耗时（毫秒）。
    pub read_wait_millis: u64,
    /// 读锁单次最长等待耗时（毫秒）。
    pub read_wait_max_millis: u64,
    /// 取写锁次数；每转发一条下行事件至少一次。
    pub writes: u64,
    /// 写锁累计等待耗时（毫秒）。
    pub write_wait_millis: u64,
    /// 写锁单次最长等待耗时（毫秒）。
    pub write_wait_max_millis: u64,
    /// 下行转发次数。
    pub forwards: u64,
    /// 单次转发的临界区最长耗时（毫秒，含取写锁与投递）。
    pub forward_max_millis: u64,
}

/// 会话锁取锁等待指标（P2.1-008.1）：只覆盖等锁；锁内持有时间见 [`SessionLockHold`]。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
pub struct SessionLockWait {
    /// 取锁次数。
    pub calls: u64,
    /// 累计等待耗时（毫秒）。
    pub total_millis: u64,
    /// 单次最长等待耗时（毫秒）。
    pub max_millis: u64,
}

/// 取连接等待与写锁争用的统计：转写 `lib-db` 的快照，避免本模块的公开类型依赖 `lib-db`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
pub struct DatabaseAcquire {
    /// 取连接次数。
    pub calls: u64,
    /// 累计等待耗时（毫秒）。
    pub total_millis: u64,
    /// 单次最长等待耗时（毫秒）。
    pub max_millis: u64,
    /// 写锁争用类错误（`database is locked` / `SQLITE_BUSY`）次数。
    pub errors: u64,
}

/// Run 排队与执行的统计：转写 `agent-runtime` 的快照，避免本模块的公开类型依赖 `agent-runtime`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
pub struct LimitMetrics {
    /// 进入过排队的 Run 数（含排队超时与被取消）。
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

/// 日志审计的统计：转写 `lib-core` 的审计快照，把「日志里发生了什么」变成可断言的数据。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
pub struct AuditMetrics {
    /// 事件总数：`0` 说明日志根本没有产出。
    pub total: u64,
    /// `ERROR` 级事件数。
    pub error: u64,
    /// `WARN` 级事件数。
    pub warn: u64,
    /// 敏感串（如 `[provider] api_key`）在事件字段里出现的次数。
    pub secret_hits: u64,
    /// 进程内 panic 次数。
    pub panics: u64,
}

/// 运行时指标的全量快照。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "metrics", derive(serde::Serialize, serde::Deserialize))]
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
    /// 会话锁持有时间（P2.1-006）。
    pub session_lock_hold: SessionLockHold,
    /// 注册表整表锁（P2.1-008.1）。
    pub registry_lock: RegistryLockMetrics,
    /// 会话锁取锁等待（P2.1-008.1）。
    pub session_lock_wait: SessionLockWait,
    /// 数据库取连接等待与写锁争用。
    pub database: DatabaseAcquire,
    /// Run 排队与执行（P2.2-001）。
    pub limit: LimitMetrics,
    /// 日志审计（P2.2-003）。
    pub audit: AuditMetrics,
    /// MCP 的连接、工具与调用（P4-013）：状态现状取自 manager，累计量取自 `agent-mcp`。
    pub mcp: agent_mcp::metrics::McpMetrics,
    /// Skill 的发现、加载与索引（P5-011）：状态现状取自 manager，累计量取自 `agent-skill`。
    pub skills: agent_skill::metrics::SkillMetrics,
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
#[cfg(feature = "metrics")]
static LOCK_CALLS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static LOCK_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static LOCK_MAX_NANOS: AtomicU64 = AtomicU64::new(0);
// 注册表整表锁、下行转发与会话锁取锁等待（P2.1-008.1）。
#[cfg(feature = "metrics")]
static REG_READS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static REG_READ_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static REG_READ_MAX_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static REG_WRITES: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static REG_WRITE_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static REG_WRITE_MAX_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static FORWARDS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static FORWARD_MAX_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static WAIT_CALLS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static WAIT_NANOS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "metrics")]
static WAIT_MAX_NANOS: AtomicU64 = AtomicU64::new(0);

/// 限流快照的来源：由装配层把 `agent-runtime` 的快照转写进来（本 Crate 不依赖 `agent-runtime`）。
#[cfg(feature = "metrics")]
static LIMIT_PROVIDER: OnceLock<fn() -> LimitMetrics> = OnceLock::new();

/// 注册限流快照的来源（P2.2-001）：装配层启动时调用一次。
///
/// 重复注册返回 `false` 并保留第一次的来源；未启用采集时总是返回 `false`。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub fn install_limit_provider(provider: fn() -> LimitMetrics) -> bool {
    #[cfg(feature = "metrics")]
    {
        LIMIT_PROVIDER.set(provider).is_ok()
    }
    #[cfg(not(feature = "metrics"))]
    {
        false
    }
}

/// 只读快照：调用方取一次即可，不重置。未启用采集时全部为 0。
pub fn snapshot() -> MetricsSnapshot {
    #[cfg(feature = "metrics")]
    {
        let database = lib_db::observability::snapshot();
        let audit = lib_core::audit::snapshot();
        let limit = LIMIT_PROVIDER.get().map(|provider| provider()).unwrap_or_default();
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
            session_lock_hold: SessionLockHold {
                calls: LOCK_CALLS.load(Ordering::Relaxed),
                total_millis: millis(LOCK_NANOS.load(Ordering::Relaxed)),
                max_millis: millis(LOCK_MAX_NANOS.load(Ordering::Relaxed)),
            },
            registry_lock: RegistryLockMetrics {
                reads: REG_READS.load(Ordering::Relaxed),
                read_wait_millis: millis(REG_READ_NANOS.load(Ordering::Relaxed)),
                read_wait_max_millis: millis(REG_READ_MAX_NANOS.load(Ordering::Relaxed)),
                writes: REG_WRITES.load(Ordering::Relaxed),
                write_wait_millis: millis(REG_WRITE_NANOS.load(Ordering::Relaxed)),
                write_wait_max_millis: millis(REG_WRITE_MAX_NANOS.load(Ordering::Relaxed)),
                forwards: FORWARDS.load(Ordering::Relaxed),
                forward_max_millis: millis(FORWARD_MAX_NANOS.load(Ordering::Relaxed)),
            },
            session_lock_wait: SessionLockWait {
                calls: WAIT_CALLS.load(Ordering::Relaxed),
                total_millis: millis(WAIT_NANOS.load(Ordering::Relaxed)),
                max_millis: millis(WAIT_MAX_NANOS.load(Ordering::Relaxed)),
            },
            database: DatabaseAcquire {
                calls: database.calls,
                total_millis: database.total_millis,
                max_millis: database.max_millis,
                errors: database.errors,
            },
            limit,
            audit: AuditMetrics {
                total: audit.total,
                error: audit.error,
                warn: audit.warn,
                secret_hits: audit.secret_hits,
                panics: audit.panics,
            },
            mcp: mcp_snapshot(),
            skills: skills_snapshot(),
        }
    }
    #[cfg(not(feature = "metrics"))]
    {
        MetricsSnapshot::default()
    }
}

/// MCP 段的转写：累计量来自 `agent-mcp` 的原子量，状态计数取 manager 的现状（不做累加）。
#[cfg(feature = "metrics")]
fn mcp_snapshot() -> agent_mcp::metrics::McpMetrics {
    let counts = crate::holder::holder().map_or_else(agent_mcp::metrics::McpStateCounts::default, |installed| {
        installed.mcp().state_counts()
    });
    agent_mcp::metrics::snapshot(counts)
}

/// Skill 段的转写：累计量来自 `agent-skill` 的原子量，状态计数取注册表的现状（不做累加）。
#[cfg(feature = "metrics")]
fn skills_snapshot() -> agent_skill::metrics::SkillMetrics {
    let counts = crate::holder::holder().map_or_else(agent_skill::metrics::SkillStateCounts::default, |installed| {
        installed.skills().state_counts()
    });
    agent_skill::metrics::snapshot(counts)
}

/// 处理链的在场守卫：进入 +1、离开 -1，并累计耗时。
///
/// 用守卫而不是在函数末尾记账，是为了让异常路径（`?` 提前返回、panic 展开）也把水位回落。
#[cfg(feature = "metrics")]
pub(crate) struct ChainSpan {
    /// 进入时刻：包含等 `CHAIN` 锁的时间。
    started: Instant,
}

/// 未启用采集时的占位守卫：没有需要回落的水位。
#[cfg(not(feature = "metrics"))]
pub(crate) struct ChainSpan;

#[cfg(feature = "metrics")]
impl Drop for ChainSpan {
    fn drop(&mut self) {
        CHAIN_NOW.fetch_sub(1, Ordering::Relaxed);
        let nanos = to_nanos(self.started.elapsed());
        CHAIN_RUNS.fetch_add(1, Ordering::Relaxed);
        CHAIN_NANOS.fetch_add(nanos, Ordering::Relaxed);
        CHAIN_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
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

/// 记一次会话停止：四个阶段分别累计。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub(crate) fn record_stop(total: Duration, database: Duration, executor: Duration, writer: Duration) {
    #[cfg(feature = "metrics")]
    {
        let nanos = to_nanos(total);
        STOP_CALLS.fetch_add(1, Ordering::Relaxed);
        STOP_NANOS.fetch_add(nanos, Ordering::Relaxed);
        STOP_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
        STOP_DATABASE_NANOS.fetch_add(to_nanos(database), Ordering::Relaxed);
        STOP_EXECUTOR_NANOS.fetch_add(to_nanos(executor), Ordering::Relaxed);
        STOP_WRITER_NANOS.fetch_add(to_nanos(writer), Ordering::Relaxed);
    }
}

/// 记一帧事件落盘：累计与最长写入耗时。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub(crate) fn record_append(elapsed: Duration) {
    #[cfg(feature = "metrics")]
    {
        let nanos = to_nanos(elapsed);
        APPEND_CALLS.fetch_add(1, Ordering::Relaxed);
        APPEND_NANOS.fetch_add(nanos, Ordering::Relaxed);
        APPEND_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
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

/// 记一次会话锁持有时间（P2.1-006）：锁内只做状态切换。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub(crate) fn record_session_lock_hold(elapsed: Duration) {
    #[cfg(feature = "metrics")]
    {
        let nanos = to_nanos(elapsed);
        LOCK_CALLS.fetch_add(1, Ordering::Relaxed);
        LOCK_NANOS.fetch_add(nanos, Ordering::Relaxed);
        LOCK_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
    }
}

/// 记一次注册表读锁的等待时长：只算等锁，不含临界区。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables, dead_code))]
pub(crate) fn record_registry_read(wait: Duration) {
    #[cfg(feature = "metrics")]
    {
        let nanos = to_nanos(wait);
        REG_READS.fetch_add(1, Ordering::Relaxed);
        REG_READ_NANOS.fetch_add(nanos, Ordering::Relaxed);
        REG_READ_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
    }
}

/// 记一次注册表写锁的等待时长：只算等锁，不含临界区。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables, dead_code))]
pub(crate) fn record_registry_write(wait: Duration) {
    #[cfg(feature = "metrics")]
    {
        let nanos = to_nanos(wait);
        REG_WRITES.fetch_add(1, Ordering::Relaxed);
        REG_WRITE_NANOS.fetch_add(nanos, Ordering::Relaxed);
        REG_WRITE_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
    }
}

/// 记一次下行转发：从取写锁到投递完成的整段，是「每帧一条事件」的临界区。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables, dead_code))]
pub(crate) fn record_downlink_forward(elapsed: Duration) {
    #[cfg(feature = "metrics")]
    {
        let nanos = to_nanos(elapsed);
        FORWARDS.fetch_add(1, Ordering::Relaxed);
        FORWARD_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
    }
}

/// 记一次会话锁取锁等待：只算等锁，锁内持有时间另见 [`record_session_lock_hold`]。
#[cfg_attr(not(feature = "metrics"), allow(unused_variables, dead_code))]
pub(crate) fn record_session_lock_wait(elapsed: Duration) {
    #[cfg(feature = "metrics")]
    {
        let nanos = to_nanos(elapsed);
        WAIT_CALLS.fetch_add(1, Ordering::Relaxed);
        WAIT_NANOS.fetch_add(nanos, Ordering::Relaxed);
        WAIT_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
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
