//! 一次工具调用的输入：模型给出的事实 + 运行时补上的归属。

use serde_json::Value;

/// 一次工具调用。
///
/// 前三项是模型的原始产出（与 `agent-route` 的 `LlmToolCall` 一一对应，由运行时做一次映射）；
/// 后三项是运行时补上的归属，用于审计记录与 [`crate::ToolContext`]。
/// 本 Crate 不认识 Provider 协议，因此不复用任何协议类型。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    /// 模型给出的调用标识：事件、审计记录与工具结果三者同值。
    pub tool_call_id: String,
    /// 模型请求调用的工具名（可能根本没有注册）。
    pub tool_name: String,
    /// 模型给出的原始参数 JSON（未经校验，允许不是对象）。
    pub arguments: Value,
    /// 所属会话 ID。
    pub session_id: String,
    /// 所属 Run ID。
    pub run_id: String,
    /// 第几轮工具调用（从 1 开始）。
    pub tool_round: u32,
}
