//! MCP 管理面的契约快照（D-P4-016）：server 状态与工具清单。
//!
//! 类型放在 `agent-mcp`，由 `manager-agent` 的入口函数产出、`lib-web` 的 handler 直接返回
//! （与 `ToolCallVO` 同一条路径）：接口层不再维护第二套 DTO（`AGENTS.md` 第 19 条）。
//!
//! 边界：`McpServerVO` / `McpToolVO` 由 [`crate::McpManager`] 的快照产出；动作与查询入参
//! （`McpServerUpdatePO` / `McpToolListParams`）也定义在这里，接口层只解包转发（P4-014）。

use framework_proc_auto::{auto_enum, auto_type};

/// 一个 server 的连接状态：取值与状态机、`metrics` 的计数口径一致。
///
/// `STOPPED` 只在服务关闭后出现，正常请求不会看到（README §4.8）。
#[auto_enum]
pub enum McpStatusKind {
    /// 配置存在但未启用（管理接口仍能看到它）。
    Disabled,
    /// 正在建 transport / 握手。
    Connecting,
    /// 已握手成功：可以列工具、可以调用。
    Ready,
    /// 连接不可用（原因在 `lastError`）。
    Failed,
    /// 服务已关闭：拒绝新动作与新调用。
    Stopped,
}

/// 管理接口的四个动作（`api/mcp/server/update` 的 `action`）。
#[auto_enum]
pub enum McpAction {
    /// 连接并发现；已 `READY` 时直接返回当前状态（幂等）。
    Enable,
    /// 断开并注销该 server 的全部工具（幂等）。
    Disable,
    /// 先关闭旧连接，再按 `ENABLE` 走一遍。
    Reconnect,
    /// 只读探测：`READY` 时真正发一次 `tools/list`，其余状态直接返回状态本身。
    Probe,
}

/// 一个 server 的状态快照（`api/mcp/server/list` 的元素）。
#[auto_type(default = false, clone = true)]
pub struct McpServerVO {
    /// 配置里的 server 标识。
    pub id: String,
    /// 传输方式标签：`stdio` / `http`。
    pub transport: String,
    /// 配置里是否启用（`enabled`）。
    pub enabled: bool,
    /// 当前状态。
    pub status: McpStatusKind,
    /// 该 server 当前注册在册的 MCP 工具数。
    pub tool_count: u32,
    /// 最近一次失败的可读原因（不含 URL、请求头、命令行）。
    pub last_error: Option<String>,
    /// 最近一次连接成功的时间（Unix 毫秒）。
    pub last_connected_at_ms: Option<i64>,
    /// 下一次自动重连的时间（Unix 毫秒）；未排重连时为空。
    pub next_retry_at_ms: Option<i64>,
}

/// 一条已发现的 MCP 工具（`api/mcp/tool/list` 的元素）。
#[auto_type(default = false, clone = true)]
pub struct McpToolVO {
    /// 所属 server 的标识。
    pub server_id: String,
    /// 模型可见名（`mcp__<server>__<tool>`）。
    pub tool_name: String,
    /// server 里的工具原名（调用时用它）。
    pub original_name: String,
    /// 工具说明。
    pub description: String,
    /// 原样透传的参数 JSON Schema 文本（前端不解析，只展示）。
    pub parameters: String,
}

/// 管理动作的请求体（`api/mcp/server/update`）：`serverId` + `action`。
#[auto_type(default = false)]
pub struct McpServerUpdatePO {
    /// 目标 server 标识；未知 `id` 是 400 `MCP 服务器不存在`，不是 404。
    pub server_id: String,
    /// 要执行的动作。
    pub action: McpAction,
}

/// 工具清单的查询参数（`api/mcp/tool/list`）：`serverId` 可选。
///
/// 缺省返回全部 server 的工具；未知 `id` 返回空列表（不是 404，同审计查询口径）。
#[auto_type]
pub struct McpToolListParams {
    /// 只返回该 server 的工具。
    pub server_id: Option<String>,
}
