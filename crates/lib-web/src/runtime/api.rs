//! 客户端接口：注册、上行消息与下行事件流（D-P1.1-001 / D-P3-022）。
//!
//! 上行一律 `POST` + JSON 请求体，下行只有 `api/client/stream` 一条通道；
//! 没有路径占位符（D-008），`clientId` 只出现在请求体或查询参数里。

use crate::runtime::{
    MESSAGE_CONFIRM_FIELDS_UNEXPECTED, MESSAGE_DECISION_MISSING, MESSAGE_TOOL_CALL_ID_MISSING, invalid_fields,
    runtime_error, state, stream,
};
use anyhow::Result;
use framework_web::{Json, Query, WebResponse, web_api_get, web_api_post};
use lib_core::id;
use lib_web_core::sse;
use manager_agent::runtime::ClientRecord;
use types_core::runtime::{
    ClientCreatePO, ClientCreateVO, ClientDecision, ClientDeletePO, ClientPostKind, ClientPostPO, ClientPostVO,
    ClientStreamParams,
};

/// 注册客户端：`clientId` 由服务端生成，首个 pong 随注册响应下发。
#[web_api_post(path = "api/client/create")]
pub async fn client_create(Json(_po): Json<ClientCreatePO>) -> Result<ClientCreateVO> {
    let record = ClientRecord::connected(id::next_id()?);
    let client_id = record.client_id.clone();
    let pong_at_ms = record.last_seen_ms;
    state()?.register(record);
    Ok(ClientCreateVO { client_id, pong_at_ms })
}

/// 建立下行事件流：`text/event-stream`，全系统唯一的服务端推送通道。
///
/// 元数据里本接口是 `Blob`：SDK 只导出 path 常量，帧由客户端自行解析。
/// 流建立前的失败（未注册的 `clientId`）走普通错误响应，建立之后只能用 `client_closed` 帧或断流表达。
///
/// 流建立成功后立刻补发挂起中的确认事件（D-P3-023）：确认入口随断连丢失时，客户端重连
/// 仍能收到同一 `toolCallId` 的确认请求，不必等新的推理轮次。
#[web_api_get(path = "api/client/stream")]
pub async fn client_stream(Query(po): Query<ClientStreamParams>) -> Result<WebResponse> {
    let runtime = state()?;
    let events = runtime.open_stream(&po.client_id).map_err(runtime_error)?;
    runtime.resend_confirms(&po.client_id).await;
    let mut response = WebResponse::stream(200, sse::CONTENT_TYPE, stream::body(events));
    response.headers.set(sse::HEADER_CACHE_CONTROL, sse::VALUE_NO_CACHE);
    response.headers.set(sse::HEADER_NO_BUFFERING, sse::VALUE_NO_BUFFERING);
    Ok(response)
}

/// 上行消息：心跳、问询应答与工具确认共用一条通道，按 `kind` 分派（D-P3-022）。
///
/// pong 只在这条响应里，**不**进下行流。「问询应答」只清待答状态、**不**刷新 `lastSeen`
/// （D-P1.1-006）；「工具确认」同时刷新 `lastSeen` 并回 pong。未注册（含已被反注册）的
/// `clientId` 一律 400 + 固定 message，客户端据此重新注册（D-P1.1-005）。
#[web_api_post(path = "api/client/post")]
pub async fn client_post(Json(po): Json<ClientPostPO>) -> Result<ClientPostVO> {
    let pong_at_ms = match po.kind {
        ClientPostKind::Ping => {
            reject_confirm_fields(&po)?;
            state()?.refresh_heartbeat(&po.client_id).map_err(runtime_error)?
        }
        ClientPostKind::InquiryAck => {
            reject_confirm_fields(&po)?;
            state()?.answer_inquiry(&po.client_id).map_err(runtime_error)?
        }
        ClientPostKind::ToolConfirm => {
            let (tool_call_id, decision) = confirm_fields(&po)?;
            state()?
                .submit_confirm(&po.client_id, &tool_call_id, decision)
                .await
                .map_err(runtime_error)?
        }
    };
    Ok(ClientPostVO { pong_at_ms })
}

/// 主动断开：只打标记并通知断连处理链，反注册与资产清理都在处理链里完成。
///
/// 已移除的 `clientId` 返回 400 + 固定 message（D-P1.1-005），客户端据此重新注册；
/// 标记到清理之间的窗口中重复调用同样返回成功，因此调用方可以安全重试。
#[web_api_post(path = "api/client/delete")]
pub async fn client_delete(Json(po): Json<ClientDeletePO>) -> Result<()> {
    state()?.disconnect(&po.client_id).map_err(runtime_error)?;
    Ok(())
}

/// 心跳与问询应答不得携带确认字段：带了说明客户端用错了 `kind`，属请求错误。
fn reject_confirm_fields(po: &ClientPostPO) -> Result<()> {
    if po.tool_call_id.is_some() || po.decision.is_some() {
        return Err(invalid_fields(MESSAGE_CONFIRM_FIELDS_UNEXPECTED));
    }
    Ok(())
}

/// 工具确认必须两个字段齐全：`toolCallId` 指明确认哪一次调用，`decision` 是这次的决定。
fn confirm_fields(po: &ClientPostPO) -> Result<(String, ClientDecision)> {
    let Some(tool_call_id) = po.tool_call_id.as_deref() else {
        return Err(invalid_fields(MESSAGE_TOOL_CALL_ID_MISSING));
    };
    let Some(decision) = po.decision else {
        return Err(invalid_fields(MESSAGE_DECISION_MISSING));
    };
    Ok((tool_call_id.to_string(), decision))
}
