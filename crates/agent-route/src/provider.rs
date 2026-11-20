//! LLM 调用抽象：请求、事件流与 Provider trait。
//!
//! 上层只与这里打交道：Rig 的 `Message` / `Streamed` / `ProviderError` 一律不出现在签名里
//! （D-P2-001 第 2、7 条）。取消由调用方 drop [`LlmStream`] 完成，不额外传取消信号。

pub mod service;

use std::future::Future;
use std::pin::Pin;

use agent_core::session::MessageRole;
use futures_util::Stream;

use crate::error::LlmError;

pub use service::ProviderService;

/// 一次 LLM 请求。
#[derive(Debug, Clone)]
pub struct LlmRequest {
    /// 模型标识；必须来自 `[provider.models]`，由调用方经 [`crate::ModelRoute`] 解析。
    pub model: String,
    /// 对话消息：`system` 在最前，最后一条是本次用户输入。
    pub messages: Vec<LlmMessage>,
    /// 单次输出上限；`None` 表示使用模型配置给出的值。
    pub max_output_tokens: Option<u64>,
}

/// 一条对话消息。
#[derive(Debug, Clone)]
pub struct LlmMessage {
    /// 角色：只使用 `System` / `User` / `Assistant`。
    pub role: MessageRole,
    /// 正文。
    pub content: String,
}

/// LLM 流式事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmStreamEvent {
    /// 文本增量；拼接全部增量等于完整回复。
    Delta {
        /// 本段文本。
        text: String,
    },
    /// token 用量；Provider 未给出时不产出本事件，调用方不得自行造值。
    Usage {
        /// 输入 token。
        input_tokens: i64,
        /// 输出 token。
        output_tokens: i64,
    },
    /// 流正常结束；出现本事件之后不会再有事件。
    Done,
}

/// 事件流句柄：类型擦除后由 Runtime 逐条取。
pub struct LlmStream {
    /// Adapter 提供的事件流。
    inner: Pin<Box<dyn Stream<Item = Result<LlmStreamEvent, LlmError>> + Send + 'static>>,
}

impl LlmStream {
    /// 用 Adapter 的事件流构造句柄。
    pub fn new(stream: impl Stream<Item = Result<LlmStreamEvent, LlmError>> + Send + 'static) -> Self {
        Self {
            inner: Box::pin(stream),
        }
    }

    /// 取下一条事件；流结束或已出错时返回 `None`。
    pub async fn next(&mut self) -> Option<Result<LlmStreamEvent, LlmError>> {
        std::future::poll_fn(|context| self.inner.as_mut().poll_next(context)).await
    }
}

/// 一次调用的结果：成功时给出事件流。
pub type LlmStreamFuture = Pin<Box<dyn Future<Output = Result<LlmStream, LlmError>> + Send + 'static>>;

/// LLM 调用端口：`agent-runtime` 只依赖本 trait，不感知具体 Provider。
///
/// 方法返回装箱的 Future 而不是 `async fn`，因为 Runtime 以 `Arc<dyn LlmProvider>` 持有它。
pub trait LlmProvider: Send + Sync {
    /// 发起一次流式调用；返回时首个事件尚未产出。
    fn stream(&self, request: LlmRequest) -> LlmStreamFuture;
}
