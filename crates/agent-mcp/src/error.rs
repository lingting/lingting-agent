//! MCP 侧的错误归一：文案表只有一个产点（D-P4-022）。
//!
//! 传输错误、协议错误、工具不存在统一收成 [`McpError`]；`Display` 只产出模型可读的中文固定文案
//! 加必要的 `server_id` / 工具名，**不含 URL、请求头、环境变量、命令行与堆栈**。rmcp 的原文
//! 只经 `tracing::debug` 落地，既不进事件、也不进响应体（判据 E-2 / E-3）。
//!
//! 超时不在这里造 `ToolError::Timeout`：`[tools] timeout_ms` 的计时归执行器（D-P4-018），
//! MCP 侧的 `ConnectTimeout` 只描述「握手没在 `connect_timeout_ms` 内完成」。

use std::fmt;

use agent_tool::ToolError;
use rmcp::model::ErrorCode;
use rmcp::service::{ClientInitializeError, ServiceError};

/// 文案表的固定前缀：`MCP 服务器不可用：<server_id>`。
pub const SERVER_UNAVAILABLE: &str = "MCP 服务器不可用";
/// 文案表的固定前缀：`MCP 服务器连接超时：<server_id>`。
pub const CONNECT_TIMEOUT: &str = "MCP 服务器连接超时";
/// 文案表的固定前缀：`MCP 服务器通信失败：<server_id>`。
pub const COMMUNICATION: &str = "MCP 服务器通信失败";
/// 文案表的固定前缀：`MCP 服务器返回了无法解析的响应：<server_id>`。
pub const INVALID_RESPONSE: &str = "MCP 服务器返回了无法解析的响应";
/// 文案表的固定前缀：`MCP 服务器上不存在该工具：<工具名>`。
pub const TOOL_NOT_FOUND: &str = "MCP 服务器上不存在该工具";

/// MCP 侧的唯一错误类型。
///
/// 每个取值对应一句模型可读文案（README §4.7 的表）；`server_id` / 工具名是唯一允许拼进来的
/// 变量——它们是用户自己写在配置里的标识，不含凭证。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpError {
    /// 服务器不可用：子进程起不来、握手被拒、连接已结束。
    ServerUnavailable {
        /// server 标识。
        server_id: String,
    },
    /// 握手超时：`connect_timeout_ms` 内没有完成 initialize。
    ConnectTimeout {
        /// server 标识。
        server_id: String,
        /// 超时上限（毫秒）。
        timeout_ms: u64,
    },
    /// 通信失败：传输层断开、请求发不出去、运行期崩溃。
    Communication {
        /// server 标识。
        server_id: String,
    },
    /// 非法响应：协议错误、响应形状不对、JSON-RPC 报错。
    InvalidResponse {
        /// server 标识。
        server_id: String,
    },
    /// 工具不存在：server 不认这个工具（`tools/call` 报 `METHOD_NOT_FOUND`）。
    ToolNotFound {
        /// 工具名（模型可见名）。
        tool_name: String,
    },
}

impl fmt::Display for McpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ServerUnavailable { server_id } => write!(formatter, "{SERVER_UNAVAILABLE}：{server_id}"),
            Self::ConnectTimeout { server_id, timeout_ms } => {
                write!(formatter, "{CONNECT_TIMEOUT}：{server_id}（> {timeout_ms} 毫秒）")
            }
            Self::Communication { server_id } => write!(formatter, "{COMMUNICATION}：{server_id}"),
            Self::InvalidResponse { server_id } => write!(formatter, "{INVALID_RESPONSE}：{server_id}"),
            Self::ToolNotFound { tool_name } => write!(formatter, "{TOOL_NOT_FOUND}：{tool_name}"),
        }
    }
}

impl std::error::Error for McpError {}

impl McpError {
    /// 建 transport 失败（子进程起不来 / 请求头不合法）。
    pub(crate) fn unavailable(server_id: &str) -> Self {
        Self::ServerUnavailable {
            server_id: server_id.to_owned(),
        }
    }

    /// 通信失败（读循环结束、传输层断开）。
    pub(crate) fn communication(server_id: &str) -> Self {
        Self::Communication {
            server_id: server_id.to_owned(),
        }
    }

    /// 握手失败：原文只进 `debug`，对外只有固定文案。
    pub(crate) fn from_initialize(server_id: &str, error: &ClientInitializeError) -> Self {
        tracing::debug!(server = %server_id, error = %error, "MCP 握手失败");
        Self::unavailable(server_id)
    }

    /// 请求失败：JSON-RPC 的 `METHOD_NOT_FOUND` 归「工具不存在」，传输层错误归「通信失败」，
    /// 其余协议问题归「非法响应」。
    pub(crate) fn from_service(server_id: &str, tool_name: Option<&str>, error: &ServiceError) -> Self {
        tracing::debug!(server = %server_id, tool = ?tool_name, error = %error, "MCP 请求失败");
        if let (ServiceError::McpError(data), Some(tool_name)) = (error, tool_name)
            && data.code == ErrorCode::METHOD_NOT_FOUND
        {
            return Self::ToolNotFound {
                tool_name: tool_name.to_owned(),
            };
        }
        match error {
            ServiceError::McpError(_)
            | ServiceError::UnexpectedResponse
            | ServiceError::SubscriptionLagged { .. }
            | ServiceError::InputRequiredRoundsExceeded { .. } => Self::InvalidResponse {
                server_id: server_id.to_owned(),
            },
            // 传输层与取消/超时都是「这一路走不通了」，状态机随后会改 `Failed`。
            _ => Self::communication(server_id),
        }
    }
}

/// MCP 的失败统一落到 [`ToolError::Failed`]：模型看到的是「工具执行失败：<固定文案>」。
///
/// 不在这里造 `Timeout` / `Retryable`：超时归执行器判定（D-P4-018），MCP 调用不重试。
impl From<McpError> for ToolError {
    fn from(error: McpError) -> Self {
        Self::Failed(error.to_string())
    }
}
