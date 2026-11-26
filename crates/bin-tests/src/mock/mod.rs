//! 测试替身：按被替身领域分子目录（R2）。
//!
//! 本期有两个：[`llm`]（内置 OpenAI 兼容 Mock LLM Server）与 [`mcp`]（会讲 MCP 的子进程，
//! P4-017）。用例经 `crate::mock::llm` / `crate::mock::mcp` 引用，替身自身不依赖 `cases`（R4）。

pub(crate) mod llm;
pub(crate) mod mcp;
