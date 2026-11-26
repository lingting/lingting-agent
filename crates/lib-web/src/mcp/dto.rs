//! MCP 管理接口的契约类型转出（P4-014）。
//!
//! 类型定义在 `agent-mcp/src/vo.rs`，经 `manager-agent` 转发到这里：接口层不维护第二套 DTO
//! （`AGENTS.md` 第 19 条），本模块只是 handler 的单一 import 点。
//!
//! 字段命名与 wire 形态由 `auto_type` / `auto_enum` 决定：`camelCase` 字段名、`Option` 字段可省略、
//! `i64` 时间戳按字符串传输、枚举取 `SCREAMING_SNAKE_CASE`。

pub use manager_agent::{McpAction, McpServerUpdatePO, McpServerVO, McpStatusKind, McpToolListParams, McpToolVO};
