//! Context 中的一条消息：来源 + 角色 + 正文。

use agent_core::session::MessageRole;

use crate::source::ContextSource;

/// 参与一次请求的一条消息。
///
/// 角色直接复用 Agent 领域的 [`MessageRole`]，不另造一套枚举；`source` 只在组装与裁剪阶段
/// 使用，不参与 wire。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextItem {
    /// 本条消息的来源。
    pub source: ContextSource,
    /// 消息角色。
    pub role: MessageRole,
    /// 消息正文。
    pub content: String,
}

impl ContextItem {
    /// 组装一条消息。
    pub fn new(source: ContextSource, role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            source,
            role,
            content: content.into(),
        }
    }
}
