//! 多轮闭环、并行、流式聚合、轮次上限与审计用例。
//!
//! 覆盖判据：S-2 / S-3 / S-4 / S-5 / S-7（`tool.schema` / `tool.loop` / `tool.audit`）、
//! L-1 / L-2 / L-5 / L-6 / L-7、C-1 / C-2 / C-3、S-6（`tool.stream`）。

use std::time::Duration;

use agent_tool::{Tool, ToolContext};
use anyhow::{Context as _, Result, ensure};
use framework_http::{HttpClient, HttpRequest};
use lib_bin_cli::sdk::{ApiClient, MessageRole, RunEventKind, RunGetParams, RunStatus, ToolCallStatus, ToolFileAccess};
use serde_json::{Value, json};

use super::{audit, case, events_of, messages, say, seq_contiguous, serve_with};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::mock::llm::{MockServer, observe};
use crate::util::context::TestContext;
use crate::util::flow::Occupied;

/// `http_request` 显式拒绝、`file_search` 改为需确认：一份配置同时覆盖「deny 不进清单」与
/// 「confirm 仍在清单里」两种过滤口径。
const SCHEMA_EXTRA: &str = r#"[tools.permissions]
http_request = "deny"
file_search = "confirm"
"#;

/// 一轮内并行上限压到 1：同一批调用退化为串行，结果顺序必须不变。
const SERIAL_EXTRA: &str = r#"[tools]
max_parallel_calls = 1
"#;

/// 工具轮次上限压到 1：模型第二轮仍在请求工具时 Run 必须落 `FAILED`。
const ROUNDS_EXTRA: &str = r#"[agent]
max_tool_rounds = 1
"#;

/// 轮次用尽的固定文案（与 `agent-runtime` 的 `TOOL_ROUNDS_EXCEEDED` 逐字一致）。
const ROUNDS_EXCEEDED: &str = "工具调用轮次超过上限";

/// 聚合失败的固定文案：缺 `End` 的回复在 Rig 侧就是「响应被截断」（`LlmError::Truncated`）。
const INCOMPLETE: &str = "调用模型失败：响应不完整";

/// 多轮与并行相关用例。
pub(super) fn cases() -> Vec<Case> {
    vec![
        case("tool.audit", CaseKind::SelfServed, audit_case),
        case("tool.loop", CaseKind::SelfServed, loop_case),
        case("tool.parallel", CaseKind::SelfServed, parallel),
        case("tool.rounds", CaseKind::SelfServed, rounds),
        case("tool.schema", CaseKind::SelfServed, schema),
        case("tool.stream", CaseKind::SelfServed, stream),
    ]
}

/// 一次 calculator 调用：脚本里的参数与期望结果都由用例给出。
fn calculator(expression: &str) -> Value {
    json!({"name": "calculator", "arguments": {"expression": expression}})
}

/// 最近一次请求里的工具清单（`/mock/last-request` 的 `tools` 数组）。
async fn tools_of(mock: &MockServer) -> Result<Vec<Value>> {
    let recorded = observe::last_request(mock).await?;
    Ok(recorded["tools"].as_array().cloned().unwrap_or_default())
}

/// 工具清单里的名字（按请求顺序）。
fn tool_names(tools: &[Value]) -> Vec<String> {
    tools
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_owned))
        .collect()
}

// ---------------------------------------------------------------------------
// tool.schema
// ---------------------------------------------------------------------------

/// 模型可见清单：按名字稳定排序、`deny` 缺席、`confirm` 在场、自述字段不进请求；
/// 同时在同一份 `*Args` 上核对 schema 与 serde 的接受行为一致（S-2 / S-3）。
fn schema(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();

        // 一、清单过滤与稳定排序：同一份配置两次请求产出同一份清单。
        let mock = MockServer::start().await?;
        let (first, second) = serve_with(context, &mock, SCHEMA_EXTRA, async {
            let mut occupied = Occupied::start(context, "tool.schema").await?;
            occupied.say(context, "第一次请求").await?;
            let first = tools_of(&mock).await?;
            occupied.say(context, "第二次请求").await?;
            let second = tools_of(&mock).await?;
            occupied.release(context).await?;
            Ok((first, second))
        })
        .await?;

        let names = tool_names(&first);
        ensure!(
            names == ["calculator", "current_time", "file_read", "file_search"],
            "模型可见清单应按名字排序且摘掉 deny 的工具，实际 {names:?}"
        );
        ensure!(first == second, "同一份配置两次请求的工具清单必须同序同内容");
        notes.push(format!("模型可见清单：{names:?}（两次一致）"));

        // 二、自述字段（风险等级与本地文件操作声明）不进发给模型的定义。
        let recorded = observe::last_request(&mock).await?;
        let text = recorded.to_string();
        for field in ["fileAccess", "file_access", "risk"] {
            ensure!(!text.contains(field), "请求里不应出现工具自述字段 `{field}`");
        }
        notes.push("请求里不含 fileAccess / risk".to_string());

        // 三、schema 与校验同源：calculator 的必填字段由同一份 `*Args` 派生。
        let calculator = agent_tool::builtin::Calculator;
        ensure!(
            calculator.schema().as_value()["required"] == json!(["expression"]),
            "calculator 的 schema 应把 expression 声明为必填：{}",
            calculator.schema().as_value()
        );
        ensure!(
            calculator.file_access() == agent_tool::ToolFileAccess::None,
            "calculator 不操作本地文件"
        );
        let tool_context = ToolContext {
            home: context.home().to_path_buf(),
            session_id: "s".to_string(),
            run_id: "r".to_string(),
            tool_call_id: "c".to_string(),
            deadline: std::time::Instant::now() + Duration::from_secs(5),
        };
        let error = calculator
            .call(&tool_context, json!({}))
            .await
            .err()
            .context("缺必填字段应被拒绝")?;
        ensure!(
            error.to_string().contains("参数不合法：") && error.to_string().contains("expression"),
            "缺必填字段的错误应点名字段：{error}"
        );
        notes.push(format!("schema / serde 同源：{}", error));

        Ok(notes)
    })
}

// ---------------------------------------------------------------------------
// tool.loop
// ---------------------------------------------------------------------------

/// 两轮闭环：第一轮要求调用 `calculator`，第二轮给出终答；轨迹按轮落库、事件 `seq` 连续。
fn loop_case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        serve_with(context, &mock, "", async {
            let mut occupied = Occupied::start(context, "tool.loop").await?;
            let (sent, drained) = say(&mut occupied, context, json!({"calls": [calculator("(2+3)*4")]})).await?;

            // 终态与正文：第二轮回复回显最后一条消息，也就是工具结果。
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "两轮闭环应以 run_completed 收尾，实际 {:?}",
                drained.terminal.event
            );
            ensure!(
                drained.content.contains("20"),
                "终答应基于工具结果（20），实际正文：{}",
                drained.content
            );

            // 事件契约：seq 连续、工具事件成对、字段位置不变。
            ensure!(seq_contiguous(&drained.events), "事件 seq 应从 0 连续递增");
            let started = events_of(&drained, RunEventKind::ToolCallStarted);
            let completed = events_of(&drained, RunEventKind::ToolCallCompleted);
            ensure!(
                started.len() == 1 && completed.len() == 1,
                "一次工具调用应各有一条开始与结束事件：{} / {}",
                started.len(),
                completed.len()
            );
            let (started, completed) = (started[0], completed[0]);
            ensure!(
                started.tool_call_id == completed.tool_call_id && started.tool_name.as_deref() == Some("calculator"),
                "开始与结束事件应按 toolCallId 配对：{:?} / {:?}",
                started.tool_call_id,
                completed.tool_call_id
            );
            ensure!(
                completed.tool_status == Some(ToolCallStatus::Ok),
                "工具结束状态应为 OK，实际 {:?}（{}）",
                completed.tool_status,
                completed.error.clone().unwrap_or_default()
            );
            ensure!(
                completed.tool_duration_ms.is_some() && started.tool_round == Some(1),
                "结束事件带耗时、轮次从 1 开始"
            );
            // 三个工具事件的 wire 值固定为 snake_case（SSE 的 event 行与 JSON 同源）。
            for (kind, name) in [
                (RunEventKind::ToolCallStarted, "tool_call_started"),
                (RunEventKind::ToolCallCompleted, "tool_call_completed"),
                (RunEventKind::ToolCallConfirmRequired, "tool_call_confirm_required"),
            ] {
                ensure!(
                    serde_json::to_value(kind)? == Value::String(name.to_owned()),
                    "工具事件的 wire 值应为 {name}"
                );
            }

            // 轨迹落库：中间轮 assistant 带 toolCalls，TOOL 消息带 toolCallId 与工具名。
            let stored = messages(context, &occupied.session.id).await?;
            let assistant = stored
                .iter()
                .find(|message| message.tool_calls.is_some())
                .with_context(|| format!("历史里应有带 toolCalls 的 assistant 消息：{stored:?}"))?;
            let calls = assistant.tool_calls.clone().unwrap_or_default();
            ensure!(
                calls.len() == 1 && calls[0].id == started.tool_call_id.clone().unwrap_or_default(),
                "assistant 的 toolCalls 应与事件同源：{calls:?}"
            );
            let tool_message = stored
                .iter()
                .find(|message| message.role == MessageRole::Tool)
                .context("历史里应有 TOOL 消息")?;
            ensure!(
                tool_message.tool_call_id.as_deref() == started.tool_call_id.as_deref()
                    && tool_message.tool_name.as_deref() == Some("calculator")
                    && tool_message.content.contains("20"),
                "TOOL 消息应按 toolCallId 配对并携带结果：{tool_message:?}"
            );
            ensure!(
                assistant.created_at_ms <= tool_message.created_at_ms,
                "同一轮的 assistant 调用不应晚于它的 TOOL 结果"
            );

            // 第二轮请求携带完整轨迹（L-6）：assistant `tool_calls` + 配对的 tool 结果。
            let second = observe::last_request(&mock).await?;
            let history = second["messages"].as_array().cloned().unwrap_or_default();
            ensure!(
                history
                    .iter()
                    .any(|message| message["tool_calls"].as_array().is_some_and(|calls| !calls.is_empty())),
                "第二轮请求应携带 assistant 的 tool_calls：{history:?}"
            );
            ensure!(
                history.iter().any(|message| message["role"] == "tool"
                    && message["tool_call_id"] == started.tool_call_id.clone().unwrap_or_default()),
                "第二轮请求应携带配对的工具结果消息：{history:?}"
            );
            ensure!(
                second["messages"]
                    .as_array()
                    .is_some_and(|messages| messages.last().is_some_and(|last| last["role"] == "tool")),
                "工具结果必须是第二轮请求的最后一条消息"
            );

            occupied.release(context).await?;
            Ok(vec![
                format!(
                    "工具 {} 一轮完成，终答：{}",
                    started.tool_name.clone().unwrap_or_default(),
                    drained.content
                ),
                format!("历史 {} 条，第二轮请求携带 tool_calls 与工具结果", stored.len()),
                format!("Run {} 落 {}", sent.run.id, "completed"),
            ])
        })
        .await
    })
}

// ---------------------------------------------------------------------------
// tool.parallel
// ---------------------------------------------------------------------------

/// 一轮两个调用：开始事件先于所有结束事件，结果顺序与调用顺序一致；串行时顺序不变。
fn parallel(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();
        for (label, extra) in [("默认并发", ""), ("max_parallel_calls = 1", SERIAL_EXTRA)] {
            let mock = MockServer::start().await?;
            let note = serve_with(context, &mock, extra, async {
                let mut occupied = Occupied::start(context, "tool.parallel").await?;
                let (_, drained) = say(
                    &mut occupied,
                    context,
                    json!({"calls": [calculator("1+1"), calculator("2+2")]}),
                )
                .await?;
                ensure!(
                    drained.terminal.event == RunEventKind::RunCompleted,
                    "{label}：Run 应以 run_completed 收尾"
                );

                // 同轮并行：两条开始事件都在任一结束事件之前。
                let mut started = Vec::new();
                let mut first_completed = None;
                let mut completed_ids = Vec::new();
                for (index, event) in drained.events.iter().enumerate() {
                    match event.event {
                        RunEventKind::ToolCallStarted => started.push(index),
                        RunEventKind::ToolCallCompleted => {
                            first_completed.get_or_insert(index);
                            completed_ids.push(event.tool_call_id.clone().unwrap_or_default());
                        }
                        _ => {}
                    }
                }
                ensure!(started.len() == 2, "{label}：应有两条开始事件，实际 {}", started.len());
                let first_completed = first_completed.context("缺少结束事件")?;
                ensure!(
                    started.iter().all(|index| *index < first_completed),
                    "{label}：两条开始事件都应在任一结束事件之前：{started:?} vs {first_completed}"
                );

                // 结果顺序 == 模型的调用顺序（Mock 按脚本顺序编号）。
                let expected = vec!["call_mock_0".to_string(), "call_mock_1".to_string()];
                ensure!(
                    completed_ids == expected,
                    "{label}：结束事件顺序应与调用顺序一致：{completed_ids:?} != {expected:?}"
                );

                // 结果回填到下一轮请求：两段结果都按同一顺序出现。
                let second = observe::last_request(&mock).await?;
                let contents: Vec<String> = second["messages"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter(|message| message["role"] == "tool")
                    .filter_map(|message| message["content"].as_str().map(str::to_owned))
                    .collect();
                ensure!(
                    contents.len() == 2 && contents[0] == "2" && contents[1] == "4",
                    "{label}：工具结果应按调用顺序回填：{contents:?}"
                );
                occupied.release(context).await?;
                Ok(format!("{label}：2 个调用按序完成 {completed_ids:?}"))
            })
            .await?;
            notes.push(note);
        }
        Ok(notes)
    })
}

// ---------------------------------------------------------------------------
// tool.stream
// ---------------------------------------------------------------------------

/// 流式聚合：参数分片拼成完整 JSON；缺 `End` 落 `FAILED`；纯文本流与 P2 行为一致。
fn stream(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();

        // 一、三段参数分片：审计里的参数应是完整 JSON，结果按工具语义执行。
        let mock = MockServer::start().await?;
        let note = serve_with(context, &mock, "", async {
            let mut occupied = Occupied::start(context, "tool.stream").await?;
            let (sent, drained) = say(
                &mut occupied,
                context,
                json!({"calls": [calculator("6*7")], "fragments": 3}),
            )
            .await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "分片参数应被拼成完整 JSON 并正常执行"
            );
            let records = audit(context, &sent.run.id).await?;
            ensure!(records.len() == 1, "应有一条审计记录，实际 {}", records.len());
            ensure!(
                records[0].arguments.contains("\"expression\"") && records[0].arguments.contains("6*7"),
                "分片参数应拼成完整 JSON：{}",
                records[0].arguments
            );
            ensure!(
                records[0].status == ToolCallStatus::Ok && records[0].result.as_deref() == Some("42"),
                "6*7 应得到 42，实际 {:?}",
                records[0].result
            );
            occupied.release(context).await?;
            Ok(format!("三段参数拼成 {}", records[0].arguments))
        })
        .await?;
        notes.push(note);

        // 二、缺 End：Rig 侧判为请求无效，Run 落 FAILED，且没有半截参数被交给工具。
        let missing = MockServer::start().await?;
        let note = serve_with(context, &missing, "", async {
            let mut occupied = Occupied::start(context, "tool.stream.missing_end").await?;
            let (sent, drained) = say(
                &mut occupied,
                context,
                json!({"calls": [calculator("6*7")], "fragments": 3, "missingEnd": true}),
            )
            .await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunFailed,
                "缺 End 应让 Run 落 run_failed，实际 {:?}",
                drained.terminal.event
            );
            ensure!(
                drained.terminal.error.as_deref() == Some(INCOMPLETE),
                "缺 End 的失败文案应为固定值，实际 {:?}",
                drained.terminal.error
            );
            ensure!(
                audit(context, &sent.run.id).await?.is_empty(),
                "缺 End 时不应有半截参数被执行（审计应为空）"
            );
            occupied.release(context).await?;
            Ok(format!("缺 End：{}", INCOMPLETE))
        })
        .await?;
        notes.push(note);

        // 三、纯文本流：事件序列与 P2 一致（无工具事件）。
        let plain = MockServer::start().await?;
        let note = serve_with(context, &plain, "", async {
            let mut occupied = Occupied::start(context, "tool.stream.plain").await?;
            let (_, drained) = occupied.say(context, "纯文本流").await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "纯文本流应以 run_completed 收尾"
            );
            ensure!(seq_contiguous(&drained.events), "纯文本流的事件 seq 应连续");
            ensure!(
                drained.events.iter().all(|event| !matches!(
                    event.event,
                    RunEventKind::ToolCallStarted
                        | RunEventKind::ToolCallCompleted
                        | RunEventKind::ToolCallConfirmRequired
                )),
                "没有工具调用时不应产出工具事件：{:?}",
                drained.events.iter().map(|event| event.event).collect::<Vec<_>>()
            );
            ensure!(
                drained.content == "Mock 回复：纯文本流",
                "纯文本流的正文应与 P2 一致：{}",
                drained.content
            );
            occupied.release(context).await?;
            Ok("纯文本流：无工具事件，正文与 P2 一致".to_string())
        })
        .await?;
        notes.push(note);

        Ok(notes)
    })
}

// ---------------------------------------------------------------------------
// tool.rounds
// ---------------------------------------------------------------------------

/// 轮次上限：`max_tool_rounds = 1` 时第二轮不再请求模型，Run 直接落 `FAILED`。
fn rounds(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        serve_with(context, &mock, ROUNDS_EXTRA, async {
            let mut occupied = Occupied::start(context, "tool.rounds").await?;
            let (sent, drained) = say(
                &mut occupied,
                context,
                json!({"calls": [calculator("1+1")], "repeat": true}),
            )
            .await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunFailed,
                "轮次用尽应落 run_failed，实际 {:?}",
                drained.terminal.event
            );
            ensure!(
                drained.terminal.error.as_deref() == Some(ROUNDS_EXCEEDED),
                "轮次用尽的文案应为固定值，实际 {:?}",
                drained.terminal.error
            );
            let run = context
                .client()
                .run_get(RunGetParams {
                    run_id: sent.run.id.clone(),
                })
                .await
                .map_err(lib_bin_cli::client::ClientError::from_call)?;
            ensure!(
                run.status == RunStatus::Failed && run.error.as_deref() == Some(ROUNDS_EXCEEDED),
                "run/get 应落 failed + 固定文案，实际 {:?} / {:?}",
                run.status,
                run.error
            );
            // 失败 Run 不写最终助手消息：历史里只有中间轮的轨迹。
            let stored = messages(context, &occupied.session.id).await?;
            ensure!(
                stored
                    .iter()
                    .all(|message| message.role != MessageRole::Assistant || message.tool_calls.is_some()),
                "轮次用尽不应写最终助手消息：{stored:?}"
            );
            occupied.release(context).await?;
            Ok(vec![format!("第一轮工具执行后第二轮被拦：{ROUNDS_EXCEEDED}")])
        })
        .await
    })
}

// ---------------------------------------------------------------------------
// tool.audit
// ---------------------------------------------------------------------------

/// 一次成功 + 一次「参数里带 api_key 的非法调用」：条数、状态、耗时、文件声明与脱敏逐项核对。
fn audit_case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        serve_with(context, &mock, "", async {
            let mut occupied = Occupied::start(context, "tool.audit").await?;
            let (sent, drained) = say(
                &mut occupied,
                context,
                json!({"calls": [
                    calculator("1+1"),
                    {"name": "calculator", "arguments": {"expression": "1+1", "api_key": "sk-audit-plaintext"}},
                ]}),
            )
            .await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "参数非法也是工具结果，Run 仍应完成"
            );
            let started = events_of(&drained, RunEventKind::ToolCallStarted).len();
            let records = audit(context, &sent.run.id).await?;
            ensure!(
                records.len() == started && records.len() == 2,
                "审计条数应等于开始事件条数：{} != {}",
                records.len(),
                started
            );
            let started_ids: Vec<String> = events_of(&drained, RunEventKind::ToolCallStarted)
                .iter()
                .filter_map(|event| event.tool_call_id.clone())
                .collect();
            let audited_ids: Vec<String> = records.iter().map(|record| record.tool_call_id.clone()).collect();
            ensure!(
                started_ids == audited_ids,
                "审计记录应与开始事件一一对应：{audited_ids:?} != {started_ids:?}"
            );
            ensure!(
                records.iter().all(|record| record.file_access == ToolFileAccess::None
                    && record.tool_round == 1
                    && record.duration_ms >= 0),
                "审计应带文件声明与轮次（且耗时不小于 0）：{records:?}"
            );
            ensure!(
                records.iter().all(|record| record.created_at_ms > 0)
                    && records
                        .windows(2)
                        .all(|pair| pair[0].created_at_ms <= pair[1].created_at_ms),
                "审计记录应按时间升序：{records:?}"
            );
            ensure!(
                records[0].status == ToolCallStatus::Ok,
                "第一次调用应为 OK，实际 {:?}",
                records[0].status
            );
            ensure!(
                records[1].status == ToolCallStatus::InvalidArguments,
                "第二次调用应为 INVALID_ARGUMENTS，实际 {:?}",
                records[1].status
            );
            // 脱敏在写出侧完成：审计里只有 `***`，明文不出现在任何响应里。
            ensure!(
                records[1].arguments.contains("\"api_key\":\"***\""),
                "命中的键值应替换为 ***：{}",
                records[1].arguments
            );
            ensure!(
                !records
                    .iter()
                    .any(|record| record.arguments.contains("sk-audit-plaintext")),
                "明文不应出现在审计里：{records:?}"
            );
            ensure!(
                !drained.events.iter().any(|event| event
                    .tool_arguments
                    .as_deref()
                    .is_some_and(|text| text.contains("sk-audit-plaintext"))),
                "明文不应出现在事件里"
            );

            // wire 形态：i64 字段是字符串（与其余 i64 约定一致）。
            let url = format!("{}/api/tool/audit/list?runId={}", context.base_url(), sent.run.id);
            let response = HttpClient::new()
                .get(HttpRequest::new(url))
                .await
                .context("查询审计接口失败")?;
            ensure!(response.status == 200, "审计接口应返回 200，实际 {}", response.status);
            let envelope: Value = serde_json::from_str(&response.string()).context("审计响应不是 JSON")?;
            let rows = envelope["data"].as_array().cloned().unwrap_or_default();
            ensure!(rows.len() == records.len(), "接口与 SDK 的审计条数应一致");
            ensure!(
                rows.iter()
                    .all(|row| row["durationMs"].is_string() && row["createdAtMs"].is_string()),
                "i64 字段在 wire 上应是字符串：{rows:?}"
            );

            // 未知 Run：空列表，不返回 404。
            ensure!(
                audit(context, "run-not-exist").await?.is_empty(),
                "未知 runId 应返回空列表"
            );
            occupied.release(context).await?;
            Ok(vec![
                format!(
                    "审计 {} 条，状态 {:?}",
                    records.len(),
                    records.iter().map(|record| record.status).collect::<Vec<_>>()
                ),
                "脱敏：api_key -> ***".to_string(),
            ])
        })
        .await
    })
}
