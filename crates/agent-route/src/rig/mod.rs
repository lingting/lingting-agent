//! Rig Adapter：把 `rig-core` 的 OpenAI 兼容客户端接到项目自有的 LLM 抽象上（P2-016）。
//!
//! **Rig 的类型只允许出现在本模块内**（D-P2-001 第 2 条），上一级只见 [`crate::LlmProvider`]。
//! 子模块分工：
//!
//! - [`openai`]：客户端构造与请求翻译（`OpenAIConfig` + `with_base_url` + `Route::Chat`）。
//! - [`stream`]：`Streamed<Completion>` → [`crate::LlmStreamEvent`]。
//! - [`error`]：`ProviderError` → [`crate::LlmError`]。
//!
//! 取消：上层 drop [`crate::LlmStream`] 即 drop `Streamed`，Rig 的读取随之结束，
//! 不存在后台任务继续读流（D-P2-001 的 O-1）。

mod error;
mod openai;
mod stream;

pub use openai::OpenAiProvider;
