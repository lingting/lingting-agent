//! 统一 Tool System（P3）：Tool Trait、定义、Schema、注册表、执行器与内置工具。
//!
//! 本 Crate 是「模型可以调用什么」的唯一来源：`agent-runtime` 只持有 `ToolRegistry` 与
//! `ToolExecutor`，不认识任何具体工具；MCP 工具（P4）与内置工具在运行时眼里没有区别。
//!
//! 依赖方向：`agent-tool -> agent-context-core / agent-core / lib-core`。
//!
//! 边界：
//!
//! - 不依赖 `agent-runtime` / `agent-route` / `rig-core`：第三方 Provider 协议类型不进入工具层，
//!   工具定义翻译成 LLM 请求的职责归 `agent-route`（由 `agent-runtime` 发起）。
//! - 不依赖 `service-core` / `lib-db`：审计记录只产出 `ToolCallRecord`，落库由调用方经
//!   `service-core` 完成。
//! - 不依赖 `lib-web*` / `bin-*`：本 Crate 不认识 HTTP 接口与进程装配。
//! - 不依赖 `types-core`：`[tools]` 配置由装配层读成 [`ToolLimits`] 与 [`ToolPolicy`] 再喂进来。
//! - 内置工具位于 `builtin` 子模块（P3-021 起），`builtin` 之外不出现本地文件与 HTTP 调用。

mod audit;
mod batch;
pub mod builtin;
mod call;
mod context;
mod definition;
mod error;
mod executor;
mod output;
mod permission;
mod registry;
pub mod schema;
mod tool;

pub use audit::{
    MAX_ARGUMENT_CHARS, REJECTED_TEXT, ToolAuditListParams, ToolCallRecord, ToolCallStatus, ToolCallVO,
    redact_arguments,
};
pub use batch::BatchOutcome;
pub use call::ToolCall;
pub use context::ToolContext;
pub use definition::{ToolConcurrency, ToolDefinition, ToolDefinitionVO, ToolFileAccess, ToolRisk};
pub use error::ToolError;
pub use executor::{ExecOutcome, ToolExecutor, ToolLimits};
pub use output::{ToolOutput, truncate};
pub use permission::{ToolPermission, ToolPolicy};
pub use registry::{RegistryError, ToolRegistry};
pub use schema::ToolSchema;
pub use tool::{Tool, validate_name};
