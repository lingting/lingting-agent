//! 本 Run 的工具轨迹：已发生的助手调用与工具结果（P3-014 / D-P3-024）。
//!
//! 轨迹有两个来源：本轮内存里刚产生的消息，以及挂起唤醒后从 `messages` 读回的历史。
//! 两者都先归一成 [`LlmMessage`]，每轮请求只从这里取工具部分，因此不存在「历史一份、
//! 内存一份」两套顺序。请求的基础部分（系统提示词 + 历史 + 本次输入）由
//! [`super::prepare`] 一次算好，与轨迹拼接后就是本轮请求体。
//!
//! 恢复判定口径（D-P3-023）：只有**最后一个**工具组允许缺结果（它就是挂起点）；
//! 更早的组缺结果、或出现找不到助手调用的孤儿 TOOL，一律按数据损坏处理（落 `FAILED`）。

use agent_core::session::{LlmToolCallVO, Message, MessageRole, Run};
use agent_route::{LlmMessage, LlmToolCall};
use agent_tool::ToolCallRecord;
use lib_core::{id, time};
use service_core::session::SessionService;

use crate::error;

/// 本 Run 已经发生的工具轨迹（按时间顺序，assistant 调用与其结果成对出现）。
#[derive(Default)]
pub(crate) struct Trace {
    /// 请求形态的轨迹消息：接在本次输入之后。
    messages: Vec<LlmMessage>,
    /// 已经发生的工具轮次（含恢复出来的）。
    rounds: u32,
}

impl Trace {
    /// 追加一轮完整轨迹：assistant 调用 + 它的全部 TOOL 结果。
    pub(crate) fn push_round(&mut self, mut messages: Vec<LlmMessage>) {
        self.rounds += 1;
        self.messages.append(&mut messages);
    }

    /// 追加一组工具结果（挂起恢复时 assistant 消息已在轨迹里，不重复计数）。
    pub(crate) fn push_results(&mut self, messages: Vec<LlmMessage>) {
        self.messages.extend(messages);
    }

    /// 轨迹消息。
    pub(crate) fn messages(&self) -> &[LlmMessage] {
        &self.messages
    }

    /// 已经发生的工具轮次。
    pub(crate) fn rounds(&self) -> u32 {
        self.rounds
    }
}

/// 恢复出来的轨迹与「挂起点」。
pub(crate) struct Recovered {
    /// 已落库的轨迹。
    pub(crate) trace: Trace,
    /// 最后一个工具组里没有配对结果的调用：恢复时要收尾的那几个。
    pub(crate) unpaired: Vec<LlmToolCall>,
}

/// 读回本 Run 已落库的工具轨迹，并认出挂起点（D-P3-023）。
///
/// 返回的 `Err` 是可以直接写进 `runs.error` 的固定文案（轨迹损坏或读库失败）。
pub(crate) async fn recover(service: &SessionService, run: &Run) -> Result<Recovered, String> {
    let messages = service
        .tool_trace_of_run(&run.session_id, &run.id)
        .await
        .map_err(|error| super::logged(error::STORAGE, error))?;
    let groups = group(messages)?;
    let total = groups.len();
    let mut trace = Trace::default();
    let mut unpaired = Vec::new();
    for (index, (assistant, results)) in groups.into_iter().enumerate() {
        let calls = calls_of(&assistant)?;
        trace.push_round(vec![llm_of(&assistant)?]);
        let missing: Vec<LlmToolCall> = calls
            .iter()
            .filter(|call| {
                !results
                    .iter()
                    .any(|result| result.tool_call_id.as_deref() == Some(call.id.as_str()))
            })
            .cloned()
            .collect();
        if !missing.is_empty() {
            // 挂起点只可能在最后一个组：更早的组缺结果说明历史已经损坏。
            if index + 1 != total {
                return Err(error::TOOL_TRACE_BROKEN.to_string());
            }
            unpaired = missing;
        }
        trace.push_results(results.iter().map(llm_of).collect::<Result<Vec<_>, _>>()?);
    }
    Ok(Recovered { trace, unpaired })
}

/// 按「助手调用 + 它的结果」分组：结果永远挂在它前面最近的那条助手消息上。
fn group(messages: Vec<Message>) -> Result<Vec<(Message, Vec<Message>)>, String> {
    let mut groups: Vec<(Message, Vec<Message>)> = Vec::new();
    for message in messages {
        match message.role {
            MessageRole::Assistant => groups.push((message, Vec::new())),
            MessageRole::Tool => match groups.last_mut() {
                Some(group) => group.1.push(message),
                // 孤儿 TOOL：找不到对应的助手调用（数据损坏）。
                None => return Err(error::TOOL_TRACE_BROKEN.to_string()),
            },
            MessageRole::System | MessageRole::User => {}
        }
    }
    Ok(groups)
}

/// 一轮的 assistant 消息：本轮请求的全部调用都在这里，落库后它就是挂起点的事实源。
pub(crate) fn assistant_message(run: &Run, content: &str, calls: &[LlmToolCall]) -> Result<Message, String> {
    Ok(Message {
        id: message_id()?,
        session_id: run.session_id.clone(),
        run_id: Some(run.id.clone()),
        role: MessageRole::Assistant,
        content: content.to_owned(),
        tool_call_id: None,
        tool_name: None,
        tool_calls: Some(calls.iter().map(call_of).collect()),
        model: Some(run.model.clone()),
        input_tokens: None,
        output_tokens: None,
        created_at_ms: time::now_millis(),
    })
}

/// 工具结果消息：一条审计记录一条消息，正文就是回灌给模型的文本（失败也是文本）。
pub(crate) fn tool_message(run: &Run, record: &ToolCallRecord) -> Result<Message, String> {
    Ok(Message {
        id: message_id()?,
        session_id: run.session_id.clone(),
        run_id: Some(run.id.clone()),
        role: MessageRole::Tool,
        content: record.result.clone().unwrap_or_default(),
        tool_call_id: Some(record.tool_call_id.clone()),
        tool_name: Some(record.tool_name.clone()),
        tool_calls: None,
        model: None,
        input_tokens: None,
        output_tokens: None,
        created_at_ms: time::now_millis(),
    })
}

/// 落库形态 → 请求形态：只接受工具消息（助手调用或 TOOL 结果）。
pub(crate) fn llm_messages(messages: &[Message]) -> Result<Vec<LlmMessage>, String> {
    messages.iter().map(llm_of).collect()
}

/// 一条消息的请求形态。
fn llm_of(message: &Message) -> Result<LlmMessage, String> {
    match message.role {
        MessageRole::Assistant => LlmMessage::assistant_tools(&message.content, calls_of(message)?)
            .map_err(|error| super::logged(error::TOOL_TRACE_BROKEN, error)),
        MessageRole::Tool => {
            let tool_call_id = message.tool_call_id.clone().unwrap_or_default();
            LlmMessage::tool_result(tool_call_id, &message.content)
                .map_err(|error| super::logged(error::TOOL_TRACE_BROKEN, error))
        }
        MessageRole::System | MessageRole::User => Err(error::TOOL_TRACE_BROKEN.to_string()),
    }
}

/// 助手消息里的调用清单；空清单说明这条消息不该出现在轨迹里（数据损坏）。
fn calls_of(message: &Message) -> Result<Vec<LlmToolCall>, String> {
    let calls = message.tool_calls.as_deref().unwrap_or_default();
    if calls.is_empty() {
        return Err(error::TOOL_TRACE_BROKEN.to_string());
    }
    Ok(calls
        .iter()
        .map(|call| LlmToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        })
        .collect())
}

/// 调用清单的契约形态：落库时用（与 `agent-route` 的同名结构刻意分开，协议类型不外泄）。
fn call_of(call: &LlmToolCall) -> LlmToolCallVO {
    LlmToolCallVO {
        id: call.id.clone(),
        name: call.name.clone(),
        arguments: call.arguments.clone(),
    }
}

/// 消息 ID：与其余消息共用同一套生成规则。
fn message_id() -> Result<String, String> {
    id::next_id().map_err(|error| super::logged(error::STORAGE, error))
}
