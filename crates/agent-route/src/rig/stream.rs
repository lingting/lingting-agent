//! 流式事件映射：Rig 的流式回复 → 项目事件流。
//!
//! 对外产出：文本增量逐条转发；每条工具调用在参数分片聚合完成后产出一次；本轮调用清单
//! 结束时补一条 `ToolCallsDone`；流正常结束时补一条 `Usage`（Provider 两个计数都给出时）与
//! 一条 `Done`；出错时产出一条错误事件并结束（Rig 的错误本身就是流的最后一项）。
//!
//! 工具调用的聚合口径：按 `part` 拼接 `Arguments` 分片，`End` 给出最终调用。分片不是合法
//! JSON 对象、`End` 缺失、或结束时内容不是工具调用，一律 [`LlmError::InvalidRequest`]——
//! 不 panic，也不拿半截参数去猜。

use std::collections::{HashMap, VecDeque};

use futures_util::stream::unfold;
use futures_util::{Stream, StreamExt};
use rig_core::completion::AssistantContent;
use rig_core::message::ToolCall;
use rig_core::streaming::{CompletionStream, Item, PartKind, StreamEvent};

use crate::{LlmError, LlmStreamEvent};

/// 把 Rig 的流式回复包装成项目事件流。
///
/// 返回值被 drop 时 `Relay` 一起销毁，`Streamed` 随之 drop——这就是取消的实现方式。
pub(super) fn relay(
    streamed: CompletionStream,
) -> impl Stream<Item = Result<LlmStreamEvent, LlmError>> + Send + 'static {
    unfold(
        Relay {
            streamed: Some(streamed),
            tail: VecDeque::new(),
            calls: HashMap::new(),
            called: false,
        },
        |mut relay| async move { relay.emit().await.map(|event| (event, relay)) },
    )
}

/// 翻译过程中的状态。
struct Relay {
    /// Rig 的流；读完或出错后置 `None`。
    streamed: Option<CompletionStream>,
    /// 收尾事件：先 `ToolCallsDone`（有工具调用时）与 `Usage`（有则），最后 `Done`。
    tail: VecDeque<Result<LlmStreamEvent, LlmError>>,
    /// 已开始但还没结束的工具调用：`part` → 已拼接的参数分片。
    calls: HashMap<usize, String>,
    /// 本轮是否已产出过工具调用：决定收尾是否要补 `ToolCallsDone`。
    called: bool,
}

impl Relay {
    /// 取下一条对外事件；`None` 表示流结束。
    async fn emit(&mut self) -> Option<Result<LlmStreamEvent, LlmError>> {
        loop {
            if let Some(event) = self.tail.pop_front() {
                return Some(event);
            }
            let item = match self.streamed.as_mut() {
                Some(streamed) => streamed.next().await,
                None => return None,
            };
            match item {
                Some(Ok(Item::Event(StreamEvent::Text { text, .. }))) => {
                    return Some(Ok(LlmStreamEvent::Delta { text }));
                }
                // 工具调用开一个 part 位置；之后的参数分片按这个位置拼接。
                Some(Ok(Item::Event(StreamEvent::Start { part, kind, .. }))) => {
                    if kind == PartKind::ToolCall {
                        self.calls.insert(part.index(), String::new());
                    }
                }
                Some(Ok(Item::Event(StreamEvent::Arguments { part, json }))) => {
                    if let Some(fragments) = self.calls.get_mut(&part.index()) {
                        fragments.push_str(&json);
                    }
                }
                // `part` 结算：聚合完成即产出一条工具调用事件。
                Some(Ok(Item::Event(StreamEvent::End { part, content }))) => {
                    if let Some(fragments) = self.calls.remove(&part.index()) {
                        return Some(self.close(content, fragments));
                    }
                }
                // 推理等其它部分 P2 / P3 不消费：只转发答案文本与工具调用。
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    self.streamed = None;
                    return Some(Err(super::error::map(error)));
                }
                None => self.enqueue_tail(),
            }
        }
    }

    /// 结算一次工具调用：分片必须是完整 JSON 对象，工具名以 `End` 给出的为准。
    fn close(&mut self, content: AssistantContent, fragments: String) -> Result<LlmStreamEvent, LlmError> {
        let AssistantContent::ToolCall(call) = content else {
            return Err(LlmError::InvalidRequest {
                reason: "工具调用缺少结束内容",
            });
        };
        let arguments = arguments_of(&call, fragments)?;
        self.called = true;
        Ok(LlmStreamEvent::ToolCall {
            id: call.id.to_string(),
            name: call.function.name.to_string(),
            arguments,
        })
    }

    /// 流正常结束：缺 `End` 的工具调用先报错，否则按 `ToolCallsDone` → `Usage` → `Done` 收尾。
    ///
    /// 两个计数不同时存在时不产出 `Usage`——用量缺失就留空，不构造假值。
    fn enqueue_tail(&mut self) {
        let usage = self.streamed.as_ref().map(|streamed| streamed.partial().usage);
        self.streamed = None;
        if !self.calls.is_empty() {
            self.calls.clear();
            self.tail.push_back(Err(LlmError::InvalidRequest {
                reason: "工具调用缺少结束事件",
            }));
            return;
        }
        if self.called {
            self.called = false;
            self.tail.push_back(Ok(LlmStreamEvent::ToolCallsDone));
        }
        if let Some(usage) = usage
            && let (Some(input_tokens), Some(output_tokens)) = (usage.input_tokens, usage.output_tokens)
        {
            self.tail.push_back(Ok(LlmStreamEvent::Usage {
                input_tokens: bounded(input_tokens),
                output_tokens: bounded(output_tokens),
            }));
        }
        self.tail.push_back(Ok(LlmStreamEvent::Done));
    }
}

/// 调用参数：优先用分片拼接出的原文；没有分片时用 Rig 结算出的对象。
///
/// 分片不是 JSON 对象、或 Rig 已标记参数非法时都按请求无效拒绝，不把半截参数交给上层。
fn arguments_of(call: &ToolCall, fragments: String) -> Result<String, LlmError> {
    let invalid = LlmError::InvalidRequest {
        reason: "工具调用参数不是合法 JSON",
    };
    if call.function.invalid_arguments.is_some() {
        return Err(invalid);
    }
    if fragments.is_empty() {
        return Ok(serde_json::Value::Object(call.function.arguments.clone()).to_string());
    }
    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&fragments).map_err(|_| invalid)?;
    Ok(fragments)
}

/// Provider 的计数是 `u64`，契约里的用量是 `i64`；越界值只可能来自异常上游，截到上限。
fn bounded(tokens: u64) -> i64 {
    i64::try_from(tokens).unwrap_or(i64::MAX)
}
