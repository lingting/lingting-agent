//! MCP 管理接口的三个 handler（P4-014，形态见 D-P4-016）。
//!
//! 读接口只转发门面快照（`DISABLED` 的 server 也在清单里，`tool/list` 按模型可见名升序）；
//! 写接口只转发一个动作，四个 `action` 都返回**动作完成后**的状态，客户端不必二次查询。

use anyhow::Result;
use framework_web::{Json, Query, web_api_get, web_api_post};

use crate::mcp::api_error;
use crate::mcp::dto::{McpServerUpdatePO, McpServerVO, McpToolListParams, McpToolVO};

/// 列出配置里的全部 server（含 `disabled`），按 `id` 升序。
///
/// 未装配 MCP 时返回空清单，不报错：管理面看到的就是「没有 server」。
#[web_api_get(path = "api/mcp/server/list")]
pub async fn mcp_server_list() -> Vec<McpServerVO> {
    manager_agent::mcp_servers()
}

/// 列出当前已发现的 MCP 工具，按模型可见名升序；`serverId` 给定时只返回该 server 的。
///
/// 未知 `serverId` 返回空列表：工具快照是「此刻发现了什么」，不是会话对象，不做存在性校验。
#[web_api_get(path = "api/mcp/tool/list")]
pub async fn mcp_tool_list(Query(po): Query<McpToolListParams>) -> Vec<McpToolVO> {
    manager_agent::mcp_tools(po.server_id.as_deref())
}

/// 执行一个管理动作（`ENABLE` / `DISABLE` / `RECONNECT` / `PROBE`），返回动作完成后的状态。
///
/// 未知 `serverId` 是 400 `MCP 服务器不存在`，不是 404：`serverId` 是请求参数里的标识，
/// 与运行时的「客户端未注册」同一套判定口径（`docs/api/mcp.md`）。
#[web_api_post(path = "api/mcp/server/update")]
pub async fn mcp_server_update(Json(po): Json<McpServerUpdatePO>) -> Result<McpServerVO> {
    manager_agent::mcp_update(&po.server_id, po.action)
        .await
        .map_err(api_error)
}
