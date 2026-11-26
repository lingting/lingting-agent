//! Item 来源标记：区分系统提示、历史消息、工具结果与本次输入。

/// Context Item 的来源。
///
/// 来源只用于组装与裁剪阶段的判断，不参与 wire，也不下发给模型。P3 起的 Tool / Skill / MCP
/// 来源只用于组装与裁剪阶段的判断，不参与 wire，也不下发给模型。P3 起的 Tool / MCP 等贡献点
/// 在此追加取值，不改变既有语义。
///
/// Skill 是例外（D-P5-003）：索引小节与 `AGENTS.md` 合并成**同一条** `System` 消息，本阶段不新增
/// 取值；Skill 正文只作为工具结果（`Tool`）在后续轮次出现，因此也不需要新的来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSource {
    /// 系统提示词：来自 `AGENT_HOME` 下的 `AGENTS.md`，P5 起追加 Skill 索引小节（同一条消息，
    /// D-P5-003），永不被裁。
    System,
    /// 会话历史：按 `seq` 升序排在系统提示词之后。
    History,
    /// 工具结果（P3 起使用）：本轮某个工具调用的回填，与所属的助手调用项同组（D-P3-025）。
    Tool,
    /// 本次用户输入：排在最后，永不被裁。
    User,
}
