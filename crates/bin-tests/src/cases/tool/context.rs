//! Context 裁剪用例（X-1 / X-2）与「超预算即失败」（L-8）。
//!
//! 裁剪单位是「工具调用组」（助手 `tool_calls` + 它的全部工具结果），因此断言只看
//! 最后一次请求的 `messages`：组要么整体在场，要么整体消失，永远不出现半组。

use anyhow::ensure;
use serde_json::json;

use super::{case, serve_with};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::mock::llm::{MockServer, observe};
use crate::util::context::TestContext;
use crate::util::flow::Occupied;

/// 历史条数上限压到 3：只够留下一组「助手调用 + 工具结果」（第 2 轮），更旧的组整段丢弃。
const CONTEXT_EXTRA: &str = "[agent]\ncontext_max_messages = 3\n";
/// 超出模型预算的固定文案（`agent-context-core` 的 `ContextError::PromptTooLarge`）。
const PROMPT_TOO_LARGE: &str = "上下文超出模型预算";

/// Context 相关用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("tool.context", CaseKind::SelfServed, context)]
}

/// 三轮各一次工具调用：最后一次请求里只应留下最新的一组，且组内配对完整。
fn context(test_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        serve_with(test_context, &mock, CONTEXT_EXTRA, async {
            let mut occupied = Occupied::start(test_context, "tool.context").await?;
            for (round, expression) in [(1, "1+1"), (2, "2+2"), (3, "3+3")] {
                let spec = json!({"calls": [{"name": "calculator", "arguments": {"expression": expression}}]});
                occupied
                    .say(test_context, &format!("第 {round} 轮 marker-{round}\ntool:{spec}"))
                    .await?;
            }

            // 最后一条请求就是「裁剪后的 Context」：工具结果作为最后一条消息再次请求模型。
            let request = observe::last_request(&mock).await?;
            let messages = request["messages"].as_array().cloned().unwrap_or_default();
            // 本 Run 自己的轨迹一定在场（请求体 = 裁剪后的历史 + 本次输入 + 本 Run 轨迹），
            // 因此「历史里只留一组」等价于请求里恰好两组：一组来自历史，一组来自本 Run。
            let calls: Vec<&serde_json::Value> = messages
                .iter()
                .filter(|message| message["tool_calls"].as_array().is_some_and(|calls| !calls.is_empty()))
                .collect();
            let results: Vec<&serde_json::Value> =
                messages.iter().filter(|message| message["role"] == "tool").collect();
            ensure!(
                calls.len() == 2 && results.len() == 2,
                "命中上限时请求里应只剩「历史一组 + 本 Run 一组」：{} / {}（{messages:?}）",
                calls.len(),
                results.len()
            );

            // X-1：组内配对完整 —— 每个结果都能在它之前的助手调用里找到 id。
            for (index, result) in messages
                .iter()
                .enumerate()
                .filter(|(_, message)| message["role"] == "tool")
            {
                let id = result["tool_call_id"].as_str().unwrap_or_default();
                let paired = messages[..index].iter().any(|message| {
                    message["tool_calls"]
                        .as_array()
                        .is_some_and(|calls| calls.iter().any(|call| call["id"].as_str() == Some(id)))
                });
                ensure!(paired, "工具结果 `{id}` 找不到它之前的助手调用：{messages:?}");
            }
            for call in messages.iter().filter_map(|message| message["tool_calls"].as_array()) {
                for call in call {
                    let id = call["id"].as_str().unwrap_or_default();
                    ensure!(
                        messages
                            .iter()
                            .any(|message| message["role"] == "tool" && message["tool_call_id"] == id),
                        "助手调用 `{id}` 缺少工具结果：{messages:?}"
                    );
                }
            }

            // X-2：旧组（第 1 轮）整体消失，历史里留下的是第 2 轮那组，本 Run 的是第 3 轮。
            let contents: Vec<&str> = results
                .iter()
                .filter_map(|message| message["content"].as_str())
                .collect();
            ensure!(
                contents == ["4", "6"],
                "保留下来的应是第 2 轮与第 3 轮的结果：{contents:?}"
            );
            for marker in ["marker-1", "marker-2"] {
                ensure!(
                    !messages
                        .iter()
                        .any(|message| message["content"].as_str().is_some_and(|text| text.contains(marker))),
                    "超出条数上限的用户消息 `{marker}` 应被裁掉：{messages:?}"
                );
            }
            ensure!(
                messages.len() == 6,
                "裁剪后应是 3 条历史（含一组轨迹）+ 本次输入 + 本 Run 的 2 条轨迹，实际 {} 条：{messages:?}",
                messages.len()
            );

            // L-8：本次输入本身超过模型预算时直接失败，不静默丢内容。
            let mut too_large = Occupied::start(test_context, "tool.context.too_large").await?;
            let huge = "长".repeat(40_000);
            let (_, drained) = too_large.say(test_context, &huge).await?;
            ensure!(
                drained.terminal.event == lib_bin_cli::sdk::RunEventKind::RunFailed,
                "超预算应以 run_failed 收尾，实际 {:?}",
                drained.terminal.event
            );
            ensure!(
                drained.terminal.error.as_deref() == Some(PROMPT_TOO_LARGE),
                "超预算的文案应为固定值，实际 {:?}",
                drained.terminal.error
            );
            occupied.release(test_context).await?;
            too_large.release(test_context).await?;
            Ok(vec![
                format!(
                    "最后一次请求 {} 条消息：历史留 1 组（结果 4），本 Run 1 组（结果 6）",
                    messages.len()
                ),
                format!("超预算：{PROMPT_TOO_LARGE}"),
            ])
        })
        .await
    })
}
