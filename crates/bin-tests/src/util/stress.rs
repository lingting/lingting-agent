//! 边界压力用例的纯计算（P2.2-008）：等待预算、客户端数量公式、耗时分布与死线预算。
//!
//! 本模块不认识服务端、HTTP 或日志：输入是测出来的 `Duration` 与客户端数，输出直接进报告。
//! 阈值与公式常量全部集中在这里，用例文件里不出现数字。
//!
//! # 为什么是这条公式（D-P2.2-009）
//!
//! 服务端唯一可配置的并行度是 `[agent] max_concurrent_runs`；把它压到 [`SERIAL_RUNS`] = 1 之后，
//! Run 完全串行，第 `k` 个 Run 大约要等前面 `k - 1` 个 Run 各跑完一次，即 `(k - 1) × L0`。
//!
//! 排队超时（`[agent] run_queue_timeout_ms` = [`QUEUE_BUDGET`]）只看「等全局许可」这一段
//! （`agent-runtime/src/limit.rs`），因此第 `k` 个 Run 超时当且仅当 `(k - 1) × L0 > T`，
//! 即 `k > T / L0 + 1`。于是：
//!
//! ```text
//! 不会失败的最大客户端数  N_safe = floor(T / L0) + 1
//! 第一个必然失败的客户端数 N_bad  = floor(T / L0) + 2
//! ```
//!
//! 边界就摆在这两个数之间：`N_safe` 个客户端必须全绿，`2 × N_safe` 个客户端必须出现排队超时，
//! 两条一组合才能说明「卡在不失败的门槛上」，而不是「数字取得小所以没出事」。
//!
//! `L0` 由用例每次标定得出（先跑 [`WARMUP_ROUNDS`] 轮预热丢弃冷启动，再对 [`CALIBRATION_ROUNDS`]
//! 次「发消息 → 终态」的串行段取**最大值**，保守估计），
//! 因此 `N` 每次都重算，用例文件里不出现固化的客户端数量（G-1-3）。
//!
//! 示例（注释，不是断言）：`L0 = 10ms、T = 3s` → `N = 301`；`L0 = 30ms、T = 3s` → `N = 101`。
//!
//! # 用例自己的死线
//!
//! `runner` 给每个用例的是固定的调度超时（`CASE_TIMEOUT`）。三段各有自己的死线，
//! [`budget_upper_bound`] 把它们加起来给出上界，用例据此在开跑前核对「会不会被调度层杀掉」。

use std::time::Duration;

/// 并行度压到的值：`[agent] max_concurrent_runs`，1 表示 Run 完全串行、排队时长可线性推算。
pub(crate) const SERIAL_RUNS: usize = 1;

/// 等待预算 `T`：用例配置里的 `[agent] run_queue_timeout_ms`，等全局许可超过它就落排队超时。
pub(crate) const QUEUE_BUDGET: Duration = Duration::from_secs(3);

/// 预热轮数：这几轮只付冷启动（建库、建立 Provider 连接），样本丢弃。
pub(crate) const WARMUP_ROUNDS: usize = 1;

/// 标定轮数：`L0` 取这几轮的最大值。
pub(crate) const CALIBRATION_ROUNDS: usize = 3;

/// 形成边界所需的最小客户端数：`N` 低于它说明本机参数不足以形成有意义的边界。
pub(crate) const MIN_CLIENTS: usize = 8;

/// 外边界倍数：`2N` 个客户端才能自证门槛真实存在（D-P2.2-007）。
pub(crate) const OUTER_FACTOR: usize = 2;

/// 内边界的「进队错峰」容忍：`N` 条 `message/create` 同时到达，但 Run 是在服务端各自的
/// 执行任务里进队，最后一个可能比第一个晚一个 `L0` 量级才进来，而此刻第一个已经把许可交出。
/// 因此「在跑 + 排队」的峰值允许比 `N - 1` 少这么多个；实测 5 次分别是 0、0、1、0、2。
///
/// 只影响这条诊断断言的容差，**不影响** `N` 的取值（D-P2.2-009 禁止对 `N` 放水）。
pub(crate) const QUEUE_PEAK_SLACK: usize = 4;

/// 标定的合计死线（预热轮 + 标定轮共用）。
pub(crate) const CALIBRATION_DEADLINE: Duration = Duration::from_secs(10);

/// 阶段「注册 → 会话 → 占用 → 订阅」的固定开销：客户端并发做，因此只按少量线性放大。
const SETUP_BASE: Duration = Duration::from_secs(4);
/// 阶段准备的每客户端放大：`N` 越大，服务端建会话与打开下行流越慢。
const SETUP_PER_CLIENT: Duration = Duration::from_millis(8);

/// 阶段收尾（全部抵终态）在等待预算之外留的余量：覆盖下行转发与落盘的尾延迟。
const SETTLE_MARGIN: Duration = Duration::from_secs(4);

/// 同一阶段的客户端「对齐起跑」前的等待：让所有客户端都进入发消息的同一时刻。
pub(crate) const START_ALIGN_DELAY: Duration = Duration::from_millis(20);

/// 启停与采集的允许量：起服务端 + 取快照 + 停止子进程。
const LIFECYCLE_ALLOWANCE: Duration = Duration::from_secs(5);

/// 公式本体：`floor(budget / per_run) + 1`，即不会失败的最大客户端数 `N_safe`。
///
/// `per_run` 为 0（测不到耗时）时返回 `usize::MAX`：让「参数不足」的判断与文案能复用同一条公式。
pub(crate) fn count(per_run: Duration, budget: Duration) -> usize {
    let per_run = per_run.as_nanos();
    if per_run == 0 {
        return usize::MAX;
    }
    usize::try_from(budget.as_nanos() / per_run + 1).unwrap_or(usize::MAX)
}

/// 边界客户端数：`per_run` 为 0，或结果低于 [`MIN_CLIENTS`] 时返回 `None`（参数不足以形成边界）。
pub(crate) fn boundary(per_run: Duration, budget: Duration) -> Option<usize> {
    if per_run.is_zero() {
        return None;
    }
    let safe = count(per_run, budget);
    (safe >= MIN_CLIENTS).then_some(safe)
}

/// 外边界客户端数：`2 × N`。
pub(crate) fn outer(clients: usize) -> usize {
    clients.saturating_mul(OUTER_FACTOR)
}

/// 一个阶段准备（注册 → 会话 → 占用 → 订阅）的死线。
pub(crate) fn setup_deadline(clients: usize) -> Duration {
    SETUP_BASE.saturating_add(SETUP_PER_CLIENT.saturating_mul(clients as u32))
}

/// 一个阶段从发消息到全部抵终态的死线。
pub(crate) fn settle_deadline() -> Duration {
    QUEUE_BUDGET.saturating_add(SETTLE_MARGIN)
}

/// 三段合计的死线上界：标定 + 两段准备与收敛 + 启停采集（P2.2-010 第 2 条的核算口径）。
pub(crate) fn budget_upper_bound(clients: usize) -> Duration {
    CALIBRATION_DEADLINE
        .saturating_add(setup_deadline(clients))
        .saturating_add(settle_deadline())
        .saturating_add(setup_deadline(outer(clients)))
        .saturating_add(settle_deadline())
        .saturating_add(LIFECYCLE_ALLOWANCE)
}

/// 报告口径的毫秒文本：`1234ms`。
pub(crate) fn millis(value: Duration) -> String {
    format!("{}ms", value.as_millis())
}

/// 报告口径的高精度毫秒文本：`41.7ms`（按 [`count`] 复算公式时要能对上）。
pub(crate) fn precise(value: Duration) -> String {
    format!("{:.1}ms", value.as_secs_f64() * 1_000.0)
}

/// 单次耗时分布：两个阶段共用同一份实现（G-1-3 的分布口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Distribution {
    /// 样本数。
    pub(crate) samples: usize,
    /// 最小值。
    pub(crate) min: Duration,
    /// 中位：偶数个样本取中间两个的平均。
    pub(crate) median: Duration,
    /// 95 分位：按 `ceil(0.95 × 样本数)` 取排序后的第几个。
    pub(crate) p95: Duration,
    /// 最大值。
    pub(crate) max: Duration,
}

impl Distribution {
    /// 由样本算出分布：空样本返回 `None`。
    pub(crate) fn of(samples: &[Duration]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let len = sorted.len();
        let median = if len % 2 == 1 {
            sorted[len / 2]
        } else {
            (sorted[len / 2 - 1] + sorted[len / 2]) / 2
        };
        let rank = (len * 95).div_ceil(100);
        let p95 = sorted[rank.saturating_sub(1).min(len - 1)];
        Some(Self {
            samples: len,
            min: sorted[0],
            median,
            p95,
            max: sorted[len - 1],
        })
    }

    /// 报告口径：`样本 12 次｜min 8.0ms｜中位 12.3ms｜p95 30.1ms｜max 41.7ms`。
    pub(crate) fn render(&self) -> String {
        format!(
            "样本 {} 次｜min {}｜中位 {}｜p95 {}｜max {}",
            self.samples,
            precise(self.min),
            precise(self.median),
            precise(self.p95),
            precise(self.max)
        )
    }
}
