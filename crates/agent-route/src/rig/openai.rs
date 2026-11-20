//! OpenAI 兼容协议的 Provider：客户端构造与请求翻译。

use std::time::Duration;

use agent_core::session::MessageRole;
use rig_core::completion::{CompletionRequest, Message};
use rig_core::providers::openai::{OpenAI, OpenAIConfig, Route};
use types_core::config::ProviderConfig;

use crate::{LlmError, LlmMessage, LlmProvider, LlmRequest, LlmStream, LlmStreamFuture};

/// OpenAI 兼容协议的 LLM Provider。
#[derive(Debug, Clone)]
pub struct OpenAiProvider {
    /// Rig 客户端；内部共享 reqwest 连接池，clone 只增加引用计数。
    client: OpenAI,
    /// 流式空闲超时（`agent.llm_idle_timeout_ms`）；`None` 表示不限。
    idle_timeout: Option<Duration>,
}

impl OpenAiProvider {
    /// 用 Provider 配置构造；`api_key` 只经 [`types_core::config::Secret::expose`] 取值，
    /// 不落日志、不进错误（D-P2-017）。
    pub fn new(config: &ProviderConfig, idle_timeout: Option<Duration>) -> Self {
        let client = OpenAIConfig::new(config.api_key.expose().to_owned())
            .with_base_url(config.base_url.clone())
            // 完成接口固定 Chat Completions，不随 dialect 的默认路由漂到 `/responses`（D-P2-001 第 5 条）。
            .with_route(Route::Chat)
            .client();
        Self { client, idle_timeout }
    }
}

impl LlmProvider for OpenAiProvider {
    fn stream(&self, request: LlmRequest) -> LlmStreamFuture {
        let client = self.client.clone();
        let idle_timeout = self.idle_timeout;
        Box::pin(async move {
            let (model_id, rig_request) = translate(request)?;
            let model = client.chat(model_id).with_idle_timeout(idle_timeout);
            // 编码期失败（例如请求建不出来）在这里返回；之后的失败都在流里。
            let streamed = model.stream(rig_request).map_err(super::error::map)?;
            Ok(LlmStream::new(super::stream::relay(streamed)))
        })
    }
}

/// 把项目请求翻译成 Rig 请求，并取出模型标识。
///
/// 约定（见 [`LlmRequest::messages`]）：`system` 在最前，最后一条是本次用户输入；
/// 违反约定（没有任何消息）按请求无效处理，不发出去也不 panic。
fn translate(request: LlmRequest) -> Result<(String, CompletionRequest), LlmError> {
    let mut history: Vec<Message> = request.messages.into_iter().map(to_rig_message).collect();
    let Some(prompt) = history.pop() else {
        return Err(LlmError::InvalidRequest {
            reason: "没有可发送的消息",
        });
    };
    let rig_request = CompletionRequest::new(prompt)
        .messages(history)
        .max_tokens(request.max_output_tokens);
    Ok((request.model, rig_request))
}

/// 一条消息的角色映射：Rig 的 `Message` 只有 `System` / `User` / `Assistant` 三种形态。
///
/// P2 单轮没有工具调用，历史里的 `TOOL` 消息由 Context 层过滤（P2-020）；这里兜底成
/// 用户可见内容，避免丢弃已经进入请求的文本。
fn to_rig_message(message: LlmMessage) -> Message {
    match message.role {
        MessageRole::System => Message::system(message.content),
        MessageRole::Assistant => Message::assistant(message.content),
        MessageRole::User | MessageRole::Tool => Message::user(message.content),
    }
}
