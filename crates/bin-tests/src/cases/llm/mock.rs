//! `llm.mock`：Mock LLM Server 自身可用性——四个端点、七种场景、分块间隔与连接关闭计数。
//!
//! 这里只验证 Mock 自己（以及它与真实 HTTP 客户端的配合）。「Provider 如何消费这些响应」
//! 由 P2-026 的 `run.execute` / `run.failed_auth` / `run.failed_upstream` / `run.failed_timeout` 断言。
//!
//! 请求构造与 SSE 读取在 [`crate::mock::llm::probe`]，`/mock/*` 的观测读取与等待在
//! [`crate::mock::llm::observe`]（P2.1-010）：本文件只保留步骤与断言。

use crate::case::CaseFuture;
use crate::mock::llm::{MODEL, MockServer, observe, probe};
use crate::util::context::TestContext;
use anyhow::{Context, ensure};
use framework_http::HttpClient;
use serde_json::Value;
use std::time::Duration;

pub(crate) fn mock(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let server = MockServer::start().await?;
        let client = HttpClient::new();
        let mut notes = vec![format!("Mock LLM：{}/v1", server.base_url())];

        // 1) 模型清单：Provider 侧据此判断 Mock 可用。
        let models = observe::get_json(&server.url("v1/models")).await?;
        let ids: Vec<&str> = models["data"]
            .as_array()
            .map(|list| list.iter().filter_map(|model| model["id"].as_str()).collect())
            .unwrap_or_default();
        ensure!(ids.contains(&MODEL), "/v1/models 应包含 {MODEL}，实际 {ids:?}");
        notes.push(format!("模型清单：{ids:?}"));

        // 2) 普通补全：回显最后一条消息，用量有值。
        let prompt = "普通回复";
        let messages = probe::chat_messages(prompt);
        let response = probe::post_chat(&client, &server.url("v1/chat/completions"), &messages).await?;
        ensure!(response.status == 200, "普通补全应返回 200，实际 {}", response.status);
        let completion: Value = serde_json::from_slice(&response.body).context("普通补全的响应不是 JSON")?;
        let content = completion["choices"][0]["message"]["content"]
            .as_str()
            .context("普通补全缺少 choices[0].message.content")?;
        ensure!(content == probe::expected(prompt), "普通补全的正文不符：{content}");
        ensure!(
            completion["usage"]["total_tokens"].as_u64().unwrap_or(0) > 0,
            "普通补全应带用量：{}",
            completion["usage"]
        );

        // 3) last-request：完整 messages 与顺序（Context 类断言的数据来源）。
        let recorded = observe::last_request(&server).await?;
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
        let response = probe::stream_chat(&client, &url, &probe::chat_messages(&streaming_prompt)).await?;
        ensure!(response.status == 200, "流式补全应返回 200，实际 {}", response.status);
        let (events, elapsed) = probe::read_sse(response).await?;
        let streamed = probe::summarize(&events);
        ensure!(streamed.done, "正常场景的流式响应应以 [DONE] 收尾");
        ensure!(streamed.frames > 1, "流式响应应分成多帧，实际 {} 帧", streamed.frames);
        ensure!(
            streamed.text == probe::expected(&streaming_prompt),
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
        let response = probe::post_chat(
            &client,
            &server.url("v1/chat/completions"),
            &probe::chat_messages(keyword),
        )
        .await?;
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
        let response = probe::post_chat(&client, &url, &probe::chat_messages("上游出错")).await?;
        ensure!(
            response.status == 503,
            "上游 5xx 场景应返回 503，实际 {}",
            response.status
        );

        // 7) 非法 JSON：状态 200，但响应体不是合法 JSON。
        let url = format!("{}?scenario=invalid_json", server.url("v1/chat/completions"));
        let response = probe::post_chat(&client, &url, &probe::chat_messages("非法 JSON")).await?;
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
        let response = probe::stream_chat(&client, &url, &probe::chat_messages("中途断连")).await?;
        let (events, _) = probe::read_sse(response).await?;
        let broken = probe::summarize(&events);
        ensure!(!events.is_empty(), "中途断连场景应至少写出一帧");
        ensure!(!broken.done, "中途断连场景不应写出 [DONE]");

        // 9) 静默 stall：先静默，再关连接，没有 [DONE]。
        let url = format!("{}?scenario=stall&stall_ms=150", server.url("v1/chat/completions"));
        let response = probe::stream_chat(&client, &url, &probe::chat_messages("静默")).await?;
        let (events, elapsed) = probe::read_sse(response).await?;
        ensure!(!probe::summarize(&events).done, "静默 stall 场景不应写出 [DONE]");
        ensure!(
            elapsed >= Duration::from_millis(120),
            "静默 stall 应先停顿 150ms，实际 {}ms",
            elapsed.as_millis()
        );

        // 10) 慢速分块：默认 200ms 间隔，仍然完整收尾。
        let url = format!("{}?scenario=slow", server.url("v1/chat/completions"));
        let response = probe::stream_chat(&client, &url, &probe::chat_messages("慢速分块")).await?;
        let (events, elapsed) = probe::read_sse(response).await?;
        let slow = probe::summarize(&events);
        ensure!(slow.done, "慢速分块场景应以 [DONE] 收尾");
        ensure!(
            slow.text == probe::expected("慢速分块"),
            "慢速分块的正文不符：{}",
            slow.text
        );
        ensure!(
            elapsed >= Duration::from_millis(400),
            "慢速分块默认 200ms 间隔，实际只用了 {}ms",
            elapsed.as_millis()
        );

        // 11) 客户端中途关闭：Mock 侧的连接关闭计数 +1（取消类断言的数据来源）。
        let before = observe::aborted(&server).await?;
        let url = format!("{}?chunk_ms=50", server.url("v1/chat/completions"));
        let mut response =
            probe::stream_chat(&client, &url, &probe::chat_messages(&"客户端中途关闭".repeat(8))).await?;
        ensure!(response.next().await.is_some(), "中途关闭前应至少收到一块数据");
        drop(response);
        let after = observe::wait_aborted_over(&server, before).await?;
        ensure!(
            after > before,
            "客户端关闭 SSE 后 streamsAborted 未增加：{before} → {after}"
        );
        notes.push(format!("客户端关闭 SSE：streamsAborted {before} → {after}"));

        // 12) 统计汇总：七种场景都被记录，流式收尾计数可读。
        let stats = observe::stats(&server).await?;
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
