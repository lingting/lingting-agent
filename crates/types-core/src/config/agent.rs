//! Agent 运行参数配置（`[agent]` 段）。
//!
//! 全部字段有缺省值：`[agent]` 段整体缺省时按 [`Default`] 取 README「配置面」里的值，
//! 段存在但只写了部分字段时，缺的字段同样回落到 `Default`（`#[serde(default)]` 作用于整个结构）。
//!
//! 各项的语义与使用点：`max_concurrent_runs` 是全局 `Semaphore` 的许可数（P2-025），写法见
//! [`Concurrency`]（P2.1-015）；`run_queue_timeout_ms` 是排队超时（超时落 `FAILED`「执行排队超时」）；
//! `llm_idle_timeout_ms` 是模型流的空闲超时（P2-016）；`context_max_messages` / `context_safety_tokens`
//! 是 Context 预算的两个可调项（D-P2-012 / D-P2-013）；`max_tool_rounds` 是一次 Run 内最多
//! 几轮 Provider 请求（工具轮次的终止条件，D-P3-009）；`run_max_retries` / `run_retry_backoff_ms`
//! 是同一次 LLM 请求的重试次数与退避（缺省 `0` 即不重试，D-P6-012）。

use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};

/// 全局并发上限的缺省值：3 倍 CPU 核数。
pub const DEFAULT_MAX_CONCURRENT_RUNS: Concurrency = Concurrency::Cores(3);
/// 排队超时的缺省值（毫秒）。
pub const DEFAULT_RUN_QUEUE_TIMEOUT_MS: u64 = 120_000;
/// 模型流空闲超时的缺省值（毫秒）。
pub const DEFAULT_LLM_IDLE_TIMEOUT_MS: u64 = 60_000;
/// Context 历史消息条数上限的缺省值。
pub const DEFAULT_CONTEXT_MAX_MESSAGES: usize = 40;
/// Context 预算预留的缺省值（token 数）。
pub const DEFAULT_CONTEXT_SAFETY_TOKENS: usize = 256;
/// 一次 Run 内工具轮次上限的缺省值（D-P3-009）。
pub const DEFAULT_MAX_TOOL_ROUNDS: usize = 8;
/// Run 级重试次数的缺省值：`0` 表示不重试，与 P2 以来的失败口径逐字段一致（D-P6-012）。
pub const DEFAULT_RUN_MAX_RETRIES: usize = 0;
/// 每次重试前的退避的缺省值（毫秒）。
pub const DEFAULT_RUN_RETRY_BACKOFF_MS: u64 = 500;
/// Run 级重试次数的上限：配置超过它是启动期硬错误（重试不是无限预算）。
pub const MAX_RUN_MAX_RETRIES: usize = 10;
/// 重试退避的上限（毫秒）：配置超过它是启动期硬错误（重试不该拖成长任务饥饿）。
pub const MAX_RUN_RETRY_BACKOFF_MS: u64 = 60_000;

/// 配置字段名：解析与校验的错误文案统一点名该字段。
const FIELD: &str = "[agent] max_concurrent_runs";

/// 全局并发上限：绝对值或 CPU 核数倍数。
///
/// 配置文件里两种写法等价：`max_concurrent_runs = 4`（绝对值）与 `max_concurrent_runs = "2C"`
/// （2 × CPU 核数）。核数由装配层取一次，最终许可数由 [`Concurrency::resolve`] 算出（P2.1-015）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Concurrency {
    /// 绝对许可数。
    Absolute(usize),
    /// CPU 核数的倍数。
    Cores(usize),
}

impl Concurrency {
    /// `Semaphore` 可接受的许可数上限（与 `tokio::sync::Semaphore::MAX_PERMITS` 一致）。
    pub const MAX_PERMITS: usize = usize::MAX >> 3;

    /// 按核数算出最终许可数：结果落在 `1..=MAX_PERMITS`，倍数溢出按上限饱和，不产生非法信号量。
    ///
    /// 纯函数：核数由调用方注入，便于按固定核数断言；`0` 与 `0C` 由装载校验拒绝，这里只兜底。
    pub fn resolve(self, cores: usize) -> usize {
        let permits = match self {
            Self::Absolute(permits) => permits,
            Self::Cores(factor) => factor.saturating_mul(cores.max(1)),
        };
        permits.clamp(1, Self::MAX_PERMITS)
    }

    /// 配置里写下的数值：绝对值本身，或核数倍数的倍数。
    pub fn factor(self) -> usize {
        match self {
            Self::Absolute(value) | Self::Cores(value) => value,
        }
    }
}

impl Default for Concurrency {
    fn default() -> Self {
        DEFAULT_MAX_CONCURRENT_RUNS
    }
}

impl<'de> Deserialize<'de> for Concurrency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(RawConcurrency)
    }
}

/// 配置文本里的两种写法：整数（绝对值）与 `"NC"`（核数倍数）。
struct RawConcurrency;

impl<'de> Visitor<'de> for RawConcurrency {
    type Value = Concurrency;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{FIELD} 应为整数或 \"NC\" 形式（如 \"2C\"）")
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        usize::try_from(value)
            .map(Concurrency::Absolute)
            .map_err(|_| E::custom(over_limit()))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        if value < 0 {
            return Err(E::custom(format!("{FIELD} 不接受负数：{value}")));
        }
        self.visit_u64(value as u64)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        parse_concurrency(value).map_err(E::custom)
    }
}

/// 解析 `"NC"` 文本：数字 + `C` 后缀表示核数倍数，纯数字视为绝对值。
fn parse_concurrency(text: &str) -> Result<Concurrency, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(format!("{FIELD} 不能为空"));
    }
    let (digits, cores) = match text.strip_suffix('C').or_else(|| text.strip_suffix('c')) {
        Some(digits) => (digits.trim(), true),
        None => (text, false),
    };
    let value: usize = digits
        .parse()
        .map_err(|_| format!("{FIELD} 取值无效：{text}（支持整数与 \"NC\" 形式，如 \"2C\"）"))?;
    Ok(if cores {
        Concurrency::Cores(value)
    } else {
        Concurrency::Absolute(value)
    })
}

/// 超过 `Semaphore` 可用上限的错误文案。
fn over_limit() -> String {
    format!("{FIELD} 超过可用上限 {}", Concurrency::MAX_PERMITS)
}

/// Agent 运行参数。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// 全局并发上限：同时处于 `RUNNING` 的 Run 数量上限。
    pub max_concurrent_runs: Concurrency,
    /// 排队超时（毫秒）：等待全局许可超过该时长即落 `FAILED`。
    pub run_queue_timeout_ms: u64,
    /// 模型流空闲超时（毫秒）：两段增量之间的最长等待。
    pub llm_idle_timeout_ms: u64,
    /// Context 保留的历史消息条数上限，超过则丢弃最旧的。
    pub context_max_messages: usize,
    /// Context 预算的安全余量（token 数）。
    pub context_safety_tokens: usize,
    /// 一次 Run 内最多几轮 Provider 请求（第一轮也算一轮）：用尽且仍要求调用工具即落 `FAILED`。
    pub max_tool_rounds: usize,
    /// 同一次 LLM 请求的重试次数：`0`（缺省）表示不重试，正整数时按次数重试（D-P6-012）。
    ///
    /// 只重试可重试分类（上游瞬时错误与传输类），不重新入队、不重跑已提交轮次。
    pub run_max_retries: usize,
    /// 每次重试前的固定退避（毫秒）：`0` 表示立即重试。
    pub run_retry_backoff_ms: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_concurrent_runs: DEFAULT_MAX_CONCURRENT_RUNS,
            run_queue_timeout_ms: DEFAULT_RUN_QUEUE_TIMEOUT_MS,
            llm_idle_timeout_ms: DEFAULT_LLM_IDLE_TIMEOUT_MS,
            context_max_messages: DEFAULT_CONTEXT_MAX_MESSAGES,
            context_safety_tokens: DEFAULT_CONTEXT_SAFETY_TOKENS,
            max_tool_rounds: DEFAULT_MAX_TOOL_ROUNDS,
            run_max_retries: DEFAULT_RUN_MAX_RETRIES,
            run_retry_backoff_ms: DEFAULT_RUN_RETRY_BACKOFF_MS,
        }
    }
}
