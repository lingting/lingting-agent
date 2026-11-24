//! 会话业务接口。
//!
//! 路径遵守 P1-004 的约束：没有路径占位符，会话与 Run 的 ID 走查询参数或请求体（D-008）；
//! 动作段只用契约规范固定的动词（`create` / `list` / `get` / `cancel`）。
//!
//! Handler 只做“参数解包 + 业务调用 + 错误映射”，业务规则全部在 `service-core`，
//! 装配与跨服务协调在 `manager-agent`（P2-013 起不再读进程级单例）。

use agent_core::session::{
    Message, MessageListParams, MessageSendPO, MessageSendVO, Run, RunCancelPO, RunGetParams, SessionCreatePO,
    SessionSummary,
};
use anyhow::Result;
use framework_web::{Json, Query, WebError, web_api_get, web_api_post};
use manager_agent::SessionError;

/// 创建会话。
#[web_api_post(path = "api/session/create")]
pub async fn session_create(Json(po): Json<SessionCreatePO>) -> Result<SessionSummary> {
    manager_agent::create_session(po).await.map_err(api_error)
}

/// 列出全部会话，按最后更新时间倒序。
///
/// 读库失败必须作为错误返回（D-P2-036）：不允许把存储异常吞成空列表，否则客户端无法区分
/// 「没有会话」与「数据库坏了」。
#[web_api_get(path = "api/session/list")]
pub async fn session_list() -> Result<Vec<SessionSummary>> {
    manager_agent::list_sessions().await.map_err(api_error)
}

/// 列出指定会话的消息，按写入顺序返回。
#[web_api_get(path = "api/message/list")]
pub async fn message_list(Query(po): Query<MessageListParams>) -> Result<Vec<Message>> {
    manager_agent::list_messages(po).await.map_err(api_error)
}

/// 发送消息：写入用户消息并创建对应的 Run。
#[web_api_post(path = "api/message/create")]
pub async fn message_create(Json(po): Json<MessageSendPO>) -> Result<MessageSendVO> {
    manager_agent::send_message(po).await.map_err(api_error)
}

/// 查询 Run。
#[web_api_get(path = "api/run/get")]
pub async fn run_get(Query(po): Query<RunGetParams>) -> Result<Run> {
    manager_agent::get_run(po).await.map_err(api_error)
}

/// 取消未结束的 Run。
#[web_api_post(path = "api/run/cancel")]
pub async fn run_cancel(Json(po): Json<RunCancelPO>) -> Result<Run> {
    manager_agent::cancel_run(po).await.map_err(api_error)
}

/// 把业务错误映射成统一的错误响应（D-024）。
pub(crate) fn api_error(error: SessionError) -> anyhow::Error {
    match error {
        SessionError::NotFound(message) => WebError::not_found(message).into(),
        SessionError::Invalid(message) => WebError::parameter(message, "会话业务参数或状态不满足规则").into(),
        SessionError::Internal(message) => WebError::internal("会话业务内部错误", message).into(),
    }
}
