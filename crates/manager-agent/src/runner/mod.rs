//! Run 执行：队列与取消信号登记（P1.1 的执行通路在装配层的落位）。
//!
//! 归属（D-P2-035）：Run 的执行同时需要「会话业务」（写库）与「运行时会话」（事件生产方），
//! 这类跨组件动作放在装配层 `manager-agent`，`service-core` 只提供业务与数据访问。
//!
//! - [`queue`]：随运行时会话启动的串行执行队列，执行内容交给注入的 `RunDriver`（P2-023）。
//! - [`resume`]：消息触发的恢复段，先拾取该会话的 `PAUSED` Run（P6A-011 / D-P6-009）。
//! - [`cancel`]：在跑 Run 的取消信号登记表，三条终止路径都经它触发（P2-024）。

pub(crate) mod cancel;
pub(crate) mod queue;
pub(crate) mod resume;
