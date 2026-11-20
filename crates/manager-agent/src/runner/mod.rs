//! Run 执行：队列与事件生产（P1.1 的执行通路在装配层的落位）。
//!
//! 归属（D-P2-035）：Run 的执行同时需要「会话业务」（写库）与「运行时会话」（事件生产方），
//! 这类跨组件动作放在装配层 `manager-agent`，`service-core` 只提供业务与数据访问。
//!
//! - [`queue`]：随运行时会话启动的串行执行队列。
//! - [`mock`]：当前的 Mock 事件生产方，P2 后续（Batch F）由 `agent-runtime` 替换。

pub(crate) mod mock;
pub(crate) mod queue;
