//! Agent 运行参数配置（`[agent]` 段）。
//!
//! 全部字段有缺省值：`[agent]` 段整体缺省时按 [`Default`] 取 README「配置面」里的值，
//! 段存在但只写了部分字段时，缺的字段同样回落到 `Default`（`#[serde(default)]` 作用于整个结构）。
//!
//! 各项的语义与使用点：`max_concurrent_runs` 是全局 `Semaphore` 的许可数（P2-025）；
//! `run_queue_timeout_ms` 是排队超时（超时落 `FAILED`「执行排队超时」）；`llm_idle_timeout_ms`
//! 是模型流的空闲超时（P2-016）；`context_max_messages` / `context_safety_tokens` 是 Context 预算的
//! 两个可调项（D-P2-012 / D-P2-013）。

use serde::Deserialize;

/// 全局并发上限的缺省值。
pub const DEFAULT_MAX_CONCURRENT_RUNS: usize = 4;
/// 排队超时的缺省值（毫秒）。
pub const DEFAULT_RUN_QUEUE_TIMEOUT_MS: u64 = 120_000;
/// 模型流空闲超时的缺省值（毫秒）。
pub const DEFAULT_LLM_IDLE_TIMEOUT_MS: u64 = 60_000;
/// Context 历史消息条数上限的缺省值。
pub const DEFAULT_CONTEXT_MAX_MESSAGES: usize = 40;
/// Context 预算预留的缺省值（token 数）。
pub const DEFAULT_CONTEXT_SAFETY_TOKENS: usize = 256;

/// Agent 运行参数。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// 全局并发上限：同时处于 `RUNNING` 的 Run 数量上限。
    pub max_concurrent_runs: usize,
    /// 排队超时（毫秒）：等待全局许可超过该时长即落 `FAILED`。
    pub run_queue_timeout_ms: u64,
    /// 模型流空闲超时（毫秒）：两段增量之间的最长等待。
    pub llm_idle_timeout_ms: u64,
    /// Context 保留的历史消息条数上限，超过则丢弃最旧的。
    pub context_max_messages: usize,
    /// Context 预算的安全余量（token 数）。
    pub context_safety_tokens: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_concurrent_runs: DEFAULT_MAX_CONCURRENT_RUNS,
            run_queue_timeout_ms: DEFAULT_RUN_QUEUE_TIMEOUT_MS,
            llm_idle_timeout_ms: DEFAULT_LLM_IDLE_TIMEOUT_MS,
            context_max_messages: DEFAULT_CONTEXT_MAX_MESSAGES,
            context_safety_tokens: DEFAULT_CONTEXT_SAFETY_TOKENS,
        }
    }
}
