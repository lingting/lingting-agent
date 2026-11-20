//! Agent 装配、进程内运行时与跨 Service 编排。
//!
//! 本 Crate 是**装配层**：把数据访问（`lib-db`）、会话业务（`service-core`）与进程内运行时
//! 装到一起，并以进程级 holder 交给 Handler 使用。`agent + runtime + service` 之间的交互
//! 全部收敛在这里（D-P2-035）：`service-core` 不持有运行时，也不感知 Agent。
//!
//! 依赖方向：`manager-agent -> service-core / lib-db / agent-core / types-core / lib-core`；
//! `service-core` 不反向依赖本 Crate（`docs/workspace.md` 4.2）。

mod assemble;
mod holder;
mod runner;
pub mod runtime;
mod session;

pub use assemble::{AssemblyOptions, assemble, assemble_with};
pub use holder::{AgentHolder, holder, install, runtime, shutdown};
pub use session::{cancel_run, create_session, get_run, list_messages, list_sessions, running_run_count, send_message};

/// 会话业务错误：与 `service-core` 同一套语义，`lib-web` 只在这里映射一次状态码。
pub use service_core::session::SessionError;
/// 会话业务服务：Handler 只需要引用它来校验占用，业务方法一律走本 Crate 的入口函数。
pub use service_core::session::SessionService;
