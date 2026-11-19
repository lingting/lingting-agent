//! 客户端接口：注册、心跳 / 问询应答与下行事件流（D-P1.1-001）。
//!
//! 上行一律 `POST` + JSON 请求体，下行只有 `api/client/stream` 一条通道；
//! 没有路径占位符（D-008），`clientId` 只出现在请求体或查询参数里。

use crate::runtime::{runtime_error, state, stream};
use anyhow::Result;
use framework_web::{Json, Query, WebResponse, web_api_get, web_api_post};
use lib_core::id;
use lib_web_core::sse;
use service_core::runtime::ClientRecord;
use types_core::runtime::{
    ClientCreatePO, ClientCreateVO, ClientDeletePO, ClientStreamParams, ClientUpdatePO, ClientUpdateVO,
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
#[web_api_get(path = "api/client/stream")]
pub async fn client_stream(Query(po): Query<ClientStreamParams>) -> Result<WebResponse> {
    let events = state()?.open_stream(&po.client_id).map_err(runtime_error)?;
    let mut response = WebResponse::stream(200, sse::CONTENT_TYPE, stream::body(events));
    response.headers.set(sse::HEADER_CACHE_CONTROL, sse::VALUE_NO_CACHE);
    response.headers.set(sse::HEADER_NO_BUFFERING, sse::VALUE_NO_BUFFERING);
    Ok(response)
}

/// 心跳 / 问询应答：pong 就在这次请求的响应体里，**不**进下行流（一次心跳只有一帧）。
///
/// 「问询应答」只清待答状态、**不**刷新 `lastSeen`（D-P1.1-006）；未注册（含已被反注册）的
/// `clientId` 一律 400 + 固定 message，客户端据此重新注册（D-P1.1-005）。
#[web_api_post(path = "api/client/update")]
pub async fn client_update(Json(po): Json<ClientUpdatePO>) -> Result<ClientUpdateVO> {
    let pong_at_ms = state()?.heartbeat(&po.client_id, po.kind).map_err(runtime_error)?;
    Ok(ClientUpdateVO { pong_at_ms })
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
