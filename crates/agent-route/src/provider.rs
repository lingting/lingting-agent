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
    /// 本次可用的工具；空表示不带工具，请求里不出现工具定义。
    pub tools: Vec<LlmToolDefinition>,
    /// 单次输出上限；`None` 表示使用模型配置给出的值。
    pub max_output_tokens: Option<u64>,
}

/// 一个工具的定义：只含模型看得到的部分。
///
/// 字段恰好是 name / description / parameters 三个（D-P3-021）：风险等级与本地文件操作声明
/// 是给运行时与审计看的自述，不进请求，也不出现在这里。
#[derive(Debug, Clone, PartialEq)]
pub struct LlmToolDefinition {
    /// 工具名（模型可见的稳定标识）。
    pub name: String,
    /// 工具说明。
    pub description: String,
    /// 参数 JSON Schema。
    pub parameters: serde_json::Value,
}

/// 模型请求的一次工具调用。
///
/// `arguments` 以完整 JSON 文本承载而不是 `Value`：流式分片按文本拼接，历史里也按原样存库，
/// 全程不做二次编码，任何一处都不猜参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmToolCall {
    /// 调用标识；工具结果用它配对。
    pub id: String,
    /// 工具名。
    pub name: String,
    /// 参数：完整 JSON 文本。
    pub arguments: String,
}

/// 一条对话消息：三种形态共用一个结构。
///
/// - 普通文本：`System` / `User` / `Assistant`，两个工具字段都为空；
/// - 助手请求调用：`Assistant` + [`LlmMessage::tool_calls`]，正文可以为空串；
/// - 工具结果：`Tool` + [`LlmMessage::tool_call_id`]，标识必填。
///
/// 非法组合在构造处就被拒绝（[`LlmError::InvalidRequest`]），不会把非法历史发给上游。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmMessage {
    /// 角色。
    pub role: MessageRole,
    /// 正文。
    pub content: String,
    /// 工具结果应答的调用标识；只有 `Tool` 消息有值。
    pub tool_call_id: Option<String>,
    /// 助手本轮的调用清单；只有 `Assistant` 消息可能非空。
    pub tool_calls: Vec<LlmToolCall>,
}

impl LlmMessage {
    /// 普通文本消息：`System` / `User` / `Assistant`。
    ///
    /// `Tool` 角色必须走 [`LlmMessage::tool_result`]——工具结果没有调用标识就是非法历史。
    pub fn text(role: MessageRole, content: impl Into<String>) -> Result<Self, LlmError> {
        if role == MessageRole::Tool {
            return Err(LlmError::InvalidRequest {
                reason: "工具结果消息必须带 tool_call_id",
            });
        }
        Ok(Self {
            role,
            content: content.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        })
    }

    /// 工具结果消息：`tool_call_id` 是必填项，空串与缺失一样按请求无效拒绝。
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Result<Self, LlmError> {
        let tool_call_id = tool_call_id.into();
        if tool_call_id.is_empty() {
            return Err(LlmError::InvalidRequest {
                reason: "工具结果消息必须带 tool_call_id",
            });
        }
        Ok(Self {
            role: MessageRole::Tool,
            content: content.into(),
            tool_call_id: Some(tool_call_id),
            tool_calls: Vec::new(),
        })
    }

    /// 助手请求调用的消息：正文可以为空串，调用清单不能为空。
    pub fn assistant_tools(content: impl Into<String>, tool_calls: Vec<LlmToolCall>) -> Result<Self, LlmError> {
        if tool_calls.is_empty() {
            return Err(LlmError::InvalidRequest {
                reason: "助手工具调用清单不能为空",
            });
        }
        Ok(Self {
            role: MessageRole::Assistant,
            content: content.into(),
            tool_call_id: None,
            tool_calls,
        })
    }
}

/// LLM 流式事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmStreamEvent {
    /// 文本增量；拼接全部增量等于完整回复。
    Delta {
        /// 本段文本。
        text: String,
    },
    /// 一次工具调用：一次调用一条，`arguments` 是完整 JSON 文本。
    ToolCall {
        /// 调用标识。
        id: String,
        /// 工具名。
        name: String,
        /// 参数：完整 JSON 文本。
        arguments: String,
    },
    /// 本轮工具调用清单结束；之后可以继续消费收尾事件。
    ToolCallsDone,
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
