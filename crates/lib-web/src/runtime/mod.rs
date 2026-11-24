//! 运行时接口：客户端注册、上行消息与下行事件流（P1.1 / P3-027）。
//!
//! Handler 只做“参数解包 + 业务调用 + 错误映射”，运行时规则全部在 `manager_agent::runtime`；
//! 帧编码集中在 [`stream`]，错误映射与固定 message 集中在本文件（D-P1.1-005 的收敛点）。

pub mod api;
pub mod occupancy_api;
mod stream;

use framework_web::WebError;
use manager_agent::runtime::{RuntimeError, RuntimeState};
/// 「客户端未注册」的固定 message（D-P1.1-005）：客户端按 message 识别并重新注册。
const MESSAGE_CLIENT_NOT_REGISTERED: &str = "客户端未注册，请重新连接";
/// 「会话已被其他客户端占用」的固定 message。
const MESSAGE_SESSION_OCCUPIED: &str = "会话已被其他客户端占用";
/// 「客户端已占用其他会话」的固定 message。
const MESSAGE_CLIENT_BUSY: &str = "客户端已占用其他会话";
/// 「会话未被占用」的固定 message。
const MESSAGE_NOT_OCCUPIED: &str = "会话未被占用";
/// 「客户端未占用会话」的固定 message：工具确认必须来自占用该会话的客户端（D-P3-023）。
const MESSAGE_NO_OCCUPANCY: &str = "客户端未占用会话";
/// 「待确认的调用不存在或已处理」的固定 message：重复确认与过期 `toolCallId` 共用。
const MESSAGE_PENDING_TOOL_CALL_NOT_FOUND: &str = "待确认的工具调用不存在或已处理";
/// 工具确认缺少 `toolCallId` 的固定 message。
pub(crate) const MESSAGE_TOOL_CALL_ID_MISSING: &str = "工具确认缺少 toolCallId";
/// 工具确认缺少 `decision` 的固定 message。
pub(crate) const MESSAGE_DECISION_MISSING: &str = "工具确认缺少 decision";
/// 心跳 / 问询应答携带确认字段的固定 message：说明客户端用错了 `kind`。
pub(crate) const MESSAGE_CONFIRM_FIELDS_UNEXPECTED: &str = "心跳与问询应答不得携带工具确认字段";
/// 内部失败对外只有这一句，细节只进日志。
const MESSAGE_INTERNAL: &str = "服务器内部错误";

/// 运行时错误的统一出口：框架支持自定义 code 后只改这里。
fn runtime_error(error: RuntimeError) -> anyhow::Error {
    match error {
        RuntimeError::ClientNotRegistered => WebError::parameter(MESSAGE_CLIENT_NOT_REGISTERED, error).into(),
        RuntimeError::SessionOccupied => WebError::parameter(MESSAGE_SESSION_OCCUPIED, error).into(),
        RuntimeError::ClientBusy => WebError::parameter(MESSAGE_CLIENT_BUSY, error).into(),
        // 会话不存在时带上 `sessionId`，与 session 领域既有文案风格一致。
        RuntimeError::SessionNotFound(_) => WebError::not_found(error.to_string()).into(),
        RuntimeError::NotOccupied => WebError::not_found(MESSAGE_NOT_OCCUPIED).into(),
        RuntimeError::NoOccupancy => WebError::parameter(MESSAGE_NO_OCCUPANCY, error).into(),
        RuntimeError::PendingToolCallNotFound => WebError::parameter(MESSAGE_PENDING_TOOL_CALL_NOT_FOUND, error).into(),
        RuntimeError::Internal(detail) => WebError::internal(MESSAGE_INTERNAL, detail).into(),
    }
}

/// 请求字段组合不合法的统一出口：400 + 固定 message（客户端判定是请求写错，不是业务冲突）。
pub(crate) fn invalid_fields(message: &'static str) -> anyhow::Error {
    WebError::parameter(message, message).into()
}

/// 取已注入的运行时状态；缺失说明启动装配不完整，属服务端内部错误。
fn state() -> anyhow::Result<&'static RuntimeState> {
    manager_agent::runtime().ok_or_else(|| WebError::message("运行时状态未注入").into())
}
