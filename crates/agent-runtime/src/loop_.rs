//! 单轮 Agent Loop（P2-022）：排队 → Context → LLM 流 → 收尾。
//!
//! 时序固定（D-P2-008 / D-P2-029）：
//!
//! 1. 取全局并发许可（排队超时即落 `FAILED`，不发起上游请求）；
//! 2. 推进 `QUEUED → RUNNING`：排队期间 Run 停在 `QUEUED`，`RUNNING` 才代表正在执行；
//! 3. 组装 Context：模型预算 + 系统提示词 + 历史 + 本次输入，超预算落 `FAILED`；
//! 4. 发 `run_started`（携带助手消息 ID 与估算输入 token）；
//! 5. 逐段消费 LLM 流：增量即事件，流式期间不写库；
//! 6. 成功 → 助手消息 + `COMPLETED` 同事务提交，提交后才发 `run_completed`；
//!    失败 → `FAILED` + `run_failed`；取消 → `run_cancelled` / `server_shutdown`。
//!
//! 取消一律 `select!` 命中后 drop 模型流：连接随之关闭，不留下继续烧 token 的后台任务。

use agent_context_core::ContextPolicy;
use agent_core::session::{Message, MessageRole, Run, RunEventKind, RunStatus};
use agent_route::{LlmError, LlmMessage, LlmRequest, LlmStreamEvent};
use lib_core::{id, time};
use service_core::session::{RunInput, RunOutcome, RunStop};
use tracing::{info, warn};

use crate::error;
use crate::feed::Feed;
use crate::limit::{self, Acquired};
use crate::port::AgentRuntime;

/// 一次 Run 的执行主体。
pub(crate) async fn drive(runtime: &AgentRuntime, input: RunInput) -> RunOutcome {
    let RunInput {
        run,
        prompt,
        events,
        cancel,
    } = input;
    let mut feed = Feed::new(&run, &events);

    // 一、全局并发上限：许可持有到本函数结束，代表「这个 Run 正在占用一个并发位」。
    let _permit = match limit::acquire(&runtime.limit, runtime.agent.run_queue_timeout_ms, &cancel).await {
        Acquired::Permit(permit) => permit,
        Acquired::Timeout => return fail(runtime, &run, feed, error::QUEUE_TIMEOUT).await,
        Acquired::Cancelled => return cancelled(runtime, &run, feed, cancel.stop()).await,
    };

    // 二、推进到 RUNNING：拿到许可之后才算「执行中」（P2-025），排队期间仍停在 QUEUED。
    match runtime.service.start_run(&run.id).await {
        Ok(true) => {}
        Ok(false) => {
            // 排队期间被取消 / 已被别的路径落成终态：不产出任何事件。
            info!(run_id = %run.id, "Run 已不在排队中，跳过执行");
            return RunOutcome::Cancelled {
                reason: error::CANCEL_FALLBACK.to_string(),
            };
        }
        Err(error) => {
            warn!(run_id = %run.id, %error, "推进 Run 到执行中失败");
            return RunOutcome::SessionGone;
        }
    }
    // 三、Context：模型预算、系统提示词、历史（不含本次输入）与本次输入。
    let prepared = match prepare(runtime, &run, &prompt).await {
        Ok(prepared) => prepared,
        Err(public_error) => return fail(runtime, &run, feed, public_error).await,
    };

    // 四、run_started：助手消息 ID 先定，成功收尾时按它写库。
    let assistant_message_id = match id::next_id() {
        Ok(id) => id,
        Err(error) => {
            warn!(run_id = %run.id, %error, "生成助手消息 ID 失败");
            return fail(runtime, &run, feed, error::STORAGE).await;
        }
    };
    let mut started = feed.event(RunEventKind::RunStarted);
    started.status = Some(RunStatus::Running);
    started.message_id = Some(assistant_message_id.clone());
    started.input_tokens = Some(prepared.input_tokens);
    if !feed.send(started).await {
        return RunOutcome::SessionGone;
    }

    // 五、调用模型：未配置 Provider 时按脱敏文案落 FAILED，不发上游请求。
    let Some(llm) = runtime.llm.clone() else {
        return fail(runtime, &run, feed, LlmError::Unconfigured.public_message()).await;
    };
    let request = LlmRequest {
        model: run.model.clone(),
        messages: prepared.messages,
        // 工具定义由 Batch D 的工具循环提供：Batch C 只把「能带工具、能收到调用」表达进契约。
        tools: Vec::new(),
        max_output_tokens: Some(prepared.max_output_tokens),
    };
    let mut stream = match llm.stream(request).await {
        Ok(stream) => stream,
        Err(error) => return fail(runtime, &run, feed, error.public_message()).await,
    };

    // 六、逐段消费：取消命中即 drop 流，取消原因取数据库里的权威值。
    let mut content = String::new();
    let mut usage = None;
    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                drop(stream);
                return cancelled(runtime, &run, feed, cancel.stop()).await;
            }
            item = stream.next() => match item {
                Some(Ok(LlmStreamEvent::Delta { text })) => {
                    content.push_str(&text);
                    let mut delta = feed.event(RunEventKind::MessageDelta);
                    delta.delta = Some(text);
                    if !feed.send(delta).await {
                        return RunOutcome::SessionGone;
                    }
                }
                Some(Ok(LlmStreamEvent::Usage { input_tokens, output_tokens })) => {
                    usage = Some((input_tokens, output_tokens));
                }
                // 工具调用由 Batch D 的多轮循环接管：Batch C 的请求不带工具定义，不会出现。
                Some(Ok(LlmStreamEvent::ToolCall { .. })) | Some(Ok(LlmStreamEvent::ToolCallsDone)) => {}
                Some(Ok(LlmStreamEvent::Done)) | None => break,
                Some(Err(error)) => {
                    drop(stream);
                    return fail(runtime, &run, feed, error.public_message()).await;
                }
            }
        }
    }
    // 流读完即释放：连接与缓冲区不再需要。
    drop(stream);

    // 七、成功收尾：先落库，提交成功后才发 run_completed。
    complete(runtime, &run, feed, assistant_message_id, content, usage).await
}

/// Context 组装结果：请求体 + 本次请求的输出上限。
struct Prepared {
    /// 按发送顺序排好的消息：系统提示词（存在时）→ 历史 → 本次输入。
    messages: Vec<LlmMessage>,
    /// 本次请求的输出上限（模型配置的 `max_output_tokens`）。
    max_output_tokens: u64,
    /// `run_started` 里的输入 token：Context 的估算值（P2-014 第 4 条）。
    input_tokens: i64,
}

/// 组装 Context：任何一步失败都返回可直接写进 `runs.error` 的脱敏文案。
async fn prepare(runtime: &AgentRuntime, run: &Run, prompt: &str) -> Result<Prepared, String> {
    let model = runtime
        .provider
        .model(Some(&run.model))
        .map_err(|error| error.public_message())?;
    let session = runtime
        .service
        .session_summary(&run.session_id)
        .await
        .map_err(|error| logged(error::STORAGE, error))?;
    let history = runtime
        .service
        .history_before_run(&run.session_id, &run.id)
        .await
        .map_err(|error| logged(error::STORAGE, error))?;
    let policy = ContextPolicy {
        context_window: model.context_window,
        max_output_tokens: model.max_output_tokens,
        safety_tokens: runtime.agent.context_safety_tokens,
        max_messages: runtime.agent.context_max_messages,
    };
    let context = runtime
        .builder
        .build(&session, &history, prompt, &policy, &run.model)
        .map_err(|error| logged(error.to_string(), error))?;
    Ok(Prepared {
        messages: context
            .items()
            .iter()
            .map(|item| LlmMessage::text(item.role, item.content.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| logged(error.public_message(), error))?,
        max_output_tokens: model.max_output_tokens as u64,
        input_tokens: context.metadata().estimated_tokens as i64,
    })
}

/// 成功收尾：助手消息与 `COMPLETED` 同事务提交；条件更新影响 0 行即静默退出（D-P2-009）。
async fn complete(
    runtime: &AgentRuntime,
    run: &Run,
    mut feed: Feed<'_>,
    assistant_message_id: String,
    content: String,
    usage: Option<(i64, i64)>,
) -> RunOutcome {
    // token 用量用 Provider 的真实 usage；缺一项就留空，不构造估算值（P2-014 第 4 条）。
    let (input_tokens, output_tokens) = match usage {
        Some((input, output)) => (Some(input), Some(output)),
        None => (None, None),
    };
    let message = Message {
        id: assistant_message_id.clone(),
        session_id: run.session_id.clone(),
        run_id: Some(run.id.clone()),
        role: MessageRole::Assistant,
        content,
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
        model: Some(run.model.clone()),
        input_tokens,
        output_tokens,
        created_at_ms: time::now_millis(),
    };
    match runtime
        .service
        .complete_run(&run.id, &message, input_tokens, output_tokens)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            // Run 已被取消或已终态：不补终态事件，助手消息也不会写入。
            return RunOutcome::Cancelled {
                reason: reason_of(runtime, run).await,
            };
        }
        Err(error) => {
            warn!(run_id = %run.id, %error, "写入助手消息失败，Run 停留在执行中状态");
            return RunOutcome::SessionGone;
        }
    }

    let mut completed = feed.event(RunEventKind::RunCompleted);
    completed.status = Some(RunStatus::Completed);
    completed.message_id = Some(assistant_message_id);
    completed.input_tokens = input_tokens;
    completed.output_tokens = output_tokens;
    // Run 已是终态：会话此刻停止也不影响记录，发送失败不算错误。
    let _ = feed.send(completed).await;
    RunOutcome::Completed
}

/// 失败收尾：`FAILED` 条件更新成功才发 `run_failed`。
async fn fail(runtime: &AgentRuntime, run: &Run, mut feed: Feed<'_>, public_error: impl Into<String>) -> RunOutcome {
    let public_error = public_error.into();
    match runtime.service.fail_run(&run.id, &public_error).await {
        Ok(true) => {}
        Ok(false) => {
            // 已被取消 / 已终态：本次失败不再产出终态事件（D-P2-009 第 2 条）。
            info!(run_id = %run.id, "Run 已不在执行中，跳过 run_failed");
            return RunOutcome::Cancelled { reason: public_error };
        }
        Err(error) => {
            warn!(run_id = %run.id, %error, "标记 Run 失败时写库失败");
            return RunOutcome::SessionGone;
        }
    }
    let mut failed = feed.event(RunEventKind::RunFailed);
    failed.status = Some(RunStatus::Failed);
    failed.error = Some(public_error.clone());
    let _ = feed.send(failed).await;
    RunOutcome::Failed { public_error }
}

/// 取消收尾：停止类别决定终态事件类型，原因取数据库里的权威文本（D-P2-029）。
async fn cancelled(runtime: &AgentRuntime, run: &Run, mut feed: Feed<'_>, stop: RunStop) -> RunOutcome {
    let reason = reason_of(runtime, run).await;
    let mut event = feed.event(kind_of(stop));
    event.status = Some(RunStatus::Cancelled);
    event.error = Some(reason.clone());
    let _ = feed.send(event).await;
    RunOutcome::Cancelled { reason }
}

/// 取消原因：取消方先写库再触发信号，数据库里的 `error` 就是权威文本（D-P2-010 / D-P2-029）。
async fn reason_of(runtime: &AgentRuntime, run: &Run) -> String {
    match runtime.service.run_error(&run.id).await {
        Ok(Some(reason)) => reason,
        Ok(None) => error::CANCEL_FALLBACK.to_string(),
        Err(error) => {
            warn!(run_id = %run.id, %error, "读取取消原因失败，使用兜底文案");
            error::CANCEL_FALLBACK.to_string()
        }
    }
}

/// 停止类别 → 终态事件类型：只有服务关闭落 `server_shutdown`。
fn kind_of(stop: RunStop) -> RunEventKind {
    match stop {
        RunStop::Cancelled => RunEventKind::RunCancelled,
        RunStop::Shutdown => RunEventKind::ServerShutdown,
    }
}

/// 对外文案固定，原始错误只进日志。
fn logged(public: impl Into<String>, error: impl std::fmt::Display) -> String {
    warn!(%error, "Run 执行前置步骤失败");
    public.into()
}
