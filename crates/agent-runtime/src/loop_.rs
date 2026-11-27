//! 多轮 Agent Loop（P3-014）：排队 → Context → 每轮「请求 + 工具」 → 收尾。
//!
//! 时序固定（D-P2-008 / D-P2-029 / D-P3-024）：
//!
//! 1. 取全局并发许可（排队超时即落 `FAILED`，不发起上游请求）；
//! 2. 推进到 `RUNNING`：正常 Run 从 `QUEUED` 推进；中断恢复的 Run 从 `PAUSED` 推进并在同一语句里
//!    累加恢复次数（D-P6-006），两者都是条件更新，影响 0 行即静默放弃（不产事件）；
//! 3. 组装 Context：模型预算 + 系统提示词 + 历史 + 本次输入，超预算落 `FAILED`；
//! 4. 发 `run_started`（携带助手消息 ID 与估算输入 token；恢复的 Run 带中断原因）；
//!    正常路径随后写起点检查点（`MODEL_DONE`）——恢复路径不补写，它没有新的已提交边界；
//! 5. 恢复本 Run 已落库的工具轨迹：确认唤醒先收尾挂起的工具组（批准执行 / 驳回回灌）；
//!    中断恢复则把未配对的调用**一律不重跑**，改为固定通知文本回灌（D-P6-007）；
//! 6. 逐轮：请求 Provider（带工具清单）→ 增量即事件 → 有工具调用则执行并按轮落库，
//!    提交后才写检查点（见 [`checkpoint`]）；工具调用执行前先写「开始标记」；
//! 7. 没有工具调用的那一轮 → 助手消息 + `COMPLETED` 同事务提交，提交后才发 `run_completed`。
//!
//! 结构性问题才落 `FAILED`（[`fail`] 的调用点即清单）：Provider 错误、Context 超预算、
//! 轮次用尽、工具轨迹落库失败、轨迹配对损坏。参数非法 / 超时 / 执行失败 / 权限拒绝都是
//! **工具结果**，以 TOOL 消息回灌模型后继续下一轮（P3-016）。
//!
//! 取消一律 `select!` 命中后 drop 模型流或工具批次：连接随之关闭，不留下继续烧 token
//! 或继续执行工具的后台任务。

pub(crate) mod checkpoint;
mod request;
mod tool;
mod trace;

use std::time::Duration;

use agent_context_core::{ContextItem, ContextPolicy};
use agent_core::session::{CheckpointPhase, LlmToolCallVO, Message, MessageRole, Run, RunEventKind, RunStatus};
use agent_route::{LlmError, LlmMessage, LlmToolCall};
use lib_core::{id, time};
use service_core::session::{RunCancel, RunInput, RunOutcome, RunStop};
use tracing::{info, warn};

use crate::error;
use crate::feed::Feed;
use crate::limit::{self, Acquired};
use crate::metrics;
use crate::port::AgentRuntime;

use tool::{Resumed, Step};

/// 一次 Run 的执行主体。
pub(crate) async fn drive(runtime: &AgentRuntime, input: RunInput) -> RunOutcome {
    let RunInput {
        run,
        prompt,
        resume,
        events,
        cancel,
    } = input;
    let mut feed = Feed::new(&run, &events);

    // 一、全局并发上限：许可持有到本函数结束，代表「这个 Run 正在占用一个并发位」。
    let _permit = match limit::acquire(&runtime.limit, runtime.agent.run_queue_timeout_ms, &cancel).await {
        Acquired::Permit(permit) => permit,
        Acquired::Timeout => return fail(runtime, &run, &mut feed, error::QUEUE_TIMEOUT).await,
        Acquired::Cancelled => return cancelled(runtime, &run, &mut feed, cancel.stop()).await,
    };

    // 二、推进到 RUNNING：正常 Run 从 `QUEUED` 推进；恢复的 Run 从 `PAUSED` 推进并累加恢复次数。
    //    两者都是条件更新，影响 0 行说明已被别的路径推进 / 已终态，这里不产出任何事件。
    let advanced = match resume.is_some() {
        true => runtime.service.resume_paused_run(&run.id).await,
        false => runtime.service.start_run(&run.id).await,
    };
    match advanced {
        Ok(true) => {}
        Ok(false) => {
            // 排队期间被取消 / 已被别的路径落成终态 / 仍在等确认 / 已被别人恢复：不产出任何事件。
            info!(run_id = %run.id, "Run 已不在可推进状态，跳过执行");
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
        Err(public_error) => return fail(runtime, &run, &mut feed, public_error).await,
    };

    // 四、run_started：助手消息 ID 先定，成功收尾时按它写库。
    let assistant_message_id = match id::next_id() {
        Ok(id) => id,
        Err(error) => {
            warn!(run_id = %run.id, %error, "生成助手消息 ID 失败");
            return fail(runtime, &run, &mut feed, error::STORAGE).await;
        }
    };
    let mut started = feed.event(RunEventKind::RunStarted);
    started.status = Some(RunStatus::Running);
    started.message_id = Some(assistant_message_id.clone());
    started.input_tokens = Some(prepared.input_tokens);
    // 恢复的 Run 在首个 `run_started` 上带同一句简短原因（复用既有字段，不新增事件类型，D-P6-007）。
    if resume.is_some() {
        started.error = Some(error::interrupted_notice(run.interrupted_reason.as_deref()));
    }
    if !feed.send(started).await {
        return RunOutcome::SessionGone;
    }

    // 四.5、起点检查点：只在正常路径写。恢复的 Run 上次的进度已经有检查点，
    //      这里没有新的已提交边界，补一条只会让序号与「已提交到哪」脱节。
    if resume.is_none()
        && let Err(public) = checkpoint::append(runtime, &run, CheckpointPhase::ModelDone, 0, Vec::new()).await
    {
        return fail(runtime, &run, &mut feed, public).await;
    }

    // 五、调用模型：未配置 Provider 时按脱敏文案落 FAILED，不发上游请求。
    let Some(llm) = runtime.llm.clone() else {
        return fail(runtime, &run, &mut feed, LlmError::Unconfigured.public_message()).await;
    };

    // 六、工具清单与本 Run 已落库的轨迹：确认唤醒从「待确认的调用」续上；中断恢复则先收尾
    //    上一次留下的未配对调用（一律不重跑），再进多轮。
    let tools = tool::definitions(runtime);
    let recovered = match trace::recover(&runtime.service, &run).await {
        Ok(recovered) => recovered,
        Err(public) => return fail(runtime, &run, &mut feed, public).await,
    };
    let resumed = match resume {
        None => tool::resume(runtime, &run, &mut feed, recovered, &cancel).await,
        Some(point) => {
            // 判定阶段的恢复点与轨迹必须逐项相同：不一致说明检查点与轨迹在判定之后被改过，
            // 这种「半恢复」不允许继续执行（D-P6-007）。
            if point.round != recovered.trace.rounds() || !same_calls(&point.unpaired, &recovered.unpaired) {
                return fail(runtime, &run, &mut feed, error::RESUME_PHASE_MISMATCH).await;
            }
            info!(
                run_id = %run.id,
                checkpoint_seq = point.checkpoint_seq,
                phase = ?point.phase,
                round = point.round,
                "从检查点恢复执行"
            );
            tool::resume_interrupted(runtime, &run, &mut feed, recovered).await
        }
    };
    let mut trace = match resumed {
        Resumed::Ready(trace) => trace,
        Resumed::Cancelled => return cancelled(runtime, &run, &mut feed, cancel.stop()).await,
        Resumed::Failed(public) => return fail(runtime, &run, &mut feed, public).await,
        Resumed::SessionGone => return RunOutcome::SessionGone,
    };

    // 七、多轮：轮次从「已发生的工具轮次」继续数，轮次用尽即 FAILED。
    let max_rounds = u32::try_from(runtime.agent.max_tool_rounds).unwrap_or(u32::MAX).max(1);
    // Run 级重试上限（D-P6-012）：`0` 表示一次都不重试，与改动前的失败口径完全一致。
    let max_retries = u32::try_from(runtime.agent.run_max_retries).unwrap_or(u32::MAX);
    let mut round = trace.rounds();
    let mut usage: Option<(i64, i64)> = None;
    loop {
        round += 1;
        if round > max_rounds {
            return fail(runtime, &run, &mut feed, error::TOOL_ROUNDS_EXCEEDED).await;
        }
        let request = request::build(&run, &prepared, &trace, &tools);
        // Run 级重试（D-P6-012）：只覆盖「同一次 LLM 请求」，不重新入队、不重跑已提交轮次。
        let mut attempt: u32 = 0;
        let outcome = loop {
            let outcome = request::stream(llm.as_ref(), request.clone(), &mut feed, &cancel).await;
            match outcome {
                // 可重试分类 + 本轮还没有任何增量发给客户端，才允许重发同一份请求。
                request::Outcome::Failed { error, partial }
                    if !partial && error::retryable(&error) && attempt < max_retries =>
                {
                    attempt += 1;
                    metrics::record_run_retry();
                    warn!(run_id = %run.id, attempt, %error, "模型调用失败，按配置重试同一轮请求");
                    if !backoff(runtime, &cancel).await {
                        return cancelled(runtime, &run, &mut feed, cancel.stop()).await;
                    }
                }
                other => break other,
            }
        };
        match outcome {
            request::Outcome::Cancelled => return cancelled(runtime, &run, &mut feed, cancel.stop()).await,
            request::Outcome::Failed { error, .. } => {
                // 真的重试过就用固定文案；一次都没重试（不可重试分类 / 已推过增量 / 未配置重试）
                // 仍用原始脱敏文案，既有失败口径逐字段不变。
                let public = match attempt {
                    0 => error.public_message(),
                    _ => error::RETRY_EXHAUSTED.to_owned(),
                };
                return fail(runtime, &run, &mut feed, public).await;
            }
            request::Outcome::SessionGone => return RunOutcome::SessionGone,
            request::Outcome::Final {
                content,
                usage: round_usage,
            } => {
                usage = add_usage(usage, round_usage);
                return complete(runtime, &run, &mut feed, assistant_message_id, content, usage).await;
            }
            request::Outcome::ToolCalls {
                content,
                calls,
                usage: round_usage,
            } => {
                usage = add_usage(usage, round_usage);
                let turn = tool::Turn { round, content, calls };
                let step = tool::run_round(runtime, &run, &mut feed, turn, &mut trace, &cancel).await;
                match step {
                    Step::Continue => {}
                    Step::Suspended { tool_call_id } => return RunOutcome::Suspended { tool_call_id },
                    Step::Cancelled => return cancelled(runtime, &run, &mut feed, cancel.stop()).await,
                    Step::Failed(public) => return fail(runtime, &run, &mut feed, public).await,
                    Step::SessionGone => return RunOutcome::SessionGone,
                }
            }
        }
    }
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
            .map(to_llm_message)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| logged(error.public_message(), error))?,
        max_output_tokens: model.max_output_tokens as u64,
        input_tokens: context.metadata().estimated_tokens as i64,
    })
}

/// Context Item → 请求消息：工具轨迹要带上 `tool_call_id` 与调用清单，否则 Provider 侧无法
/// 表达「助手请求调用」与「工具结果」这两条历史（P3-010 的三种消息形态）。
///
/// 历史里带工具轨迹是常态（上一轮会话留下的），因此这一步必须按 `ContextItem` 的字段逐项还原，
/// 不能一律当普通文本——那样第二轮请求就会被适配器判为「工具结果消息必须带 tool_call_id」。
fn to_llm_message(item: &ContextItem) -> Result<LlmMessage, LlmError> {
    if item.role == MessageRole::Tool {
        return LlmMessage::tool_result(item.tool_call_id.clone().unwrap_or_default(), item.content.clone());
    }
    if !item.tool_calls.is_empty() {
        let calls = item
            .tool_calls
            .iter()
            .map(|call| LlmToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            })
            .collect();
        return LlmMessage::assistant_tools(item.content.clone(), calls);
    }
    LlmMessage::text(item.role, item.content.clone())
}

/// token 用量累加（P3-014 第 8 条）：每轮的 usage 相加，缺一轮就按已有值算。
fn add_usage(total: Option<(i64, i64)>, next: Option<(i64, i64)>) -> Option<(i64, i64)> {
    match (total, next) {
        (Some((input, output)), Some((more_input, more_output))) => Some((input + more_input, output + more_output)),
        (Some(total), None) => Some(total),
        (None, next) => next,
    }
}

/// 判定阶段的未配对调用与轨迹里的必须逐条相同：同一份数据的两次读取，不一致即「半恢复」。
fn same_calls(planned: &[LlmToolCallVO], traced: &[LlmToolCall]) -> bool {
    planned.len() == traced.len()
        && planned
            .iter()
            .zip(traced)
            .all(|(planned, traced)| planned.id == traced.id)
}

/// 成功收尾：助手消息与 `COMPLETED` 同事务提交；条件更新影响 0 行即静默退出（D-P2-009）。
async fn complete(
    runtime: &AgentRuntime,
    run: &Run,
    feed: &mut Feed<'_>,
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

/// 重试前的固定退避（D-P6-012）：等待 `[agent] run_retry_backoff_ms`，期间收到停止信号即返回 `false`。
///
/// 退避为 `0` 表示立即重试：仍然先看一眼停止信号，避免取消被拖到下一轮请求。
async fn backoff(runtime: &AgentRuntime, cancel: &RunCancel) -> bool {
    let wait = runtime.agent.run_retry_backoff_ms;
    if wait == 0 {
        return !cancel.is_cancelled();
    }
    tokio::select! {
        biased;
        () = cancel.cancelled() => false,
        () = tokio::time::sleep(Duration::from_millis(wait)) => true,
    }
}

/// 失败收尾：`FAILED` 条件更新成功才发 `run_failed`。
async fn fail(runtime: &AgentRuntime, run: &Run, feed: &mut Feed<'_>, public_error: impl Into<String>) -> RunOutcome {
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
async fn cancelled(runtime: &AgentRuntime, run: &Run, feed: &mut Feed<'_>, stop: RunStop) -> RunOutcome {
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
