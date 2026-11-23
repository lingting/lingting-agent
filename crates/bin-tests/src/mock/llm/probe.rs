//! Mock 的请求侧 helper：构造 chat 请求、发起请求、读 SSE 并汇总。
//!
//! `llm.mock` 只写步骤与断言，请求体构造与 SSE 解析集中在这里（P2.1-010）。
//! 断言口径（帧数、耗时、正文）仍由用例决定，这里只提供读取结果的形状。

use anyhow::{Context, Result};
use framework_http::{HttpClient, HttpRequest, HttpResponse, HttpStreamResponse};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

use super::reply::MODEL;

/// 回复正文前缀：Mock 回显最后一条消息的内容。
const REPLY_PREFIX: &str = "Mock 回复：";

/// 一次 chat 请求的 messages：system + 历史 + 本次输入，用于断言顺序。
pub(crate) fn chat_messages(prompt: &str) -> Vec<Value> {
    vec![
        json!({"role": "system", "content": "mock-system-prompt"}),
        json!({"role": "user", "content": "历史输入"}),
        json!({"role": "assistant", "content": "历史回复"}),
        json!({"role": "user", "content": prompt}),
    ]
}

/// 普通（非流式）chat 请求。
pub(crate) async fn post_chat(client: &HttpClient, url: &str, messages: &[Value]) -> Result<HttpResponse> {
    let body = chat_body(messages, false)?;
    client
        .post(
            HttpRequest::new(url)
                .with_content_type("application/json")
                .with_body(body),
        )
        .await
        .with_context(|| format!("POST {url} 失败"))
}

/// 流式 chat 请求。
pub(crate) async fn stream_chat(client: &HttpClient, url: &str, messages: &[Value]) -> Result<HttpStreamResponse> {
    let body = chat_body(messages, true)?;
    client
        .stream_post(url, body)
        .await
        .with_context(|| format!("POST {url}（流式）失败"))
}

/// 读完整条 SSE 响应：事件负载列表与读取耗时。
pub(crate) async fn read_sse(mut response: HttpStreamResponse) -> Result<(Vec<String>, Duration)> {
    let started = Instant::now();
    let mut buffer = Vec::new();
    let mut events = Vec::new();

    while let Some(chunk) = response.next().await {
        buffer.extend_from_slice(&chunk.context("读取 SSE 分块失败")?);
        while let Some(position) = buffer.windows(2).position(|window| window == b"\n\n") {
            let frame: Vec<u8> = buffer.drain(..position + 2).collect();
            let frame = String::from_utf8(frame).context("SSE 帧不是 UTF-8")?;
            if let Some(payload) = frame.trim_end_matches("\n\n").strip_prefix("data: ") {
                events.push(payload.to_string());
            }
        }
    }

    Ok((events, started.elapsed()))
}

/// SSE 事件的解析结果。
pub(crate) struct Streamed {
    /// 所有 `delta.content` 的拼接。
    pub(crate) text: String,
    /// 带正文的帧数。
    pub(crate) frames: usize,
    /// 是否收到 `[DONE]`。
    pub(crate) done: bool,
}

/// 汇总 SSE 事件。
pub(crate) fn summarize(events: &[String]) -> Streamed {
    let mut streamed = Streamed {
        text: String::new(),
        frames: 0,
        done: false,
    };
    for event in events {
        if event == "[DONE]" {
            streamed.done = true;
            continue;
        }
        let Ok(payload) = serde_json::from_str::<Value>(event) else {
            continue;
        };
        if let Some(content) = payload["choices"][0]["delta"]["content"].as_str() {
            streamed.text.push_str(content);
            streamed.frames += 1;
        }
    }
    streamed
}

/// 回复正文：Mock 回显最后一条消息。
pub(crate) fn expected(prompt: &str) -> String {
    format!("{REPLY_PREFIX}{prompt}")
}

/// chat 请求体。
fn chat_body(messages: &[Value], streaming: bool) -> Result<Vec<u8>> {
    serde_json::to_vec(&json!({
        "model": MODEL,
        "messages": messages,
        "stream": streaming,
        "max_tokens": 256,
    }))
    .context("序列化 chat 请求体失败")
}
