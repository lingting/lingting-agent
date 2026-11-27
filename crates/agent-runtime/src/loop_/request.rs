//! 单轮请求与流消费（P3-010 / P3-011 / P3-014）。
//!
//! 每轮都重新拼请求体：基础部分（系统提示词 + 历史 + 本次输入）+ 本 Run 的工具轨迹，
//! 不缓存上一轮的请求（D-P3-014）。流式增量直接进 `message_delta` 事件；工具调用由适配器
//! 聚合完整后一条一条交过来，这里只按模型顺序收集，不做参数猜测。

use agent_core::session::{Run, RunEventKind};
use agent_route::{LlmError, LlmMessage, LlmProvider, LlmRequest, LlmStreamEvent, LlmToolCall, LlmToolDefinition};
use service_core::session::RunCancel;

use crate::feed::Feed;

use super::Prepared;
use super::trace::Trace;

/// 一轮的结局。
pub(crate) enum Outcome {
    /// 没有工具调用：可以收尾了。
    Final {
        /// 本轮正文。
        content: String,
        /// 本轮 token 用量。
        usage: Option<(i64, i64)>,
    },
    /// 本轮请求了工具调用（参数已聚合完整）。
    ToolCalls {
        /// 本轮正文（调用之前的说明文本，可为空串）。
        content: String,
        /// 按模型给出的顺序排列的调用。
        calls: Vec<LlmToolCall>,
        /// 本轮 token 用量。
        usage: Option<(i64, i64)>,
    },
    /// 取消：流已 drop，连接随之关闭。
    Cancelled,
    /// Provider 出错（脱敏文案）。
    /// Provider 出错：保留原始错误供分类，`partial` 表示本轮已经对客户端发出过增量。
    Failed {
        /// 原始错误：分类见 `error::retryable`，对外文案由 `public_message()` 产出。
        error: LlmError,
        /// 本轮是否已经发出过 `message_delta`：发过就不重试——重发会让客户端看到两段拼接的文本。
        partial: bool,
    },
    /// 事件通道已关闭（运行时会话已停止）。
    SessionGone,
}

/// 拼本轮请求：基础消息 + 本 Run 的轨迹，带权限过滤后的工具清单。
pub(crate) fn build(run: &Run, prepared: &Prepared, trace: &Trace, tools: &[LlmToolDefinition]) -> LlmRequest {
    let mut messages: Vec<LlmMessage> = prepared.messages.clone();
    messages.extend(trace.messages().iter().cloned());
    LlmRequest {
        model: run.model.clone(),
        messages,
        tools: tools.to_vec(),
        max_output_tokens: Some(prepared.max_output_tokens),
    }
}

/// 发一次请求并消费整条流。
pub(crate) async fn stream(
    llm: &dyn LlmProvider,
    request: LlmRequest,
    feed: &mut Feed<'_>,
    cancel: &RunCancel,
) -> Outcome {
    let mut stream = match llm.stream(request).await {
        Ok(stream) => stream,
        // 建流就失败：一个字都没发给客户端，重试覆盖的是这种情况。
        Err(error) => {
            return Outcome::Failed { error, partial: false };
        }
    };
    let mut content = String::new();
    let mut calls: Vec<LlmToolCall> = Vec::new();
    let mut usage = None;
    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                drop(stream);
                return Outcome::Cancelled;
            }
            item = stream.next() => match item {
                Some(Ok(LlmStreamEvent::Delta { text })) => {
                    content.push_str(&text);
                    let mut delta = feed.event(RunEventKind::MessageDelta);
                    delta.delta = Some(text);
                    if !feed.send(delta).await {
                        return Outcome::SessionGone;
                    }
                }
                Some(Ok(LlmStreamEvent::ToolCall { id, name, arguments })) => {
                    calls.push(LlmToolCall { id, name, arguments });
                }
                // 本轮调用清单结束：流后面只剩 usage 与 done，继续消费即可。
                Some(Ok(LlmStreamEvent::ToolCallsDone)) => {}
                Some(Ok(LlmStreamEvent::Usage {
                    input_tokens,
                    output_tokens,
                })) => {
                    usage = Some((input_tokens, output_tokens));
                }
                Some(Ok(LlmStreamEvent::Done)) | None => break,
                Some(Err(error)) => {
                    drop(stream);
                    // 已经推过增量就不再重试：重发会让客户端把同一段文本看两遍。
                    return Outcome::Failed {
                        error,
                        partial: !content.is_empty(),
                    };
                }
            }
        }
    }
    // 流读完即释放：连接与缓冲区不再需要。
    drop(stream);
    if calls.is_empty() {
        Outcome::Final { content, usage }
    } else {
        Outcome::ToolCalls { content, calls, usage }
    }
}
