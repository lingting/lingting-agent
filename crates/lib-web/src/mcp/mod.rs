//! MCP 管理接口（P4-014）：server 状态与工具清单可查，启停 / 重连 / 探测可触发。
//!
//! 路径沿用 D-P4-016：资源段 `mcp`，动作段只有 `list` / `update`，没有路径占位符
//! （`serverId` 走查询参数或请求体，D-008）。
//!
//! Handler 只做「参数解包 + 门面调用 + 错误映射」：状态机、注册表差集与退避重连全在
//! `agent-mcp`，装配与门面取用在 `manager-agent`。错误映射只在本文件发生一次。

pub mod api;
pub mod dto;

use framework_web::WebError;
use manager_agent::McpManagerError;

/// 管理动作的唯一错误出口：未知 server → 400 + 固定文案，其余 → 500。
///
/// 400 的文案取自 [`McpManagerError`] 的 `Display`（`agent-mcp/src/manager.rs` 是唯一产点），
/// 接口层不另写一份字面量；500 的对外文案由框架统一成 `服务器内部错误`，细节只进日志。
pub(crate) fn api_error(error: anyhow::Error) -> anyhow::Error {
    let Some(kind) = error.downcast_ref::<McpManagerError>() else {
        return WebError::internal("MCP 管理接口内部错误", error).into();
    };
    WebError::parameter(kind.to_string(), error).into()
}
