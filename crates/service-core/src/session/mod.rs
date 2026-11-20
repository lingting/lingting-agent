//! 会话业务：Session / Message / Run 的业务入口与数据访问。
//!
//! Run 的执行（谁推进 `QUEUED → RUNNING`、谁产出事件）属于进程内运行时，
//! 由装配层 `manager-agent` 的 `runner` 承担（D-P2-035）；本模块只提供业务入口与
//! 执行方需要的读写方法。

mod error;
mod service;

pub use error::SessionError;
pub use service::{REASON_CANCELLED, SessionService};
