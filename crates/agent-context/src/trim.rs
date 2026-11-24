//! 历史裁剪：先按条数上限、再按 token 预算丢弃最旧（D-P2-013 第 1、2 条）。
//!
//! 裁剪单位是「段」而不是单条消息：普通消息自成一段，工具调用组（助手 `tool_calls` 项 + 其
//! 全部工具结果项）合为一段（D-P3-025）。两个函数都只返回「要丢弃的最旧段数」，由调用方从头部
//! 切走：历史按 `seq` 升序，丢弃只发生在最旧的一端；系统提示词与本次用户输入由调用方单独持有，
//! 因此永远不会被这里裁掉。

use agent_context_core::{ContextItem, TokenEstimator};

use crate::estimate::item_tokens;

/// 把历史切成裁剪单位，返回每段的 `[start, end)` 下标。
///
/// 相邻且 `group_id` 相同的 Item 属于同一工具调用组；`group_id` 为空的 Item 自成一段。组的
/// 相邻性由写入口保证：一轮的助手消息与全部工具结果在同一事务里写入（D-P3-024），因此不会
/// 被其它消息插到中间。
pub(crate) fn chunks(items: &[ContextItem]) -> Vec<(usize, usize)> {
    let mut chunks: Vec<(usize, usize)> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let group = item.group_id.as_deref();
        let same_group = match (group, chunks.last()) {
            (Some(group), Some((start, _))) => items[*start].group_id.as_deref() == Some(group),
            _ => false,
        };
        if same_group {
            if let Some(last) = chunks.last_mut() {
                last.1 = index + 1;
            }
        } else {
            chunks.push((index, index + 1));
        }
    }
    chunks
}

/// 第一步：按条数上限计算要丢弃的最旧段数。
///
/// 段整体丢弃，因此丢完后的消息条数是各段长度的和：条数上限不再保证恰好取到，只会不超过它。
pub(crate) fn dropped_by_count(chunks: &[(usize, usize)], max_messages: usize) -> usize {
    let mut total: usize = chunks.iter().map(|(start, end)| end - start).sum();
    let mut dropped = 0usize;
    while dropped < chunks.len() && total > max_messages {
        let (start, end) = chunks[dropped];
        total -= end - start;
        dropped += 1;
    }
    dropped
}

/// 第二步：从最旧一端逐段丢弃，直到估算不超预算，返回需要**额外**丢弃的段数。
///
/// 整段丢弃而不是截断正文：一段历史要么完整进入请求，要么整段不要（D-P2-013 / D-P3-025 的
/// 取舍）。段内的每条消息都按 `MESSAGE_TOKEN_OVERHEAD` + 正文估算，段作为整体参与比较。
pub(crate) fn dropped_by_budget(
    chunks: &[(usize, usize)],
    items: &[ContextItem],
    estimator: &dyn TokenEstimator,
    budget: usize,
) -> usize {
    let mut total = 0usize;
    let mut kept = 0usize;
    for (start, end) in chunks.iter().rev() {
        let cost: usize = items[*start..*end]
            .iter()
            .map(|item| item_tokens(estimator, &item.content))
            .sum();
        if total.saturating_add(cost) > budget {
            break;
        }
        total += cost;
        kept += 1;
    }
    chunks.len() - kept
}
