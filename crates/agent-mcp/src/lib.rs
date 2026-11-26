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
//! - 不依赖 `types-core`：`[mcp]` 配置由装配层读成 `McpConfig` 再喂进来（与 `agent-tool`
//!   处理 `[tools]` 的口径一致，D-P4-001）。
//! - 不依赖 `service-core` / `lib-db` / `lib-web*` / `bin-*`：本 Crate 不认识持久化与 HTTP 接口。

mod file_access;

pub use file_access::FileAccessRules;
