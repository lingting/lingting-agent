//! 路由与场景行为：把请求变成 OpenAI 兼容响应，并把观测结果记入 [`MockState`]。
//!
//! 路径按后缀匹配（`…/chat/completions`、`…/models`），因此 `[provider] base_url`
//! 写 `…/v1` 或不写都能用。

use anyhow::Result;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::time::sleep;

use super::http::{self, Request};
use super::record::{MockState, StreamOutcome};
use super::reply;
use super::scenario::{Plan, Scenario};

/// 处理一条连接：读一个请求、回一个响应，然后关闭。
pub(crate) async fn handle(mut socket: TcpStream, state: Arc<MockState>) {
    let request = match http::read_request(&mut socket).await {
        Ok(request) => request,
        // 读失败（客户端提前断开或请求格式非法）时没有可回应的请求，直接关闭。
        Err(_) => return,
    };
    // 响应写失败只说明客户端已经走了，不需要再处理。
    let _ = route(&mut socket, &state, request).await;
}

async fn route(socket: &mut TcpStream, state: &MockState, request: Request) -> Result<()> {
    state.record_request();
    let get = request.method.eq_ignore_ascii_case("GET");
    let post = request.method.eq_ignore_ascii_case("POST");

    if get && request.path.ends_with("/models") {
        return http::write_json(socket, 200, &reply::models()).await;
    }
    if get && request.path.ends_with("/mock/stats") {
        return http::write_json(socket, 200, &state.snapshot()).await;
    }
    if get && request.path.ends_with("/mock/last-request") {
        return match state.last_request() {
            Some(body) => http::write_json(socket, 200, &body).await,
            None => not_found(socket, "还没有收到 chat 请求").await,
        };
    }
    if post && request.path.ends_with("/chat/completions") {
        return chat(socket, state, &request).await;
    }
    not_found(socket, &format!("不认识的路径：{}", request.path)).await
}

async fn chat(socket: &mut TcpStream, state: &MockState, request: &Request) -> Result<()> {
    let Some(body) = request.json() else {
        return http::write_json(socket, 400, &json!({"error": {"message": "请求体不是合法 JSON"}})).await;
    };
    let plan = Plan::resolve(request, &body);
    let streaming = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    state.record_chat(&body, plan.scenario, streaming);

    match plan.scenario {
        Scenario::AuthFailure => return http::write_json(socket, 401, &reply::auth_error()).await,
        Scenario::UpstreamError => return http::write_json(socket, 503, &reply::upstream_error()).await,
        Scenario::InvalidJson => {
            return http::write_body(socket, 200, "application/json", reply::BROKEN_JSON.as_bytes()).await;
        }
        _ => {}
    }
    if !streaming {
        return http::write_json(socket, 200, &reply::completion(&body, &reply::text(&body))).await;
    }
    stream(socket, state, &body, &plan).await
}

/// 流式响应：分块之间按 `chunk_interval` 停顿，每块之后探一次客户端是否已关闭。
async fn stream(socket: &mut TcpStream, state: &MockState, body: &Value, plan: &Plan) -> Result<()> {
    if http::begin_sse(socket).await.is_err() {
        state.record_stream(StreamOutcome::Aborted);
        return Ok(());
    }
    let frames = reply::stream_frames(body, &reply::text(body));

    // 两种「不正常收尾」的场景只写一帧就停：不写 `[DONE]`。
    match plan.scenario {
        Scenario::Disconnect => {
            if let Some(first) = frames.first() {
                let _ = http::write_event(socket, first).await;
            }
            return Ok(());
        }
        Scenario::Stall => {
            if let Some(first) = frames.first() {
                let _ = http::write_event(socket, first).await;
            }
            sleep(plan.stall).await;
            return Ok(());
        }
        _ => {}
    }

    for frame in &frames {
        if !write(socket, frame).await {
            state.record_stream(StreamOutcome::Aborted);
            return Ok(());
        }
        if !plan.chunk_interval.is_zero() {
            sleep(plan.chunk_interval).await;
        }
    }
    if write(socket, "[DONE]").await {
        state.record_stream(StreamOutcome::Completed);
    } else {
        state.record_stream(StreamOutcome::Aborted);
    }
    Ok(())
}

/// 写一帧并确认连接仍然可用；返回 `false` 表示客户端已经关闭。
async fn write(socket: &mut TcpStream, frame: &str) -> bool {
    http::write_event(socket, frame).await.is_ok() && !http::is_closed(socket)
}

async fn not_found(socket: &mut TcpStream, message: &str) -> Result<()> {
    http::write_json(socket, 404, &json!({"error": {"message": message}})).await
}
