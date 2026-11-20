//! Provider / Model 解析与 LLM 调用适配（P2-015）。
//!
//! 本 Crate 把「用哪个模型、怎么调」从 Agent Runtime 里隔离出来：上层只见项目自有的
//! [`LlmProvider`] / [`LlmRequest`] / [`LlmStreamEvent`] / [`LlmError`]，Rig 的类型与错误
//! 只允许出现在 `rig` 子模块里（D-P2-001）。
//!
//! 依赖方向：`agent-route -> agent-core / types-core / lib-core`（另加第三方 `rig-core`）；
//! 不依赖 `service-core` / `agent-runtime` / `lib-web*`。Provider 配置的**装载**（文件解析、环境
//! 无关的启动期校验）归启动方 `lib-bin-cli`（P2-017），本 Crate 只负责装载结果的只读承载与模型解析
//! （[`ProviderService`]）。
//!
//! 取消语义：调用方直接 drop [`LlmStream`]，实现方不得在后台继续读取（D-P2-001 的 O-1）。

mod error;
mod model;
mod provider;
pub mod rig;

pub use error::LlmError;
pub use model::ModelRoute;
pub use provider::{LlmMessage, LlmProvider, LlmRequest, LlmStream, LlmStreamEvent, LlmStreamFuture, ProviderService};
