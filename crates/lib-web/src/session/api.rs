//! 会话业务接口。
//!
//! 路径遵守 P1-004 的约束：没有路径占位符，会话与 Run 的 ID 走查询参数或请求体（D-008）；
//! 动作段只用契约规范固定的动词（`create` / `list` / `get` / `cancel`）。
//!
//! Handler 只做“参数解包 + 业务调用 + 错误映射”，业务规则全部在 `service-core`。

use crate::session::stream;
use anyhow::Result;
use framework_web::{Json, Query, WebError, WebResponse, web_api_get, web_api_post};
use lib_web_core::sse;
use service_core::holder;
use service_core::session::SessionError;
use types_core::session::{
    Message, MessageListParams, MessageSendPO, MessageSendVO, Run, RunCancelPO, RunGetParams, RunStreamParams,
    SessionCreatePO, SessionSummary,
};

/// 创建会话。
#[web_api_post(path = "api/session/create")]
pub async fn session_create(params: Json<SessionCreatePO>) -> Result<SessionSummary> {
    holder::session().create_session(params.0).map_err(api_error)
}

/// 列出全部会话，按最后更新时间倒序。
#[web_api_get(path = "api/session/list")]
pub async fn session_list() -> Vec<SessionSummary> {
    holder::session().list_sessions()
}

/// 列出指定会话的消息，按写入顺序返回。
#[web_api_get(path = "api/message/list")]
pub async fn message_list(params: Query<MessageListParams>) -> Result<Vec<Message>> {
    holder::session().list_messages(params.0).map_err(api_error)
}

/// 发送消息：写入用户消息并创建对应的 Run。
#[web_api_post(path = "api/message/create")]
pub async fn message_create(params: Json<MessageSendPO>) -> Result<MessageSendVO> {
    holder::session().send_message(params.0).map_err(api_error)
}

/// 查询 Run。
#[web_api_get(path = "api/run/get")]
pub async fn run_get(params: Query<RunGetParams>) -> Result<Run> {
    holder::session().get_run(params.0).map_err(api_error)
}

/// 取消未结束的 Run。
#[web_api_post(path = "api/run/cancel")]
pub async fn run_cancel(params: Json<RunCancelPO>) -> Result<Run> {
    holder::session().cancel_run(params.0).map_err(api_error)
}

/// 订阅 Run 的事件流：`text/event-stream`，每条事件是 `RunEvent`。
///
/// 只有 `QUEUED` 的 Run 可以订阅，订阅即把它推进到 `RUNNING`（D-026）。
/// 元数据里本接口是 `Blob`（D-007）：SDK 只导出 path 常量，SSE 的消费由客户端自己实现，
/// 事件类型用生成的 `RunEvent` 反序列化，`event:` 行等于 `event` 字段的取值。
#[web_api_get(path = "api/run/stream")]
pub async fn run_stream(params: Query<RunStreamParams>) -> Result<WebResponse> {
    let events = holder::session().subscribe_run(params.0).map_err(api_error)?;
    let mut response = WebResponse::stream(200, sse::CONTENT_TYPE, stream::body(events));
    response.headers.set(sse::HEADER_CACHE_CONTROL, sse::VALUE_NO_CACHE);
    response.headers.set(sse::HEADER_NO_BUFFERING, sse::VALUE_NO_BUFFERING);
    Ok(response)
}

/// 把业务错误映射成统一的错误响应（D-024）。
fn api_error(error: SessionError) -> anyhow::Error {
    match error {
        SessionError::NotFound(message) => WebError::not_found(message).into(),
        SessionError::Invalid(message) => WebError::parameter(message, "会话业务参数或状态不满足规则").into(),
        SessionError::Internal(message) => WebError::internal("会话业务内部错误", message).into(),
    }
}
