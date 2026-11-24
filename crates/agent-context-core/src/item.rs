//! Context 中的一条消息：来源 + 角色 + 正文，以及工具轨迹的两个附加维度。

use agent_core::session::{LlmToolCallVO, MessageRole};

use crate::source::ContextSource;

/// 参与一次请求的一条消息。
///
/// 角色直接复用 Agent 领域的 [`MessageRole`]，不另造一套枚举；`source` 只在组装与裁剪阶段
/// 使用，不参与 wire。
///
/// 三种形态与 `agent-route` 的 `LlmMessage` 一一对应（D-P3-003）：
///
/// - 普通文本：三个工具字段都为空；
/// - 助手请求调用：[`Self::tool_calls`] 非空，正文可以为空串；
/// - 工具结果：`source` 为 [`ContextSource::Tool`]，[`Self::tool_call_id`] 必填。
///
/// [`Self::group_id`] 是裁剪用的组标识（该轮助手消息的 `message_id`）：同一工具调用组的
/// assistant 项与其全部工具结果项取值相同，裁剪按组整体进行（D-P3-025）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextItem {
    /// 本条消息的来源。
    pub source: ContextSource,
    /// 消息角色。
    pub role: MessageRole,
    /// 消息正文。
    pub content: String,
    /// 工具结果应答的调用标识；只有工具结果项有值。
    pub tool_call_id: Option<String>,
    /// 助手本轮请求的调用清单；只有助手项可能非空。
    pub tool_calls: Vec<LlmToolCallVO>,
    /// 工具调用组标识；普通消息为空。
    pub group_id: Option<String>,
}

impl ContextItem {
    /// 组装一条普通文本消息：三个工具字段都为空。
    pub fn new(source: ContextSource, role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            source,
            role,
            content: content.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
            group_id: None,
        }
    }

    /// 组装一条助手请求工具调用的消息：`group_id` 取该助手消息的 ID。
    pub fn assistant_tools(
        group_id: impl Into<String>,
        tool_calls: Vec<LlmToolCallVO>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            source: ContextSource::History,
            role: MessageRole::Assistant,
            content: content.into(),
            tool_call_id: None,
            tool_calls,
            group_id: Some(group_id.into()),
        }
    }

    /// 组装一条工具结果消息：`group_id` 取所属助手消息的 ID。
    pub fn tool_result(
        group_id: impl Into<String>,
        tool_call_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            source: ContextSource::Tool,
            role: MessageRole::Tool,
            content: content.into(),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: Vec::new(),
            group_id: Some(group_id.into()),
        }
    }
}
