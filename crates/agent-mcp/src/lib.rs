//! MCP 集成（P4）：Client、transport、工具发现与 MCP Tool 适配。
//!
//! 本 Crate 是 `rmcp` 的唯一落点：MCP 协议类型不越过这里的边界，`agent-runtime` /
//! `agent-route` / `agent-core` 与装配层之外的业务 Crate 都不认识 MCP。
//!
//! 依赖方向：`agent-mcp -> agent-tool / lib-core`（+ `rmcp`）。
//!
//! 边界：
//!
//! - 不依赖 `agent-runtime` / `agent-route` / `rig-core`：MCP 工具经 `agent_tool::Tool` 适配，
//!   与内置工具在运行时眼里完全同构；执行路径只有 `agent-tool` 的执行器一条。
//! - 不依赖 `types-core`：`[mcp]` 配置由装配层读成 [`McpServerSpec`] 再喂进来（与 `agent-tool`
//!   处理 `[tools]` 的口径一致，D-P4-001）。
//! - 不依赖 `service-core` / `lib-db` / `lib-web*` / `bin-*`：本 Crate 不认识持久化与 HTTP 接口。
//!
//! 模块分工（一个概念一个落点）：
//!
//! - [`client`]：连接、握手、状态机、关闭与调用路径（两种 transport 共用一份实现）；
//! - [`transport`]：怎么把一个 server 变成可 `serve` 的 transport（差异只在这里）；
//! - [`discover`]：`tools/list` → 工具快照（翻页、按原名升序、冲突消解、声明落位）；
//! - [`naming`]：模型可见名 `mcp__<server>__<tool>` 的唯一实现点；
//! - [`tool`]：`McpTool` 适配器；[`result`]：调用结果到 `ToolOutput` 的映射；
//! - `manager`：多 server 门面——状态快照、启停、退避重连与差集注册；
//! - `vo`：管理面的契约快照；[`metrics`]：`metrics` 特性下的累计量；
//! - [`error`]：固定文案表与到 `ToolError` 的归一；
//! - [`config`]：装配层翻译过来的连接参数；[`file_access`]：`file_access` 的声明解析。

mod client;
mod config;
mod discover;
mod error;
mod file_access;
mod manager;
pub mod metrics;
mod naming;
mod result;
mod state;
mod tool;
mod transport;
mod vo;

pub use client::{CLIENT_NAME, CloseOutcome, McpClient};
pub use config::{
    DEFAULT_CONNECT_TIMEOUT_MS, DEFAULT_RECONNECT_INITIAL_BACKOFF_MS, DEFAULT_RECONNECT_MAX_BACKOFF_MS, McpConfig,
    McpServerSpec, McpTransport,
};
pub use discover::{McpToolSpec, RawTool};
pub use error::{COMMUNICATION, CONNECT_TIMEOUT, INVALID_RESPONSE, McpError, SERVER_UNAVAILABLE, TOOL_NOT_FOUND};
pub use file_access::FileAccessRules;
pub use manager::{GRACEFUL_CLOSE_TIMEOUT, McpManager, McpManagerError};
pub use naming::{MAX_NAME_LEN, NAME_PREFIX, disambiguate, visible_name};
pub use state::McpState;
pub use tool::McpTool;
pub use vo::{McpAction, McpServerVO, McpStatusKind, McpToolVO};
