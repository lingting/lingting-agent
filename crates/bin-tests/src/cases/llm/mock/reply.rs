//! OpenAI 兼容响应体：模型清单、普通补全、流式增量帧与错误体。

use serde_json::{Value, json};

/// Mock 提供的唯一模型 id；`bin-tests` 服务端的 `config.toml` 用的是同一个 id。
pub(crate) const MODEL: &str = "mock-model";

/// 每个增量帧携带的字符数：够小，保证 `message_delta` 条数远大于 1。
const CHUNK_CHARS: usize = 6;

/// 故意写坏的 JSON（`InvalidJson` 场景）。
pub(crate) const BROKEN_JSON: &str = r#"{"choices": [ 这不是合法 JSON"#;

/// `/v1/models` 的响应体。
pub(crate) fn models() -> Value {
    json!({
        "object": "list",
        "data": [{"id": MODEL, "object": "model", "created": 0, "owned_by": "bin-tests"}],
    })
}

/// 认证失败的响应体（401）。
pub(crate) fn auth_error() -> Value {
    json!({
        "error": {
            "message": "Incorrect API key provided",
            "type": "invalid_request_error",
            "code": "invalid_api_key",
        },
    })
}

/// 上游 5xx 的响应体（503）。
pub(crate) fn upstream_error() -> Value {
    json!({
        "error": {
            "message": "The server is overloaded",
            "type": "server_error",
            "code": "server_error",
        },
    })
}

/// 回复正文：回显最后一条消息，用例据此确认「回复对应哪次请求」。
pub(crate) fn text(body: &Value) -> String {
    format!("Mock 回复：{}", last_content(body))
}

/// 普通（非流式）补全的响应体。
pub(crate) fn completion(body: &Value, text: &str) -> Value {
    let (input, output) = usage(body, text);
    json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion",
        "created": 0,
        "model": model(body),
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": "stop",
        }],
        "usage": {"prompt_tokens": input, "completion_tokens": output, "total_tokens": input + output},
    })
}

/// 流式增量帧序列（不含 `[DONE]`）：每个分块一帧，最后一帧带 `finish_reason` 与用量。
pub(crate) fn stream_frames(body: &Value, text: &str) -> Vec<String> {
    let model = model(body);
    let pieces = chunks(text);
    let mut frames = Vec::with_capacity(pieces.len() + 1);

    for (index, piece) in pieces.iter().enumerate() {
        // 首帧带上角色，与 OpenAI 的真实流一致。
        let delta = if index == 0 {
            json!({"role": "assistant", "content": piece})
        } else {
            json!({"content": piece})
        };
        frames.push(frame(
            &model,
            json!([{"index": 0, "delta": delta, "finish_reason": null}]),
            None,
        ));
    }

    let (input, output) = usage(body, text);
    let usage = json!({"prompt_tokens": input, "completion_tokens": output, "total_tokens": input + output});
    frames.push(frame(
        &model,
        json!([{"index": 0, "delta": {}, "finish_reason": "stop"}]),
        Some(usage),
    ));
    frames
}

/// 回复正文的分块：按字符切分，不拆开多字节字符。
fn chunks(text: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut count = 0;
    for character in text.chars() {
        current.push(character);
        count += 1;
        if count == CHUNK_CHARS {
            pieces.push(std::mem::take(&mut current));
            count = 0;
        }
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

/// 一帧流式增量。
fn frame(model: &str, choices: Value, usage: Option<Value>) -> String {
    let mut payload = json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": model,
        "choices": choices,
    });
    if let Some(usage) = usage {
        payload["usage"] = usage;
    }
    payload.to_string()
}

/// 请求体里的模型 id：没写时用 [`MODEL`]。
fn model(body: &Value) -> String {
    body.get("model").and_then(Value::as_str).unwrap_or(MODEL).to_string()
}

/// 最后一条消息的文本内容。
fn last_content(body: &Value) -> String {
    body.get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| messages.last())
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// 用量估算：四个字符一个 token，至少 1，保证两个计数都大于 0。
fn usage(body: &Value, text: &str) -> (u64, u64) {
    let prompt: usize = body
        .get("messages")
        .and_then(Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .filter_map(|message| message.get("content").and_then(Value::as_str))
                .map(|content| content.chars().count())
                .sum()
        })
        .unwrap_or(0);
    (tokens(prompt), tokens(text.chars().count()))
}

/// 字符数换算成 token 数。
fn tokens(chars: usize) -> u64 {
    ((chars as u64) / 4).max(1)
}
