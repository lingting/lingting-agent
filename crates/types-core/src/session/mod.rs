//! 会话领域契约：Session / Message / Run，以及 Run 的事件流。
//!
//! 类型全部用 `auto_type` / `auto_enum` 标注，参与 TS / Rust SDK 生成；
//! 字段按 P2 的需要一次到位（模型、token 用量、时间戳），P2 接入真实运行时后只替换实现。

mod dto;
mod event;
mod request;

pub use dto::{Message, MessageRole, MessageSendVO, Run, RunStatus, SessionSummary};
pub use event::{RunEvent, RunEventKind};
pub use request::{MessageListParams, MessageSendPO, RunCancelPO, RunGetParams, RunStreamParams, SessionCreatePO};
