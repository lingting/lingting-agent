//! 会话领域契约：Session / Message / Run，以及 Run 的事件流与状态机。
//!
//! 类型全部用 `auto_type` / `auto_enum` 标注，参与 TS / Rust SDK 生成；
//! 字段按 P2 的需要一次到位（模型、token 用量、时间戳），P2 接入真实运行时后只替换实现。
//! `run` 模块只放 `RunStatus` 的状态机（D-P2-009 的转换表），不定义契约类型。

mod checkpoint;
mod dto;
mod event;
mod request;
mod run;

pub use checkpoint::{
    CHECKPOINT_FORMAT_VERSION, CHECKPOINT_KEEP_LAST, CheckpointDecodeError, CheckpointPayload, CheckpointPhase,
    ResumePoint, RunCheckpoint, ToolCallDigest,
};
pub use dto::{
    LlmToolCallVO, Message, MessageRole, MessageSendVO, Run, RunHistory, RunStatus, RunStep, SessionSummary,
};
pub use event::{RunEvent, RunEventKind, ToolCallStatus};
pub use request::{MessageListParams, MessageSendPO, RunCancelPO, RunGetParams, RunHistoryParams, SessionCreatePO};
