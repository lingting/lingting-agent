//! 会话业务：Session / Message / Run 的业务入口、内存 Mock 存储与 Mock 事件流。
//!
//! P2 的替换点：`store.rs` 换成 Repository（SQLite），`service.rs` 换成真实 Run 状态机，
//! `stream.rs` 换成 Agent Runtime 的 token 流；本模块的公开方法与返回类型保持现状，
//! `lib-web` 的 Handler 不需要改。

mod error;
mod service;
mod store;
mod stream;

pub use error::SessionError;
pub use service::SessionService;
pub use stream::RunEventStream;
