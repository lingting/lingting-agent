//! 会话业务的对外入口：转调 `service-core` 的 Service，并补齐装配层的协调动作。
//!
//! 唯一的协调点是「发送消息后唤醒该会话的执行队列」：Run 只有在会话被占用时才会被执行
//! （D-P1.1-004），而占用信息在运行时会话里，因此这个动作只能在装配层完成（D-P2-035）。

use agent_core::session::{
    Message, MessageListParams, MessageSendPO, MessageSendVO, Run, RunCancelPO, RunGetParams, SessionCreatePO,
    SessionSummary,
};
use service_core::session::{SessionError, SessionService};

use crate::holder::{holder, runtime};

/// 创建会话。
pub async fn create_session(po: SessionCreatePO) -> Result<SessionSummary, SessionError> {
    session()?.create_session(po).await
}

/// 列出全部会话，按最后更新时间倒序。
pub async fn list_sessions() -> Result<Vec<SessionSummary>, SessionError> {
    session()?.list_sessions().await
}

/// 列出指定会话的消息，按写入顺序返回。
pub async fn list_messages(po: MessageListParams) -> Result<Vec<Message>, SessionError> {
    session()?.list_messages(po).await
}

/// 发送消息：写入用户消息与 `QUEUED` 的 Run，并在会话被占用时唤醒执行队列。
pub async fn send_message(po: MessageSendPO) -> Result<MessageSendVO, SessionError> {
    let sent = session()?.send_message(po).await?;
    // 未被占用的会话里 Run 停在 QUEUED，不排队也不执行，这里记录原因便于排查。
    if let Some(runtime) = runtime()
        && !runtime.notify_run(&sent.run.session_id)
    {
        tracing::info!(
            session_id = %sent.run.session_id,
            "会话未被占用，Run 保持 QUEUED（需先建立占用）"
        );
    }
    Ok(sent)
}

/// 查询 Run。
pub async fn get_run(po: RunGetParams) -> Result<Run, SessionError> {
    session()?.get_run(po).await
}

/// 取消未结束的 Run。
pub async fn cancel_run(po: RunCancelPO) -> Result<Run, SessionError> {
    session()?.cancel_run(po).await
}

/// 未结束的 Run 数量：探活接口用。
///
/// 装配未完成时返回 0（探活不应因存储不可用而不可用），但**查询失败必须向上暴露**，
/// 由 Handler 决定怎么报错（D-P2-036）。
pub async fn running_run_count() -> Result<i64, SessionError> {
    session()?.running_run_count().await
}

/// 取已注入的会话业务服务；未注入说明启动装配不完整，属服务端内部错误。
fn session() -> Result<&'static SessionService, SessionError> {
    holder()
        .map(|holder| holder.session().as_ref())
        .ok_or_else(|| SessionError::Internal("启动装配未完成：会话服务未注入".to_string()))
}
