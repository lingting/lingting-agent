//! Agent Runtime（P2-022）：实现 `service-core` 定义的执行端口。
//!
//! 一个 `RUNNING` 的 Run 走完整条链路：取全局并发许可 → 组装 Context → 调 `agent-route` 的
//! Provider → 逐段产出事件 → 收尾（落库 + 终态事件）。触发点、会话内串行队列与事件落盘
//! 全部沿用 P1.1（D-P2-028），本 Crate 只替换「执行内容」。
//!
//! 依赖方向：`agent-runtime -> service-core / agent-context / agent-context-core / agent-route /
//! agent-core / types-core / lib-core`；不依赖 `manager-agent` / `lib-web*` / `bin-*`，由启动方
//! `lib-bin-web` 构造后注入。
//!
//! 本阶段只做单轮、无 Tool Calling（P2-022 第 4 条）：请求体不含 tools，代码中也没有 tool loop。
//! 流式期间不写库（D-P2-008）：增量只进事件，助手消息在成功收尾时与 `COMPLETED` 同事务提交。

mod error;
mod feed;
mod limit;
mod loop_;
/// Run 排队与执行并发度的进程内指标。
pub mod metrics;
mod port;

pub use port::AgentRuntime;
