//! 工具轮：执行一批调用、按轮落库、命中确认即挂起（P3-014 / P3-015 / P3-016）。
//!
//! 固定的执行顺序（D-P3-024 / D-P3-019）：
//!
//! 1. 调用开始前发 `tool_call_started`（参数已脱敏、已截断）；
//! 2. 交给 [`agent_tool::ToolExecutor`]，与取消信号并列 `select!`；
//! 3. 落库本轮 assistant `tool_calls` 与全部 TOOL 结果（同一事务），提交后才发 `tool_call_completed`；
//! 4. 命中确认点：写 `PENDING` 审计（挂起的事实源）→ `RUNNING` 转 `WAITING` → 发
//!    `tool_call_confirm_required` → 本次 `drive` 结束（让出并发许可）。
//!
//! 失败即回灌（P3-016）：参数非法、超时、执行失败、权限拒绝都已经是 `ToolCallRecord` 里的
//! 模型可读文本，本轮照常进入下一轮；只有**结构性问题**（落库失败、轨迹损坏）才让 Run 落
//! `FAILED`。本模块不认识任何具体工具：并发语义、风险等级、文件操作声明全部由工具自述提供。

use agent_core::session::{CheckpointPhase, Message, Run, RunEventKind, ToolCallStatus};
use agent_route::{LlmToolCall, LlmToolDefinition};
use agent_tool::{ExecOutcome, ToolCall, ToolCallRecord, ToolFileAccess, redact_arguments};
use serde_json::Value;
use service_core::session::RunCancel;
use tracing::warn;

use crate::error;
use crate::feed::Feed;
use crate::port::AgentRuntime;

use super::checkpoint;
use super::trace::{self, Recovered, Trace};

/// 一轮工具执行的结局。
pub(crate) enum Step {
    /// 本轮完成，继续下一轮推理。
    Continue,
    /// 命中确认点：Run 已挂起，本次 `drive` 结束。
    Suspended {
        /// 等待确认的调用标识。
        tool_call_id: String,
    },
    /// Run 被取消（终态事件由取消收尾负责）。
    Cancelled,
    /// 结构性问题：本轮失败并落 `FAILED`。
    Failed(String),
    /// 事件通道已关闭（运行时会话已停止）。
    SessionGone,
}

/// 恢复上一个挂起工具组的结局。
pub(crate) enum Resumed {
    /// 收尾完成：轨迹可以继续用。
    Ready(Trace),
    /// Run 被取消。
    Cancelled,
    /// 结构性问题。
    Failed(String),
    /// 事件通道已关闭。
    SessionGone,
}

/// 模型可见的工具清单：权限过滤 + 按名字稳定排序（P3-004）。
///
/// `ToolDefinition` 的风险等级与本地文件操作声明不进请求，模型只看到名称、说明与参数 schema。
pub(crate) fn definitions(runtime: &AgentRuntime) -> Vec<LlmToolDefinition> {
    runtime
        .tools
        .model_definitions()
        .iter()
        .map(|definition| LlmToolDefinition {
            name: definition.name.clone(),
            description: definition.description.clone(),
            parameters: definition.schema.as_value().clone(),
        })
        .collect()
}

/// 本轮要执行的调用与它的归属。
pub(crate) struct Turn {
    /// 第几轮工具调用（从 1 开始）。
    pub(crate) round: u32,
    /// 本轮正文（调用之前的说明文本，可为空串）。
    pub(crate) content: String,
    /// 模型给出的调用，按模型顺序。
    pub(crate) calls: Vec<LlmToolCall>,
}

/// 执行本轮调用：事件 → 执行 → 落库 → 挂起判定。
pub(crate) async fn run_round(
    runtime: &AgentRuntime,
    run: &Run,
    feed: &mut Feed<'_>,
    turn: Turn,
    trace: &mut Trace,
    cancel: &RunCancel,
) -> Step {
    let Turn { round, content, calls } = turn;
    let entries: Vec<ToolCall> = calls.iter().map(|call| to_call(run, round, call)).collect();
    let executed = match execute(runtime, run, feed, &entries, cancel).await {
        Ok(executed) => executed,
        Err(step) => return step,
    };
    if executed.cancelled {
        return Step::Cancelled;
    }
    // 本轮轨迹：assistant 的全部调用 + 已完成调用的结果，同一事务提交（D-P3-024）。
    let mut messages = match trace::assistant_message(run, &content, &calls) {
        Ok(message) => vec![message],
        Err(public) => return Step::Failed(public),
    };
    for record in &executed.records {
        match trace::tool_message(run, record) {
            Ok(message) => messages.push(message),
            Err(public) => return Step::Failed(public),
        }
    }
    if let Err(public) = audit(runtime, run, &executed.records).await {
        return Step::Failed(public);
    }
    if let Err(public) = append(runtime, &messages).await {
        return Step::Failed(public);
    }
    // 本轮的两个边界都已提交：模型轮次（assistant 调用）与工具批次（结果）同一事务落库，
    // 因此检查点只能写在这之后（D-P6-001）；写不进去即 FAILED，不降级继续（D-P6-010）。
    if let Err(public) = checkpoint::append(runtime, run, CheckpointPhase::ModelDone, round, Vec::new()).await {
        return Step::Failed(public);
    }
    if let Err(public) = checkpoint::append(
        runtime,
        run,
        CheckpointPhase::ToolsDone,
        round,
        checkpoint::digests(&executed.records),
    )
    .await
    {
        return Step::Failed(public);
    }
    if !emit_completed(feed, &executed.records).await {
        return Step::SessionGone;
    }
    if let Some(pending) = executed.confirm {
        return suspend(runtime, run, feed, pending).await;
    }
    match trace::llm_messages(&messages) {
        Ok(items) => trace.push_round(items),
        Err(public) => return Step::Failed(public),
    }
    Step::Continue
}

/// 恢复：把上一个挂起的工具组收尾（批准 → 执行；驳回 → 回灌固定文本）。
///
/// 批准只对这一次调用有效：这批调用用 [`AgentRuntime::executor`] 的「已批准」入口执行，
/// 不再被 `confirm` 权限拦下，但 `deny` 仍然拒绝（挂起期间配置变化不被绕过）。
pub(crate) async fn resume(
    runtime: &AgentRuntime,
    run: &Run,
    feed: &mut Feed<'_>,
    recovered: Recovered,
    cancel: &RunCancel,
) -> Resumed {
    let Recovered { mut trace, unpaired } = recovered;
    if unpaired.is_empty() {
        return Resumed::Ready(trace);
    }
    // 挂起的调用属于最后一个已落库的工具组：轮次号就是轨迹里的组数。
    let round = trace.rounds();
    let calls: Vec<ToolCall> = unpaired.iter().map(|call| to_call(run, round, call)).collect();
    match rejected(runtime, run, &calls).await {
        Ok(true) => {
            let records: Vec<ToolCallRecord> = calls
                .iter()
                .map(|call| ToolCallRecord::pending(call, file_access(runtime, &call.tool_name)).rejected())
                .collect();
            // 审计行已由唤醒事务置为 `DENIED`（`with_resume_run`），这里不再二次收尾：
            // 收尾只命中 `PENDING` 行，重写会得到 0 行并被判成轨迹损坏（P6A-006）。
            let messages = match tool_messages(run, &records) {
                Ok(messages) => messages,
                Err(public) => return Resumed::Failed(public),
            };
            if let Err(public) = append(runtime, &messages).await {
                return Resumed::Failed(public);
            }
            // 驳回同样是这一组的终结：结果消息提交后补一条检查点，恢复时不必再解释这组调用。
            let digests = checkpoint::digests(&records);
            if let Err(public) = checkpoint::append(runtime, run, CheckpointPhase::ToolsDone, round, digests).await {
                return Resumed::Failed(public);
            }
            // 驳回也是一次终结：补完成事件，客户端的开始事件因此不会悬挂。
            if !emit_completed(feed, &records).await {
                return Resumed::SessionGone;
            }
            match trace::llm_messages(&messages) {
                Ok(items) => trace.push_results(items),
                Err(public) => return Resumed::Failed(public),
            }
            Resumed::Ready(trace)
        }
        Ok(false) => {
            let executed = match execute_approved(runtime, feed, calls, cancel).await {
                Ok(executed) => executed,
                Err(step) => return finish(step),
            };
            if executed.cancelled {
                return Resumed::Cancelled;
            }
            if let Err(public) = audit(runtime, run, &executed.records).await {
                return Resumed::Failed(public);
            }
            let messages = match tool_messages(run, &executed.records) {
                Ok(messages) => messages,
                Err(public) => return Resumed::Failed(public),
            };
            if let Err(public) = append(runtime, &messages).await {
                return Resumed::Failed(public);
            }
            // 批准后的工具批次同样是一个已提交边界：唤醒前的 `AWAITING_CONFIRM` 到此才真正收尾。
            let digests = checkpoint::digests(&executed.records);
            if let Err(public) = checkpoint::append(runtime, run, CheckpointPhase::ToolsDone, round, digests).await {
                return Resumed::Failed(public);
            }
            if !emit_completed(feed, &executed.records).await {
                return Resumed::SessionGone;
            }
            match trace::llm_messages(&messages) {
                Ok(items) => trace.push_results(items),
                Err(public) => return Resumed::Failed(public),
            }
            Resumed::Ready(trace)
        }
        Err(step) => finish(step),
    }
}

/// 恢复：把上一次中断留下的未配对调用收尾，**一次都不重跑**（D-P6-007）。
///
/// 与 [`resume`] 的分工：`resume` 处理「客户端回传确认」的挂起唤醒（会执行批准过的调用）；
/// 这里处理「进程消失」的中断恢复——未配对的调用一律不执行，改为一条 `ERROR` 审计 + 一条 TOOL
/// 结果消息，正文是固定通知文本「上次执行被中断：<简短原因>」。全仓只有这一处产出该文本，
/// `agent-tool` / `agent-mcp` 的契约因此零改动（P6A-008）。
///
/// 已配对的调用只进轨迹，同样不执行：恢复的所有分支都不产生工具执行调用。
pub(crate) async fn resume_interrupted(
    runtime: &AgentRuntime,
    run: &Run,
    feed: &mut Feed<'_>,
    recovered: Recovered,
) -> Resumed {
    let Recovered { mut trace, unpaired } = recovered;
    if unpaired.is_empty() {
        return Resumed::Ready(trace);
    }
    // 未配对的调用属于最后一个已落库的工具组：轮次号就是轨迹里的组数。
    let round = trace.rounds();
    let notice = error::interrupted_notice(run.interrupted_reason.as_deref());
    let calls: Vec<ToolCall> = unpaired.iter().map(|call| to_call(run, round, call)).collect();
    let mut records = Vec::with_capacity(calls.len());
    for call in &calls {
        let pending = ToolCallRecord::pending(call, file_access(runtime, &call.tool_name));
        // 开始标记：崩溃前写过的那条还在（`PENDING`），旧库行没有——两种形态都收敛到
        // 「这条调用有开始、没有结果」，插行冲突不算错误（它就是恢复要用的那条证据）。
        if let Err(failure) = runtime
            .service
            .begin_tool_call(&run.id, &run.session_id, &pending)
            .await
        {
            warn!(run_id = %run.id, tool_call_id = %call.tool_call_id, %failure, "恢复时写入工具开始标记失败");
            return Resumed::Failed(error::TOOL_TRACE_STORAGE.to_string());
        }
        let record = pending.finished(ToolCallStatus::Error, notice.clone(), 0);
        match runtime.service.finish_tool_call(&run.id, &record).await {
            Ok(true) => records.push(record),
            Ok(false) => {
                warn!(run_id = %run.id, tool_call_id = %record.tool_call_id, "恢复收尾未命中开始标记");
                return Resumed::Failed(error::TOOL_TRACE_BROKEN.to_string());
            }
            Err(failure) => {
                warn!(run_id = %run.id, tool_call_id = %record.tool_call_id, %failure, "恢复时收尾工具审计失败");
                return Resumed::Failed(error::TOOL_TRACE_STORAGE.to_string());
            }
        }
    }
    let messages = match tool_messages(run, &records) {
        Ok(messages) => messages,
        Err(public) => return Resumed::Failed(public),
    };
    if let Err(public) = append(runtime, &messages).await {
        return Resumed::Failed(public);
    }
    // 这一组调用到此终结：写一条 `TOOLS_DONE` 检查点（序号接着递增，不重置），
    // 恢复后的轨迹因此不必再解释这组调用。
    let digests = checkpoint::digests(&records);
    if let Err(public) = checkpoint::append(runtime, run, CheckpointPhase::ToolsDone, round, digests).await {
        return Resumed::Failed(public);
    }
    if !emit_completed(feed, &records).await {
        return Resumed::SessionGone;
    }
    match trace::llm_messages(&messages) {
        Ok(items) => trace.push_results(items),
        Err(public) => return Resumed::Failed(public),
    }
    Resumed::Ready(trace)
}

/// 一批调用的执行结局。
struct Executed {
    /// 终态记录（按输入顺序；不含需要确认的那条）。
    records: Vec<ToolCallRecord>,
    /// 需要确认的调用；工具没有被执行。
    confirm: Option<ToolCallRecord>,
    /// 是否因为取消而中断。
    cancelled: bool,
}

/// 执行一批调用：开始标记 → 开始事件 → 执行 → 完成事件。
async fn execute(
    runtime: &AgentRuntime,
    run: &Run,
    feed: &mut Feed<'_>,
    calls: &[ToolCall],
    cancel: &RunCancel,
) -> Result<Executed, Step> {
    begin(runtime, run, calls).await?;
    if !emit_started(feed, calls).await {
        return Err(Step::SessionGone);
    }
    drive_batch(runtime, feed, calls, cancel, false).await
}

/// 执行前写入本批调用的开始标记（`PENDING` + `started_at_ms`，P6A-006）。
///
/// 这一步是「这次调用可能已经执行」的唯一证据：崩溃后 `PENDING` 行留着，恢复时不重放它，
/// 只回灌中断通知（D-P6-007）。同一 `tool_call_id` 已有记录说明库里已不一致，直接失败。
async fn begin(runtime: &AgentRuntime, run: &Run, calls: &[ToolCall]) -> Result<(), Step> {
    for call in calls {
        let record = ToolCallRecord::pending(call, file_access(runtime, &call.tool_name));
        match runtime.service.begin_tool_call(&run.id, &run.session_id, &record).await {
            Ok(true) => {}
            Ok(false) => {
                warn!(run_id = %run.id, tool_call_id = %call.tool_call_id, "工具开始标记已存在，按数据不一致处理");
                return Err(Step::Failed(error::TOOL_TRACE_BROKEN.to_string()));
            }
            Err(error) => {
                warn!(run_id = %run.id, tool_call_id = %call.tool_call_id, %error, "写入工具开始标记失败");
                return Err(Step::Failed(error::TOOL_TRACE_STORAGE.to_string()));
            }
        }
    }
    Ok(())
}

/// 执行一批「客户端已批准」的调用：跳过 `confirm` 权限判定，其余顺序不变。
async fn execute_approved(
    runtime: &AgentRuntime,
    feed: &mut Feed<'_>,
    calls: Vec<ToolCall>,
    cancel: &RunCancel,
) -> Result<Executed, Step> {
    // 开始事件在挂起那一轮已经发过（同一 `tool_call_id`），这里只补完成事件。
    drive_batch(runtime, feed, &calls, cancel, true).await
}

/// 与取消并列等待批次：取消时给已发出的开始事件补一条 `CANCELLED` 的完成事件（D-P3-019）。
async fn drive_batch(
    runtime: &AgentRuntime,
    feed: &mut Feed<'_>,
    calls: &[ToolCall],
    cancel: &RunCancel,
    approved: bool,
) -> Result<Executed, Step> {
    let batch = tokio::select! {
        biased;
        () = cancel.cancelled() => None,
        outcome = runtime.executor.execute_batch_of(calls.to_vec(), cancel.token(), approved) => Some(outcome),
    };
    let Some(batch) = batch else {
        emit_cancelled(feed, calls).await;
        return Ok(Executed {
            records: Vec::new(),
            confirm: None,
            cancelled: true,
        });
    };
    let mut records = Vec::new();
    let mut confirm = None;
    for outcome in batch.outcomes {
        match outcome {
            ExecOutcome::Done(record) => records.push(record),
            ExecOutcome::ConfirmRequired { record } => confirm = Some(record),
            ExecOutcome::Cancelled => {}
        }
    }
    if batch.cancelled {
        // 已产出的记录不落库：本轮没有提交，轨迹停在上一轮边界（取消不写中间态）。
        emit_cancelled(feed, calls).await;
        return Ok(Executed {
            records: Vec::new(),
            confirm: None,
            cancelled: true,
        });
    }
    Ok(Executed {
        records,
        confirm,
        cancelled: false,
    })
}

/// 挂起：`RUNNING` 转 `WAITING` → `AWAITING_CONFIRM` 检查点 → 发确认事件。
///
/// `PENDING` 审计在执行前就写好了（[`begin`]），这里只推进状态：这样「在等哪一次调用」
/// 与「库里这次调用有开始无结束」两个事实同时成立（D-P3-027 / P6A-006）。
async fn suspend(runtime: &AgentRuntime, run: &Run, feed: &mut Feed<'_>, pending: ToolCallRecord) -> Step {
    match runtime
        .service
        .suspend_run(&run.id, &pending.tool_call_id, pending.tool_round)
        .await
    {
        Ok(true) => {}
        // 0 行：Run 在挂起前已被取消或已终态，确认事件不再发出。
        Ok(false) => return Step::Cancelled,
        Err(error) => {
            warn!(run_id = %run.id, %error, "挂起 Run 失败");
            return Step::Failed(error::STORAGE.to_string());
        }
    }
    // 挂起事务已提交：写 `AWAITING_CONFIRM` 检查点，恢复时据此知道「停在确认点」（D-P6-001）。
    let digest = checkpoint::digest(&pending);
    if let Err(public) = checkpoint::append(
        runtime,
        run,
        CheckpointPhase::AwaitingConfirm,
        pending.tool_round,
        vec![digest],
    )
    .await
    {
        return Step::Failed(public);
    }
    let mut event = feed.event(RunEventKind::ToolCallConfirmRequired);
    event.tool_call_id = Some(pending.tool_call_id.clone());
    event.tool_name = Some(pending.tool_name.clone());
    event.tool_arguments = Some(pending.arguments.clone());
    event.tool_round = Some(pending.tool_round);
    if !feed.send(event).await {
        return Step::SessionGone;
    }
    Step::Suspended {
        tool_call_id: pending.tool_call_id,
    }
}

/// 这批未配对的调用是否已被用户驳回：审计里已有 `DENIED` 记录即为驳回。
async fn rejected(runtime: &AgentRuntime, run: &Run, calls: &[ToolCall]) -> Result<bool, Step> {
    let records = runtime.service.tool_calls_of_run(&run.id).await.map_err(|error| {
        warn!(run_id = %run.id, %error, "读取工具审计失败，无法判定确认结果");
        Step::Failed(error::STORAGE.to_string())
    })?;
    Ok(records.iter().any(|record| {
        record.status == ToolCallStatus::Denied && calls.iter().any(|call| call.tool_call_id == record.tool_call_id)
    }))
}

/// 终态审计：只命中执行前写入的 `PENDING` 行（P6A-006）。
///
/// 0 行说明这次调用没有开始标记（数据不一致）→ 轨迹损坏，本轮失败；写库失败只记日志，
/// 审计不该把已经成功的 Run 打成失败（P3-018）。
async fn audit(runtime: &AgentRuntime, run: &Run, records: &[ToolCallRecord]) -> Result<(), String> {
    for record in records {
        match runtime.service.finish_tool_call(&run.id, record).await {
            Ok(true) => {}
            Ok(false) => {
                warn!(run_id = %run.id, tool_call_id = %record.tool_call_id, "工具审计收尾未命中开始标记");
                return Err(error::TOOL_TRACE_BROKEN.to_string());
            }
            Err(error) => warn!(run_id = %run.id, tool_call_id = %record.tool_call_id, %error, "工具审计写入失败"),
        }
    }
    Ok(())
}

/// 一轮轨迹落库：写不进去就本轮失败（D-P3-024，不允许「事件已发但库里没有」）。
async fn append(runtime: &AgentRuntime, messages: &[Message]) -> Result<(), String> {
    runtime.service.append_tool_round(messages).await.map_err(|error| {
        warn!(%error, "工具轨迹落库失败，本轮失败");
        error::TOOL_TRACE_STORAGE.to_string()
    })
}

/// 结果消息：一条记录一条 TOOL 消息。
fn tool_messages(run: &Run, records: &[ToolCallRecord]) -> Result<Vec<Message>, String> {
    records.iter().map(|record| trace::tool_message(run, record)).collect()
}

/// 模型调用 → 工具调用：补上运行时才知道的归属（会话、Run、轮次）。
fn to_call(run: &Run, round: u32, call: &LlmToolCall) -> ToolCall {
    ToolCall {
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        // 适配器已经把分片拼成完整 JSON；解析不出来时交给工具按「参数不合法」拒绝。
        arguments: serde_json::from_str(&call.arguments).unwrap_or(Value::Null),
        session_id: run.session_id.clone(),
        run_id: run.id.clone(),
        tool_round: round,
    }
}

/// 未执行调用的本地文件操作声明：未注册的工具按 `None` 处理。
fn file_access(runtime: &AgentRuntime, name: &str) -> ToolFileAccess {
    runtime
        .tools
        .lookup(name)
        .map_or(ToolFileAccess::None, |tool| tool.file_access())
}

/// `Resumed` 与 `Step` 之间的收尾转换：恢复路径只可能出现取消 / 失败 / 会话结束。
fn finish(step: Step) -> Resumed {
    match step {
        Step::Cancelled => Resumed::Cancelled,
        Step::Failed(public) => Resumed::Failed(public),
        Step::SessionGone => Resumed::SessionGone,
        // 批准过的调用不会再被 `confirm` 拦下，这里按失败处理（配置在挂起期间被改回 confirm）。
        Step::Suspended { .. } | Step::Continue => Resumed::Failed(error::TOOL_TRACE_BROKEN.to_string()),
    }
}

/// 发开始事件：一条调用一条，参数已脱敏并截断。
async fn emit_started(feed: &mut Feed<'_>, calls: &[ToolCall]) -> bool {
    for call in calls {
        let mut event = feed.event(RunEventKind::ToolCallStarted);
        event.tool_call_id = Some(call.tool_call_id.clone());
        event.tool_name = Some(call.tool_name.clone());
        event.tool_arguments = Some(redact_arguments(&call.arguments));
        event.tool_round = Some(call.tool_round);
        if !feed.send(event).await {
            return false;
        }
    }
    true
}

/// 发完成事件：失败时同时带上工具错误文本，客户端不必再查审计。
async fn emit_completed(feed: &mut Feed<'_>, records: &[ToolCallRecord]) -> bool {
    for record in records {
        let mut event = feed.event(RunEventKind::ToolCallCompleted);
        event.tool_call_id = Some(record.tool_call_id.clone());
        event.tool_name = Some(record.tool_name.clone());
        event.tool_status = Some(record.status);
        event.tool_duration_ms = Some(i64::try_from(record.duration_ms).unwrap_or(i64::MAX));
        if record.status != ToolCallStatus::Ok {
            event.error = record.result.clone();
        }
        if !feed.send(event).await {
            return false;
        }
    }
    true
}

/// 取消：已经发出过开始事件的调用各补一条 `CANCELLED` 的完成事件（D-P3-019）。
async fn emit_cancelled(feed: &mut Feed<'_>, calls: &[ToolCall]) {
    for call in calls {
        let mut event = feed.event(RunEventKind::ToolCallCompleted);
        event.tool_call_id = Some(call.tool_call_id.clone());
        event.tool_name = Some(call.tool_name.clone());
        event.tool_status = Some(ToolCallStatus::Cancelled);
        event.tool_duration_ms = Some(0);
        if !feed.send(event).await {
            return;
        }
    }
}
