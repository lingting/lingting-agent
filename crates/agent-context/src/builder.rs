//! Context Builder：把系统提示词、会话历史与本次用户输入组装成一次请求（P2-020）。
//!
//! 顺序固定（D-P2-013）：系统提示词（存在时）→ 历史（按 `seq` 升序）→ 本次用户输入。
//! 裁剪同样固定：先按 `max_messages` 截断最旧，再按预算逐条丢弃最旧；系统提示词与本次输入
//! 不参与裁剪。本次输入单条即超预算时返回 [`ContextError::PromptTooLarge`]，由调用方落 `FAILED`。

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

        let history_items: Vec<ContextItem> = history.iter().filter_map(history_item).collect();
        let dropped_by_count = trim::dropped_by_count(history_items.len(), policy.max_messages);
        let remainder = &history_items[dropped_by_count..];
        let dropped_by_budget = trim::dropped_by_budget(remainder, estimator, available);
        let kept = &remainder[dropped_by_budget..];

        let mut items: Vec<ContextItem> = Vec::with_capacity(kept.len() + 2);
        if let Some(item) = system_item {
            items.push(item);
        }
        items.extend(kept.iter().cloned());
        items.push(ContextItem::new(ContextSource::User, MessageRole::User, prompt));

        let metadata = ContextMetadata {
            session_id: session.id.clone(),
            model: model.to_owned(),
            trimmed_messages: dropped_by_count + dropped_by_budget,
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

/// 历史消息 → Context Item：只保留用户与助手两类，`SYSTEM` / `TOOL` 不进请求（D-P2-013）。
fn history_item(message: &Message) -> Option<ContextItem> {
    match message.role {
        MessageRole::User | MessageRole::Assistant => Some(ContextItem::new(
            ContextSource::History,
            message.role,
            message.content.as_str(),
        )),
        MessageRole::System | MessageRole::Tool => None,
    }
}
