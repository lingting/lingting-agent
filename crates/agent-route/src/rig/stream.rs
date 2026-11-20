//! 流式事件映射：Rig 的流式回复 → 项目事件流。
//!
//! 对外只产出三类事件：文本增量逐条转发；流正常结束时补一条 `Usage`（Provider 两个计数都
//! 给出时）与一条 `Done`；出错时产出一条错误事件并结束（Rig 的错误本身就是流的最后一项）。

use std::collections::VecDeque;

use futures_util::stream::unfold;
use futures_util::{Stream, StreamExt};
use rig_core::streaming::{CompletionStream, Item, StreamEvent};

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
        },
        |mut relay| async move { relay.emit().await.map(|event| (event, relay)) },
    )
}

/// 翻译过程中的状态。
struct Relay {
    /// Rig 的流；读完或出错后置 `None`。
    streamed: Option<CompletionStream>,
    /// 收尾事件：先 `Usage`（有则）后 `Done`。
    tail: VecDeque<Result<LlmStreamEvent, LlmError>>,
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
                // 推理、工具调用等部分 P2 不消费，只转发答案文本。
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    self.streamed = None;
                    return Some(Err(super::error::map(error)));
                }
                None => self.collect_tail(),
            }
        }
    }

    /// 流正常结束：按 Provider 给出的用量补 `Usage`，再补 `Done`。
    ///
    /// 两个计数不同时存在时不产出 `Usage`——用量缺失就留空，不构造假值。
    fn collect_tail(&mut self) {
        let usage = self.streamed.as_ref().map(|streamed| streamed.partial().usage);
        self.streamed = None;
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

/// Provider 的计数是 `u64`，契约里的用量是 `i64`；越界值只可能来自异常上游，截到上限。
fn bounded(tokens: u64) -> i64 {
    i64::try_from(tokens).unwrap_or(i64::MAX)
}
