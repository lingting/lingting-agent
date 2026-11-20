//! 会话业务：Session / Message / Run 的业务入口、内存 Mock 存储与 Run 事件生产方。
//!
//! Run 由「会话被占用」触发执行（D-P1.1-004）：`run_trigger` 串行推进 `QUEUED` Run，
//! `stream` 把 Mock 事件写进运行时会话；`api/run/stream` 已移除，事件推送只剩 `api/client/stream`。
//!
//! P2 的替换点：`store.rs` 换成 Repository（SQLite），`service.rs` 换成真实 Run 状态机，
//! `stream.rs` 换成 Agent Runtime 的 token 流；本模块的公开方法与返回类型保持现状，
//! `lib-web` 的 Handler 不需要改。

mod error;
pub(crate) mod run_trigger;
mod service;
mod store;
mod stream;

pub use error::SessionError;
pub use service::SessionService;
