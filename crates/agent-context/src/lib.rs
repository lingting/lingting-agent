//! Context 组装实现（P2-020）：把系统提示词、会话历史与本次用户输入组装成一次请求的
//! Context，并按 token 预算裁剪。
//!
//! 职责边界：
//!
//! - 只组装数据，不读数据库、不感知 Run、不发起 LLM 请求（D-P2-018 的抽象层划分）。
//! - 历史由调用方按 `seq` 升序传入，且不含本次输入；本次输入单独经 `prompt` 传入。
//! - 系统提示词来自 [`ContextBuilder`] 持有目录下的 `AGENTS.md`，每次组装现场读取。
//! - 工具轨迹随历史进入请求；裁剪以「工具调用组」为单位，整组保留或整组丢弃（D-P3-025）。
//!
//! 依赖方向：`agent-context -> agent-context-core / agent-core`；不依赖 `agent-route` /
//! `service-core` / `lib-web*`。

mod builder;
mod estimate;
mod system;
mod trim;

pub use builder::ContextBuilder;
pub use estimate::HeuristicEstimator;
pub use system::SystemPrompt;
