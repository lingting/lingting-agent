//! Agent 领域基础类型：Session / Message / Run 的契约类型与事件（D-P2-018）。
//!
//! P1 落在 `types-core` 的 `session` 模块里的契约类型，在 P2-006 整体迁入本 Crate，
//! 类型名、字段名与 serde 行为不变；P1.1 的 `runtime` 模块（客户端连接与会话占用）不属于
//! Agent 领域，不迁入本 Crate（D-P2-031）。
//!
//! 本 Crate 是 Agent 领域最底层的类型来源，不依赖 Rig、MCP、HTTP 框架与数据库。

pub mod session;
