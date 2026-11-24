//! Context Builder：把系统提示词、会话历史与本次用户输入组装成一次请求（P2-020）。
//!
//! 顺序固定（D-P2-013）：系统提示词（存在时）→ 历史（按 `seq` 升序）→ 本次用户输入。
//! 裁剪同样固定：先按 `max_messages` 截断最旧，再按预算逐条丢弃最旧；系统提示词与本次输入
//! 不参与裁剪。裁剪的单位是「段」，工具调用组整组保留或整组丢弃（D-P3-025）。本次输入单条即
//! 超预算时返回 [`ContextError::PromptTooLarge`]，由调用方落 `FAILED`。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use agent_context_core::{
    Context, ContextError, ContextItem, ContextMetadata, ContextPolicy, ContextSource, TokenEstimator,
};
use agent_core::session::{Message, MessageRole, SessionSummary};

use crate::estimate::{HeuristicEstimator, item_tokens};
use crate::system::SystemPrompt;
use crate::trim;

/// Context 组装器：持有系统提示词来源与估算器，按调用方给出的预算组装。
///
/// 每个进程一个即可（无内部可变状态）：`build` 不做缓存，每次都会重新读取提示词文件。
pub struct ContextBuilder {
    /// 系统提示词来源。
    system_prompt: SystemPrompt,
    /// token 估算器；默认是 [`HeuristicEstimator`]。
    estimator: Arc<dyn TokenEstimator>,
}

impl ContextBuilder {
    /// 以应用根目录构造：估算器固定为 [`HeuristicEstimator`]。
    ///
    /// 这是 `HeuristicEstimator` 的唯一构造点（D-P2-012 第 4 条）：换估算器走
    /// [`ContextBuilder::with_estimator`]，不在调用方各处 `new` 出估算器。
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self::with_estimator(home, Arc::new(HeuristicEstimator))
    }

    /// 替换估算器：供可替换性核对与后续阶段接入真实分词器使用（X-6）。
    pub fn with_estimator(home: impl Into<PathBuf>, estimator: Arc<dyn TokenEstimator>) -> Self {
        Self {
            system_prompt: SystemPrompt::new(home),
            estimator,
        }
    }

    /// 组装一次请求的 Context。
    ///
    /// `history` 必须是该会话此前的消息（按 `seq` 升序，不含本次输入）：组装不再排序，
    /// 顺序由调用方保证；`model` 只用于 metadata 与日志。
    pub fn build(
        &self,
        session: &SessionSummary,
        history: &[Message],
        prompt: &str,
        policy: &ContextPolicy,
        model: &str,
    ) -> Result<Context, ContextError> {
        let estimator = self.estimator.as_ref();

        let system_item = self.system_item()?;
        let system_tokens = system_item
            .as_ref()
            .map_or(0, |item| item_tokens(estimator, &item.content));
        let available = policy.available_tokens(system_tokens);

        let prompt_tokens = item_tokens(estimator, prompt);
        if prompt_tokens > available {
            return Err(ContextError::PromptTooLarge {
                estimated_tokens: prompt_tokens,
                available_tokens: available,
            });
        }

        let history_items = history_items(history)?;
        let chunks = trim::chunks(&history_items);
        let dropped_by_count = trim::dropped_by_count(&chunks, policy.max_messages);
        let remainder = &chunks[dropped_by_count..];
        let dropped_by_budget = trim::dropped_by_budget(remainder, &history_items, estimator, available);
        // 裁剪只丢最旧的一端，因此「保留的第一段起点」同时就是被裁掉的消息条数。
        let kept_start = chunks
            .get(dropped_by_count + dropped_by_budget)
            .map_or(history_items.len(), |(start, _)| *start);
        let kept = &history_items[kept_start..];
        let mut items: Vec<ContextItem> = Vec::with_capacity(kept.len() + 2);
        if let Some(item) = system_item {
            items.push(item);
        }
        items.extend(kept.iter().cloned());
        items.push(ContextItem::new(ContextSource::User, MessageRole::User, prompt));

        let metadata = ContextMetadata {
            session_id: session.id.clone(),
            model: model.to_owned(),
            trimmed_messages: kept_start,
            estimated_tokens: items.iter().map(|item| item_tokens(estimator, &item.content)).sum(),
        };
        tracing::info!(
            session_id = %metadata.session_id,
            model = %metadata.model,
            messages = items.len(),
            trimmed = metadata.trimmed_messages,
            tokens = metadata.estimated_tokens,
            "Context 组装完成"
        );

        Ok(Context::new(items, metadata))
    }

    /// 读取系统提示词并转成 Item：内容为空（没有提示词文件或文件为空）时返回 `None`。
    fn system_item(&self) -> Result<Option<ContextItem>, ContextError> {
        let content = self.system_prompt.load()?;
        if content.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(ContextItem::new(
            ContextSource::System,
            MessageRole::System,
            content,
        )))
    }
}

/// 历史消息 → Context Item：`SYSTEM` 不进请求，其余角色逐条映射（D-P3-003）。
///
/// 助手消息带 `tool_calls` 时保留调用清单，工具结果消息带 `tool_call_id`，两者共用「助手消息
/// ID」作为组标识（D-P3-025）。工具结果找不到对应的助手调用属于被写坏的历史，组装直接失败。
fn history_items(history: &[Message]) -> Result<Vec<ContextItem>, ContextError> {
    // 调用标识 → 所属助手消息 ID；助手调用总在结果之前，因此单趟扫描足够。
    let mut owners: HashMap<&str, &str> = HashMap::new();
    let mut items: Vec<ContextItem> = Vec::with_capacity(history.len());
    for message in history {
        match message.role {
            MessageRole::System => {}
            MessageRole::User => items.push(ContextItem::new(
                ContextSource::History,
                MessageRole::User,
                message.content.as_str(),
            )),
            MessageRole::Assistant => match message.tool_calls.as_deref() {
                Some(calls) if !calls.is_empty() => {
                    for call in calls {
                        owners.insert(call.id.as_str(), message.id.as_str());
                    }
                    items.push(ContextItem::assistant_tools(
                        message.id.as_str(),
                        calls.to_vec(),
                        message.content.as_str(),
                    ));
                }
                _ => items.push(ContextItem::new(
                    ContextSource::History,
                    MessageRole::Assistant,
                    message.content.as_str(),
                )),
            },
            MessageRole::Tool => {
                let tool_call_id = message.tool_call_id.as_deref();
                let owner = tool_call_id.and_then(|call_id| owners.get(call_id).copied());
                match (owner, tool_call_id) {
                    (Some(owner), Some(call_id)) => {
                        items.push(ContextItem::tool_result(owner, call_id, message.content.as_str()));
                    }
                    _ => {
                        return Err(ContextError::ToolPairing {
                            tool_call_id: tool_call_id.map(str::to_owned),
                        });
                    }
                }
            }
        }
    }
    Ok(items)
}
