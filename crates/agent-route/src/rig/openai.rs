//! OpenAI 兼容协议的 Provider：客户端构造与请求翻译。

use std::collections::HashMap;
use std::time::Duration;

use agent_core::session::MessageRole;
use rig_core::completion::{AssistantContent, AssistantMessage, CompletionRequest, Message, ToolDefinition};
use rig_core::message::{CallId, Text, ToolCall, ToolFunction, ToolName, ToolResult, ToolResultContent, UserContent};
use rig_core::providers::openai::{OpenAI, OpenAIConfig, Route};
use types_core::config::ProviderConfig;

use crate::{
    LlmError, LlmMessage, LlmProvider, LlmRequest, LlmStream, LlmStreamFuture, LlmToolCall, LlmToolDefinition,
};

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
///
/// `tools` 为空时不设置工具：请求体里不出现 `tools` 键，与 P2 逐字节一致。
fn translate(request: LlmRequest) -> Result<(String, CompletionRequest), LlmError> {
    let LlmRequest {
        model,
        messages,
        tools,
        max_output_tokens,
    } = request;

    // 工具结果消息只带调用标识，工具名与参数回查历史里的助手调用（D-P3-025 的配对口径）。
    let mut calls = HashMap::new();
    let mut history: Vec<Message> = messages
        .into_iter()
        .map(|message| to_rig_message(message, &mut calls))
        .collect::<Result<_, _>>()?;
    let Some(prompt) = history.pop() else {
        return Err(LlmError::InvalidRequest {
            reason: "没有可发送的消息",
        });
    };

    let mut rig_request = CompletionRequest::new(prompt)
        .messages(history)
        .max_tokens(max_output_tokens);
    if !tools.is_empty() {
        rig_request = rig_request.tools(to_rig_tools(tools)?);
    }
    Ok((model, rig_request))
}

/// 工具定义翻译：只搬 name / description / parameters 三个字段。
fn to_rig_tools(tools: Vec<LlmToolDefinition>) -> Result<Vec<ToolDefinition>, LlmError> {
    tools
        .into_iter()
        .map(|tool| {
            Ok(ToolDefinition::new(
                tool_name(&tool.name)?,
                tool.description,
                tool.parameters,
            ))
        })
        .collect()
}

/// 一条消息的形态映射：普通文本、助手请求调用、工具结果三种。
///
/// `calls` 按发送顺序累积「调用标识 → 工具名」，工具结果据此补齐 Rig 要求的工具名；
/// 找不到对应调用说明历史本身不合法，按请求无效拒绝而不是猜一个名字。
fn to_rig_message(message: LlmMessage, calls: &mut HashMap<String, String>) -> Result<Message, LlmError> {
    match message.role {
        MessageRole::System => Ok(Message::system(message.content)),
        MessageRole::User => Ok(Message::user(message.content)),
        MessageRole::Assistant if message.tool_calls.is_empty() => Ok(Message::assistant(message.content)),
        MessageRole::Assistant => Ok(Message::Assistant(assistant_message(message, calls)?)),
        MessageRole::Tool => to_rig_tool_result(message, calls),
    }
}

/// 助手回合：正文（非空时）在前，调用按模型给出的顺序在后。
fn assistant_message(message: LlmMessage, calls: &mut HashMap<String, String>) -> Result<AssistantMessage, LlmError> {
    let mut content = Vec::with_capacity(message.tool_calls.len() + 1);
    if !message.content.is_empty() {
        content.push(AssistantContent::Text(Text::new(message.content)));
    }
    for call in message.tool_calls {
        content.push(AssistantContent::ToolCall(to_rig_call(call, calls)?));
    }
    Ok(AssistantMessage::new(content))
}

/// 一次工具调用：参数必须是 JSON 对象，否则按请求无效拒绝，不把半截参数发出去。
fn to_rig_call(call: LlmToolCall, calls: &mut HashMap<String, String>) -> Result<ToolCall, LlmError> {
    if call.id.is_empty() {
        return Err(LlmError::InvalidRequest {
            reason: "工具调用缺少 id",
        });
    }
    let arguments =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&call.arguments).map_err(|_| {
            LlmError::InvalidRequest {
                reason: "工具调用参数不是合法 JSON",
            }
        })?;
    let name = tool_name(&call.name)?;
    let function = ToolFunction::new(name, serde_json::Value::Object(arguments));
    calls.insert(call.id.clone(), call.name);
    Ok(ToolCall::new(CallId::from_wire(call.id), function))
}

/// 一条工具结果：Rig 要求 `User` 消息里的 `ToolResult` 块，且必须带工具名。
///
/// 失败结果也走文本：工具层已把错误加工成模型可读文本，这里不另造 `is_error` 语义。
fn to_rig_tool_result(message: LlmMessage, calls: &HashMap<String, String>) -> Result<Message, LlmError> {
    let Some(call_id) = message.tool_call_id else {
        return Err(LlmError::InvalidRequest {
            reason: "工具结果消息必须带 tool_call_id",
        });
    };
    let name = calls.get(&call_id).ok_or(LlmError::InvalidRequest {
        reason: "工具结果消息找不到对应的工具调用",
    })?;
    let result = ToolResult {
        call: CallId::from_wire(call_id),
        name: tool_name(name)?,
        content: vec![ToolResultContent::text(message.content)],
        is_error: false,
    };
    Ok(Message::User {
        content: vec![UserContent::ToolResult(result)],
    })
}

/// 项目里的工具名是普通字符串，Rig 要求非空；空名属于调用方缺陷。
fn tool_name(name: &str) -> Result<ToolName, LlmError> {
    ToolName::new(name).map_err(|_| LlmError::InvalidRequest {
        reason: "工具名不能为空",
    })
}
