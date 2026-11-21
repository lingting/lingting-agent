//! Item 来源标记：区分系统提示、历史消息与本次输入。

/// Context Item 的来源。
///
/// 来源只用于组装与裁剪阶段的判断，不参与 wire，也不下发给模型。P3 起的 Tool / Skill / MCP
/// 贡献点在此追加取值，不改变既有语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSource {
    /// 系统提示词：来自 `AGENT_HOME` 下的 `AGENTS.md`，永不被裁。
    System,
    /// 会话历史：按 `seq` 升序排在系统提示词之后。
    History,
    /// 本次用户输入：排在最后，永不被裁。
    User,
}
