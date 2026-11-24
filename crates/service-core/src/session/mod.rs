//! 会话业务：Session / Message / Run 的业务入口、数据访问与执行端口。
//!
//! Run 的执行（谁推进 `QUEUED → RUNNING`、谁产出事件）属于进程内运行时，由装配层
//! `manager-agent` 的 `runner` 承担（D-P2-035）；本模块只提供业务入口、执行方需要的读写方法，
//! 以及执行端口的 trait 定义（[`RunDriver`]，D-P2-028）。

mod driver;
mod error;
mod pending;
mod service;

/// 审计记录与查询载荷的契约类型：定义在 `agent-tool`（D-P3-011），这里只做转发，
/// 让装配层与 Handler 不必直接依赖 `agent-tool`（`cargo tree -i agent-tool` 的消费者只有
/// `agent-runtime` / `service-core` / `bin-tests`）。
pub use agent_tool::{ToolAuditListParams, ToolCallVO};
pub use driver::{RunCancel, RunDriver, RunInput, RunOutcome, RunStop};
pub use error::SessionError;
pub use pending::{PendingToolCall, ResumeDecision};
pub use service::{REASON_CANCELLED, SessionService};
