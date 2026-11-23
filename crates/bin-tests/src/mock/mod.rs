//! 测试替身：按被替身领域分子目录（R2）。
//!
//! 本期只有 [`llm`]（内置 OpenAI 兼容 Mock LLM Server）。用例经 `crate::mock::llm` 引用，
//! 替身自身不依赖 `cases`（R4）。

pub(crate) mod llm;
