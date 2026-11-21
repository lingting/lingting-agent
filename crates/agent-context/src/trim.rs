//! 历史裁剪：先按条数上限、再按 token 预算丢弃最旧（D-P2-013 第 1、2 条）。
//!
//! 两个函数都只返回「要丢弃的最旧条数」，由调用方从头部切走：历史按 `seq` 升序，丢弃只发生在
//! 最旧的一端；系统提示词与本次用户输入由调用方单独持有，因此永远不会被这里裁掉。

use agent_context_core::{ContextItem, TokenEstimator};

use crate::estimate::item_tokens;

/// 第一步：按条数上限计算要丢弃的最旧条数。
pub(crate) fn dropped_by_count(len: usize, max_messages: usize) -> usize {
    len.saturating_sub(max_messages)
}

/// 第二步：从最旧一端逐条丢弃，直到估算不超预算，返回需要**额外**丢弃的条数。
///
/// 整条丢弃而不是截断正文：一条历史要么完整进入请求，要么整条不要（D-P2-013 的取舍）。
pub(crate) fn dropped_by_budget(items: &[ContextItem], estimator: &dyn TokenEstimator, budget: usize) -> usize {
    let mut total = 0usize;
    let mut kept = 0usize;
    for item in items.iter().rev() {
        let cost = item_tokens(estimator, &item.content);
        if total.saturating_add(cost) > budget {
            break;
        }
        total += cost;
        kept += 1;
    }
    items.len() - kept
}
