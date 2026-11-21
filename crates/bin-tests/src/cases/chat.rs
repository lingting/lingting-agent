//! 真实多轮对话与 Context 用例（P2-021 / P2-026）。
//!
//! 三条用例都自起服务端与 Mock LLM：断言依赖 `/mock/last-request` 的完整 `messages`，
//! 共享 Mock 会被并发用例互相覆盖。
//!
//! - `chat.multiturn`：增量拼接等于完整回复、历史按序进入请求体、终态与 usage 一致；
//! - `chat.context`：系统提示词在最前、历史居中、本次输入在最后（P2-021）；
//! - `chat.truncate`：`context_max_messages` 压到 2 后，最旧的历史不再进入请求体（P2-021）。

use anyhow::{Context, Result, ensure};
use lib_bin_cli::sdk::{ApiClient, MessageListParams, MessageRole, RunEventKind, RunStatus};
use serde_json::Value;

use crate::case::{Case, CaseFuture, CaseKind};
use crate::cases::llm::mock::MockServer;
use crate::context::TestContext;
use crate::flow::{self, Occupied};
use crate::server::{self, config};

/// 系统提示词内容：`AGENT_HOME/AGENTS.md` 写在用例目录里，每次组装现场读取（P2-020）。
const SYSTEM_PROMPT: &str = "# 测试系统提示词\n\n始终用简体中文回答。";
/// 「消息条数上限压到 2」的用例配置：安全余量调大，命中条数裁剪而不是预算裁剪。
const TRUNCATE_EXTRA: &str = "[agent]\ncontext_max_messages = 2\ncontext_safety_tokens = 7000\n";
/// 多轮对话的轮数。
const ROUNDS: usize = 3;

/// Context 领域用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "chat.context",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: context_messages,
        },
        Case {
            name: "chat.multiturn",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: multiturn,
        },
        Case {
            name: "chat.truncate",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: truncate,
        },
    ]
}

/// 多轮对话：回复拼接、历史进入请求体、终态与 usage（P2-026 第 1 条）。
fn multiturn(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let text = config(mock.base_url(), "");
        server::serve_once(context, &text, multiturn_body(context, &mock)).await
    })
}

/// 多轮对话主体：三轮问答都不带系统提示词（用例目录没有 `AGENTS.md`）。
async fn multiturn_body(context: &TestContext, mock: &MockServer) -> Result<Vec<String>> {
    let mut talk = Occupied::start(context, &format!("多轮 {}", context.token())).await?;
    let mut prompts = Vec::new();
    let mut last_run = String::new();
    for round in 1..=ROUNDS {
        let content = format!("第 {round} 轮 {}", context.token());
        let (sent, drained) = talk.say(context, &content).await?;
        ensure!(
            drained.terminal.event == RunEventKind::RunCompleted,
            "第 {round} 轮应以 run_completed 收尾，实际 {:?}",
            drained.terminal.event
        );
        // Mock 回显最后一条消息：增量拼接必须与它逐字相等。
        let expected = format!("Mock 回复：{content}");
        ensure!(
            drained.content == expected,
            "第 {round} 轮增量拼接与完整回复不一致：{} != {expected}",
            drained.content
        );
        let finished = flow::wait_run(context.client(), &sent.run.id).await?;
        ensure!(
            finished.status == RunStatus::Completed,
            "第 {round} 轮 Run 终态应为 Completed，实际 {:?}",
            finished.status
        );
        ensure!(
            finished.output_tokens.is_some_and(|tokens| tokens > 0),
            "第 {round} 轮完成后应有输出 token，实际 {:?}",
            finished.output_tokens
        );
        last_run = sent.run.id.clone();
        prompts.push(content);
    }

    // 历史进入请求体：最后一条是本次输入，之前按 user/assistant 交替且不含系统提示词。
    let messages = last_request_messages(mock).await?;
    ensure!(
        messages.len() == 2 * ROUNDS - 1,
        "最后一次请求应有 {} 条消息，实际 {}",
        2 * ROUNDS - 1,
        messages.len()
    );
    for (index, prompt) in prompts.iter().enumerate().take(ROUNDS - 1) {
        let user = &messages[index * 2];
        ensure!(
            role_of(user) == "user" && content_of(user) == prompt.as_str(),
            "第 {} 条消息应为用户输入 {prompt}，实际 {user}",
            index * 2
        );
        let assistant = &messages[index * 2 + 1];
        ensure!(
            role_of(assistant) == "assistant" && content_of(assistant) == format!("Mock 回复：{prompt}"),
            "第 {} 条消息应为助手回复，实际 {assistant}",
            index * 2 + 1
        );
    }
    let last = messages.last().context("请求体里没有消息")?;
    ensure!(
        role_of(last) == "user" && content_of(last) == prompts[ROUNDS - 1].as_str(),
        "最后一条消息应为本次输入，实际 {last}"
    );

    // 消息列表：每轮用户 + 助手各一条，顺序与写入一致。
    let listed = context
        .client()
        .message_list(MessageListParams {
            session_id: talk.session.id.clone(),
        })
        .await
        .map_err(lib_bin_cli::client::ClientError::from_call)?;
    ensure!(
        listed.len() == 2 * ROUNDS,
        "会话应有 {} 条消息，实际 {}",
        2 * ROUNDS,
        listed.len()
    );
    ensure!(
        listed
            .iter()
            .filter(|message| message.role == MessageRole::Assistant)
            .count()
            == ROUNDS,
        "助手消息条数不符"
    );

    let session_id = talk.session.id.clone();
    talk.release(context).await?;
    Ok(vec![
        format!("会话：{session_id}，{ROUNDS} 轮对话"),
        format!("最后一次请求：{} 条消息按序进入请求体", messages.len()),
        format!("最后一次 Run：{last_run}，终态 Completed 且 usage 有值"),
        format!("消息列表：{} 条（用户 + 助手交替）", listed.len()),
    ])
}

/// Context 组装顺序：系统提示词 → 历史 → 本次输入（P2-021 第 1 条）。
fn context_messages(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let text = config(mock.base_url(), "");
        server::serve_once(context, &text, context_body(context, &mock)).await
    })
}

/// Context 用例主体：先放 `AGENTS.md`，再跑三轮问答。
async fn context_body(context: &TestContext, mock: &MockServer) -> Result<Vec<String>> {
    std::fs::write(context.home().join("AGENTS.md"), SYSTEM_PROMPT)
        .with_context(|| format!("写入系统提示词失败：{}", context.home().display()))?;
    let mut talk = Occupied::start(context, &format!("Context {}", context.token())).await?;
    let mut prompts = Vec::new();
    for round in 1..=ROUNDS {
        let content = format!("第 {round} 轮 {}", context.token());
        let (_, drained) = talk.say(context, &content).await?;
        ensure!(
            drained.terminal.event == RunEventKind::RunCompleted,
            "第 {round} 轮应以 run_completed 收尾，实际 {:?}",
            drained.terminal.event
        );
        prompts.push(content);
    }

    let messages = last_request_messages(mock).await?;
    ensure!(
        messages.len() == 2 * ROUNDS,
        "带系统提示词时应有 {} 条消息，实际 {}",
        2 * ROUNDS,
        messages.len()
    );
    let first = &messages[0];
    ensure!(role_of(first) == "system", "首条消息应为系统提示词，实际 {first}");
    ensure!(
        content_of(first) == SYSTEM_PROMPT,
        "首条消息内容应等于 AGENTS.md 的内容，实际 {first}"
    );
    let last = messages.last().context("请求体里没有消息")?;
    ensure!(
        role_of(last) == "user" && content_of(last) == prompts[ROUNDS - 1].as_str(),
        "末条消息应为本次输入，实际 {last}"
    );

    talk.release(context).await?;
    Ok(vec![
        format!("系统提示词：来自 {}/AGENTS.md", context.home().display()),
        format!("请求体：{} 条消息，首条为 system，末条为本次输入", messages.len()),
        "历史按 seq 升序居中".to_string(),
    ])
}

/// 裁剪：`context_max_messages = 2` 时最旧的两条历史不再进入请求体（P2-021 第 2 条）。
fn truncate(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let text = config(mock.base_url(), TRUNCATE_EXTRA);
        server::serve_once(context, &text, truncate_body(context, &mock)).await
    })
}

/// 裁剪用例主体：三轮问答，第三次请求只应含最近两条历史与本次输入。
async fn truncate_body(context: &TestContext, mock: &MockServer) -> Result<Vec<String>> {
    let mut talk = Occupied::start(context, &format!("裁剪 {}", context.token())).await?;
    let mut prompts = Vec::new();
    for round in 1..=ROUNDS {
        let content = format!("第 {round} 轮 {}", context.token());
        let (_, drained) = talk.say(context, &content).await?;
        ensure!(
            drained.terminal.event == RunEventKind::RunCompleted,
            "第 {round} 轮应以 run_completed 收尾，实际 {:?}",
            drained.terminal.event
        );
        prompts.push(content);
    }

    let messages = last_request_messages(mock).await?;
    ensure!(
        messages.len() == 3,
        "条数上限为 2 时应保留两条历史 + 本次输入，实际 {} 条：{messages:?}",
        messages.len()
    );
    let kept: Vec<String> = messages.iter().map(content_of).collect();
    ensure!(!kept.contains(&prompts[0]), "最旧的历史不应再进入请求体：{kept:?}");
    ensure!(kept.contains(&prompts[ROUNDS - 2]), "倒数第二条历史应被保留：{kept:?}");
    ensure!(
        kept.last() == Some(&prompts[ROUNDS - 1]),
        "本次输入必须保留在最后：{kept:?}"
    );

    talk.release(context).await?;
    Ok(vec![
        "配置：context_max_messages = 2、context_safety_tokens = 7000".to_string(),
        format!("请求体：{} 条消息，最旧的历史已被裁掉", messages.len()),
        "本次输入与最近两条历史保留".to_string(),
    ])
}

/// 最后一次请求的 `messages` 数组。
async fn last_request_messages(mock: &MockServer) -> Result<Vec<Value>> {
    let request = flow::get_json(&mock.url("/mock/last-request")).await?;
    request
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .context("last-request 应返回完整的 messages 数组")
}

/// 请求体里一条消息的角色。
fn role_of(message: &Value) -> &str {
    message.get("role").and_then(Value::as_str).unwrap_or_default()
}

/// 请求体里一条消息的正文。
///
/// OpenAI 协议允许两种形态：纯字符串（用户 / 助手），以及内容分段数组
/// （Rig 对 `system` 就是这样发的：`[{"type":"text","text":"…"}]`）。两种都要能读。
fn content_of(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    }
}
