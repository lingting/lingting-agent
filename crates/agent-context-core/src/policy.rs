//! 预算参数与 Token 估算抽象。
//!
//! 预算是纯数据：`[agent]` 的两个可调项与模型配置的 `context_window` / `max_output_tokens`
//! 由调用方换算成 [`ContextPolicy`]，本 Crate 只负责按它计算可用额度（D-P2-012 第 3 条）。

/// 每条消息的固定 token 开销（D-P2-012 第 2 条）。
///
/// 角色标记、分隔符等结构开销与正文长度无关，按条计一次；估算一段 Context 的用量时，
/// 除正文估算外每条消息再加这个常量。
pub const MESSAGE_TOKEN_OVERHEAD: usize = 4;

/// Context 预算参数。
///
/// 字段全部来自配置，不含运行时状态；`context_window` / `max_output_tokens` 取自
/// `[[provider.models]]`，`safety_tokens` / `max_messages` 取自 `[agent]`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextPolicy {
    /// 模型的上下文窗口（token 数）。
    pub context_window: usize,
    /// 单次输出上限（token 数）：窗口里必须先留出这一块。
    pub max_output_tokens: usize,
    /// 安全余量（token 数）：估算偏差的兜底。
    pub safety_tokens: usize,
    /// 历史消息条数上限，超出则先丢弃最旧的。
    pub max_messages: usize,
}

impl ContextPolicy {
    /// 系统提示词之外可用于历史的 token 预算：
    /// `context_window - max_output_tokens - safety_tokens - system_tokens`。
    ///
    /// `system_tokens` 是**整条**系统消息的开销：P5 起它包含 Skill 索引小节（索引与 `AGENTS.md`
    /// 合并成同一条消息，D-P5-003），因此索引规模直接决定历史可用额度（D-P5-010）。
    /// 任一项超出窗口时按 0 处理（saturating），由调用方按「本次输入超预算」报错，不做补偿。
    pub fn available_tokens(&self, system_tokens: usize) -> usize {
        self.context_window
            .saturating_sub(self.max_output_tokens)
            .saturating_sub(self.safety_tokens)
            .saturating_sub(system_tokens)
    }
}

/// 文本 → token 估算的可替换抽象（D-P2-012 第 4 条）。
///
/// 实现只有一个默认（`agent-context` 的启发式估算）；与具体分词器的绑定方式（下载词表、
/// 本地模型）留到 P6 的 Context 压缩阶段再议，本阶段不做。
pub trait TokenEstimator: Send + Sync {
    /// 估算一段正文的 token 数。
    ///
    /// 只算正文，不含 [`MESSAGE_TOKEN_OVERHEAD`]；每条消息的固定开销由调用方按条累加。
    fn estimate(&self, text: &str) -> usize;
}
