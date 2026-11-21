//! 启发式 Token 估算（D-P2-012 第 2 条）。
//!
//! 公式：`ceil(ascii_chars / 4) + non_ascii_chars`，每条消息另加
//! [`MESSAGE_TOKEN_OVERHEAD`](agent_context_core::MESSAGE_TOKEN_OVERHEAD)。
//! 不引入 tokenizer 依赖：体积与离线要求都指向更保守的估算，宁可按偏大值裁剪。

use agent_context_core::{MESSAGE_TOKEN_OVERHEAD, TokenEstimator};

/// 启发式估算器：默认实现，也是本阶段唯一实现。
///
/// ASCII 按 4 字符 1 token 估；非 ASCII（中文、日文、emoji 等）按 1 字符 1 token 估，
/// 对中英混排偏保守。
#[derive(Debug, Clone, Copy, Default)]
pub struct HeuristicEstimator;

impl TokenEstimator for HeuristicEstimator {
    fn estimate(&self, text: &str) -> usize {
        let mut ascii_chars = 0usize;
        let mut other_chars = 0usize;
        for character in text.chars() {
            if character.is_ascii() {
                ascii_chars += 1;
            } else {
                other_chars += 1;
            }
        }
        ascii_chars.div_ceil(4) + other_chars
    }
}

/// 一条消息的估算开销：正文估算 + 固定结构开销。
pub(crate) fn item_tokens(estimator: &dyn TokenEstimator, content: &str) -> usize {
    estimator.estimate(content).saturating_add(MESSAGE_TOKEN_OVERHEAD)
}
