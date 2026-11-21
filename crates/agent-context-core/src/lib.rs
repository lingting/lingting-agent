//! Agent Context 的稳定抽象（P2-019）：有序 Item 集合、来源标记、预算策略与 Token 估算 trait。
//!
//! 本 Crate 是 Tool（P3）、MCP（P4）、Skill（P5）参与 Context 组装时共同遵循的抽象层：
//! 只定义纯数据与纯函数，不含组装流程、不做 IO、不出现 `async`，也不依赖 Rig / HTTP 框架 /
//! 数据库。具体组装在 `agent-context`，Token 估算实现同样在那里。
//!
//! 依赖方向：`agent-context-core -> agent-core / lib-core`。

mod context;
mod error;
mod item;
mod policy;
mod source;

pub use context::{Context, ContextMetadata};
pub use error::ContextError;
pub use item::ContextItem;
pub use policy::{ContextPolicy, MESSAGE_TOKEN_OVERHEAD, TokenEstimator};
pub use source::ContextSource;
