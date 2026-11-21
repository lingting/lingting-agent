//! `llm.mock`：Mock LLM Server 自身可用性——四个端点、七种场景、分块间隔与连接关闭计数。
//!
//! 这里只验证 Mock 自己（以及它与真实 HTTP 客户端的配合）。「Provider 如何消费这些响应」
//! 由 P2-026 的 `llm.stream` / `llm.auth_error` / `llm.upstream_error` / `llm.invalid_json` 断言。

use crate::case::CaseFuture;
use crate::context::TestContext;
use anyhow::{Context, Result, ensure};
use framework_http::{HttpClient, HttpRequest, HttpResponse, HttpStreamResponse};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::time::sleep;

use super::MockServer;
use super::reply::MODEL;

/// 回复正文前缀：Mock 回显最后一条消息的内容。
const REPLY_PREFIX: &str = "Mock 回复：";

pub(crate) fn mock(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let server = MockServer::start().await?;
        let client = HttpClient::new();
        let mut notes = vec![format!("Mock LLM：{}/v1", server.base_url())];

        // 1) 模型清单：Provider 侧据此判断 Mock 可用。
        let models = get_json(&client, &server.url("v1/models")).await?;
        let ids: Vec<&str> = models["data"]
            .as_array()
            .map(|list| list.iter().filter_map(|model| model["id"].as_str()).collect())
            .unwrap_or_default();
        ensure!(ids.contains(&MODEL), "/v1/models 应包含 {MODEL}，实际 {ids:?}");
        notes.push(format!("模型清单：{ids:?}"));

        // 2) 普通补全：回显最后一条消息，用量有值。
        let prompt = "普通回复";
        let messages = chat_messages(prompt);
        let response = post_chat(&client, &server.url("v1/chat/completions"), &messages).await?;
        ensure!(response.status == 200, "普通补全应返回 200，实际 {}", response.status);
        let completion: Value = serde_json::from_slice(&response.body).context("普通补全的响应不是 JSON")?;
        let content = completion["choices"][0]["message"]["content"]
            .as_str()
            .context("普通补全缺少 choices[0].message.content")?;
        ensure!(content == expected(prompt), "普通补全的正文不符：{content}");
        ensure!(
            completion["usage"]["total_tokens"].as_u64().unwrap_or(0) > 0,
            "普通补全应带用量：{}",
            completion["usage"]
        );

        // 3) last-request：完整 messages 与顺序（Context 类断言的数据来源）。
        let recorded = get_json(&client, &server.url("mock/last-request")).await?;
        ensure!(
            recorded["model"] == MODEL,
            "last-request 的模型不符：{}",
            recorded["model"]
        );
        let recorded_messages = recorded["messages"]
            .as_array()
            .context("last-request 应返回完整的 messages 数组")?;
        ensure!(
            recorded_messages.len() == messages.len(),
            "last-request 的消息条数不符：{} != {}",
            recorded_messages.len(),
            messages.len()
        );
        for (index, expected_message) in messages.iter().enumerate() {
            ensure!(
                recorded_messages[index] == *expected_message,
                "last-request 第 {index} 条消息不符：{} != {}",
                recorded_messages[index],
                expected_message
            );
        }
        notes.push(format!("last-request：{} 条消息按序返回", recorded_messages.len()));

        // 4) 流式：增量拼接等于完整回复，帧数大于 1，分块间隔生效。
        let streaming_prompt = "流式增量拼接".repeat(4);
        let url = format!("{}?chunk_ms=150", server.url("v1/chat/completions"));
        let response = stream_chat(&client, &url, &chat_messages(&streaming_prompt)).await?;
        ensure!(response.status == 200, "流式补全应返回 200，实际 {}", response.status);
        let (events, elapsed) = read_sse(response).await?;
        let streamed = summarize(&events);
        ensure!(streamed.done, "正常场景的流式响应应以 [DONE] 收尾");
        ensure!(streamed.frames > 1, "流式响应应分成多帧，实际 {} 帧", streamed.frames);
        ensure!(
            streamed.text == expected(&streaming_prompt),
            "增量拼接与完整回复不符：{}",
            streamed.text
        );
        ensure!(
            elapsed >= Duration::from_millis(300),
            "chunk_ms=150 时 {} 帧不应在 {}ms 内读完",
            streamed.frames,
            elapsed.as_millis()
        );
        notes.push(format!(
            "流式：{} 帧拼接一致，chunk_ms=150 耗时 {}ms",
            streamed.frames,
            elapsed.as_millis()
        ));

        // 5) 认证失败：prompt 关键字选中场景（真实 Provider 只改 base_url 时走的也是这条路）。
        let keyword = "mock:auth 请用这个场景";
        let response = post_chat(&client, &server.url("v1/chat/completions"), &chat_messages(keyword)).await?;
        ensure!(
            response.status == 401,
            "认证失败场景应返回 401，实际 {}",
            response.status
        );
        let error: Value = serde_json::from_slice(&response.body).context("认证失败的响应不是 JSON")?;
        ensure!(
            error["error"]["code"] == "invalid_api_key",
            "认证失败的响应体不符：{error}"
        );

        // 6) 上游 5xx：查询参数选中场景。
        let url = format!("{}?scenario=upstream", server.url("v1/chat/completions"));
        let response = post_chat(&client, &url, &chat_messages("上游出错")).await?;
        ensure!(
            response.status == 503,
            "上游 5xx 场景应返回 503，实际 {}",
            response.status
        );

        // 7) 非法 JSON：状态 200，但响应体不是合法 JSON。
        let url = format!("{}?scenario=invalid_json", server.url("v1/chat/completions"));
        let response = post_chat(&client, &url, &chat_messages("非法 JSON")).await?;
        ensure!(
            response.status == 200,
            "非法 JSON 场景应返回 200，实际 {}",
            response.status
        );
        ensure!(
            serde_json::from_slice::<Value>(&response.body).is_err(),
            "非法 JSON 场景的响应体不应能解析：{}",
            response.string()
        );

        // 8) 中途断连：路径前缀选中场景，写出一帧后直接关闭。
        let url = server.url("mock/disconnect/v1/chat/completions");
        let response = stream_chat(&client, &url, &chat_messages("中途断连")).await?;
        let (events, _) = read_sse(response).await?;
        let broken = summarize(&events);
        ensure!(!events.is_empty(), "中途断连场景应至少写出一帧");
        ensure!(!broken.done, "中途断连场景不应写出 [DONE]");

        // 9) 静默 stall：先静默，再关连接，没有 [DONE]。
        let url = format!("{}?scenario=stall&stall_ms=150", server.url("v1/chat/completions"));
        let response = stream_chat(&client, &url, &chat_messages("静默")).await?;
        let (events, elapsed) = read_sse(response).await?;
        ensure!(!summarize(&events).done, "静默 stall 场景不应写出 [DONE]");
        ensure!(
            elapsed >= Duration::from_millis(120),
            "静默 stall 应先停顿 150ms，实际 {}ms",
            elapsed.as_millis()
        );

        // 10) 慢速分块：默认 200ms 间隔，仍然完整收尾。
        let url = format!("{}?scenario=slow", server.url("v1/chat/completions"));
        let response = stream_chat(&client, &url, &chat_messages("慢速分块")).await?;
        let (events, elapsed) = read_sse(response).await?;
        let slow = summarize(&events);
        ensure!(slow.done, "慢速分块场景应以 [DONE] 收尾");
        ensure!(slow.text == expected("慢速分块"), "慢速分块的正文不符：{}", slow.text);
        ensure!(
            elapsed >= Duration::from_millis(400),
            "慢速分块默认 200ms 间隔，实际只用了 {}ms",
            elapsed.as_millis()
        );

        // 11) 客户端中途关闭：Mock 侧的连接关闭计数 +1（取消类断言的数据来源）。
        let before = aborted(&client, &server).await?;
        let url = format!("{}?chunk_ms=50", server.url("v1/chat/completions"));
        let mut response = stream_chat(&client, &url, &chat_messages(&"客户端中途关闭".repeat(8))).await?;
        ensure!(response.next().await.is_some(), "中途关闭前应至少收到一块数据");
        drop(response);
        let after = wait_for_abort(&client, &server, before).await?;
        ensure!(
            after > before,
            "客户端关闭 SSE 后 streamsAborted 未增加：{before} → {after}"
        );
        notes.push(format!("客户端关闭 SSE：streamsAborted {before} → {after}"));

        // 12) 统计汇总：七种场景都被记录，流式收尾计数可读。
        let stats = get_json(&client, &server.url("mock/stats")).await?;
        for scenario in [
            "normal",
            "auth",
            "upstream",
            "invalid_json",
            "disconnect",
            "stall",
            "slow",
        ] {
            ensure!(
                stats["scenarios"][scenario].as_u64().unwrap_or(0) > 0,
                "场景 {scenario} 未被记录：{}",
                stats["scenarios"]
            );
        }
        ensure!(
            stats["streamsCompleted"].as_u64().unwrap_or(0) >= 2,
            "完整流式响应的计数不符：{}",
            stats["streamsCompleted"]
        );
        notes.push(format!(
            "统计：chat={}（json={} / sse={}），completed={}",
            stats["chatRequests"], stats["chatJson"], stats["chatSse"], stats["streamsCompleted"]
        ));

        Ok(notes)
    })
}

/// 一次 chat 请求的 messages：system + 历史 + 本次输入，用于断言顺序。
fn chat_messages(prompt: &str) -> Vec<Value> {
    vec![
        json!({"role": "system", "content": "mock-system-prompt"}),
        json!({"role": "user", "content": "历史输入"}),
        json!({"role": "assistant", "content": "历史回复"}),
        json!({"role": "user", "content": prompt}),
    ]
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

/// 普通（非流式）chat 请求。
async fn post_chat(client: &HttpClient, url: &str, messages: &[Value]) -> Result<HttpResponse> {
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
async fn stream_chat(client: &HttpClient, url: &str, messages: &[Value]) -> Result<HttpStreamResponse> {
    let body = chat_body(messages, true)?;
    client
        .stream_post(url, body)
        .await
        .with_context(|| format!("POST {url}（流式）失败"))
}

/// GET 一个 JSON 响应。
async fn get_json(client: &HttpClient, url: &str) -> Result<Value> {
    let response = client
        .get(HttpRequest::new(url))
        .await
        .with_context(|| format!("GET {url} 失败"))?;
    ensure!(response.status == 200, "GET {url} 应返回 200，实际 {}", response.status);
    let body = response.string();
    serde_json::from_str(&body).with_context(|| format!("GET {url} 的响应不是 JSON：{body}"))
}

/// 读完整条 SSE 响应：事件负载列表与读取耗时。
async fn read_sse(mut response: HttpStreamResponse) -> Result<(Vec<String>, Duration)> {
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
struct Streamed {
    /// 所有 `delta.content` 的拼接。
    text: String,
    /// 带正文的帧数。
    frames: usize,
    /// 是否收到 `[DONE]`。
    done: bool,
}

/// 汇总 SSE 事件。
fn summarize(events: &[String]) -> Streamed {
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
fn expected(prompt: &str) -> String {
    format!("{REPLY_PREFIX}{prompt}")
}

/// 读当前的客户端关闭计数。
async fn aborted(client: &HttpClient, server: &MockServer) -> Result<u64> {
    let stats = get_json(client, &server.url("mock/stats")).await?;
    Ok(stats["streamsAborted"].as_u64().unwrap_or(0))
}

/// 轮询 `/mock/stats` 直到 `streamsAborted` 超过 `before`：FIN 到达 Mock 需要一点时间。
async fn wait_for_abort(client: &HttpClient, server: &MockServer, before: u64) -> Result<u64> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let aborted = aborted(client, server).await?;
        if aborted > before || Instant::now() >= deadline {
            return Ok(aborted);
        }
        sleep(Duration::from_millis(20)).await;
    }
}
