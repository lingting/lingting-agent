//! 会话业务的对外入口：转调 `service-core` 的 Service，并补齐装配层的协调动作。
//!
//! 两个协调点：一是「发送消息后唤醒该会话的执行队列」：Run 只有在会话被占用时才会被执行
//! （D-P1.1-004），而占用信息在运行时会话里，因此这个动作只能在装配层完成（D-P2-035）；
//! 二是「创建会话时的模型归属校验」：模型清单来自 Provider 配置（D-P2-015），校验放在这里
//! 可以让 `service-core` 继续不感知 Provider。

use agent_core::session::{
    Message, MessageListParams, MessageSendPO, MessageSendVO, Run, RunCancelPO, RunGetParams, SessionCreatePO,
    SessionSummary,
};
use agent_route::{LlmError, ProviderService};
use service_core::session::{SessionError, SessionService, ToolAuditListParams, ToolCallVO};

use crate::holder::{holder, runtime};

/// 创建会话。
///
/// 未指定模型时用 `provider.default_model`；指定了就必须出现在 `[provider.models]` 里，否则对外
/// 400「模型不可用：<id>」（D-P2-015）。没有 `[provider]` 段时不校验，沿用 `service-core` 的兜底值，
/// 保证「缺省配置可启动」。
pub async fn create_session(mut po: SessionCreatePO) -> Result<SessionSummary, SessionError> {
    if let Some(model) = resolve_model(provider()?, po.model.take())? {
        po.model = Some(model);
    }
    session()?.create_session(po).await
}

/// 按 Provider 配置决定本次会话的模型标识。
///
/// 返回 `Some` 表示要写回请求（默认模型，或已校验过的显式模型）；`None` 表示 Provider 未配置，
/// 请求原样交给 `service-core`。
fn resolve_model(provider: &ProviderService, requested: Option<String>) -> Result<Option<String>, SessionError> {
    if !provider.configured() {
        return Ok(requested);
    }
    let model = match requested.filter(|id| !id.trim().is_empty()) {
        Some(id) => provider.model(Some(&id)).map_err(invalid)?,
        None => provider.default_model().map_err(invalid)?,
    };
    Ok(Some(model.id.clone()))
}

/// Provider 侧的错误对外一律是参数错误（400），文案直接取脱敏后的公开文案。
fn invalid(error: LlmError) -> SessionError {
    SessionError::Invalid(error.public_message())
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

/// 取消未结束的 Run：数据库先落终态，再让执行方中断模型调用（D-P2-010 第 2 条）。
pub async fn cancel_run(po: RunCancelPO) -> Result<Run, SessionError> {
    let run = session()?.cancel_run(po).await?;
    // 条件更新已成功：Run 若还在执行中，触发取消信号让它 drop 模型流并发出 `run_cancelled`
    // （原因文本由执行方从数据库读回，因此顺序不能交换）。
    if let Some(runtime) = runtime()
        && !runtime.cancel_run(&run.id)
    {
        tracing::debug!(run_id = %run.id, "Run 不在执行中，无需中断模型调用");
    }
    Ok(run)
}

/// 未结束的 Run 数量：探活接口用。
///
/// 装配未完成时返回 0（探活不应因存储不可用而不可用），但**查询失败必须向上暴露**，
/// 由 Handler 决定怎么报错（D-P2-036）。
pub async fn running_run_count() -> Result<i64, SessionError> {
    session()?.running_run_count().await
}

/// 按 Run 查询工具审计记录（只读）：审计记录在 `service-core`，契约快照在 `agent-tool`，
/// 两者的映射收敛在本层，Handler 只做参数解包。
pub async fn list_tool_calls(po: ToolAuditListParams) -> Result<Vec<ToolCallVO>, SessionError> {
    let records = session()?.tool_calls_of_run(&po.run_id).await?;
    Ok(records.iter().map(ToolCallVO::from).collect())
}

/// 取已注入的会话业务服务；未注入说明启动装配不完整，属服务端内部错误。
fn session() -> Result<&'static SessionService, SessionError> {
    holder()
        .map(|holder| holder.session().as_ref())
        .ok_or_else(|| SessionError::Internal("启动装配未完成：会话服务未注入".to_string()))
}

/// 取已注入的 Provider 配置服务；未注入说明启动装配不完整，属服务端内部错误。
fn provider() -> Result<&'static ProviderService, SessionError> {
    holder()
        .map(|holder| holder.provider())
        .ok_or_else(|| SessionError::Internal("启动装配未完成：Provider 配置未注入".to_string()))
}
