//! 工具产出：工具永远返回一段模型可读文本（错误也是文本）。

/// 截断提示的前缀：整句固定，模型据此知道「看到的不是全量」（D-P3-007）。
const TRUNCATE_HINT: &str = "\n…（结果已截断，原文共 ";

/// 一次工具调用的产出。
///
/// OpenAI 兼容协议的工具结果没有 error 字段，失败只能靠文本表达，因此这里不区分「错误类型」，
/// 只区分「是不是错误」与「有没有被截断」（D-P3-007）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// 回灌给模型的文本。
    pub text: String,
    /// 是否为错误文本。
    pub is_error: bool,
    /// 是否因为超出 `[tools] max_result_chars` 被截断。
    pub truncated: bool,
}

impl ToolOutput {
    /// 成功产出。
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
            truncated: false,
        }
    }

    /// 失败产出（文本必须自己说清「失败了什么」）。
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
            truncated: false,
        }
    }
}

/// 按**字符**截断到 `max_chars`，返回（截断后的文本，是否发生截断）。
///
/// 边界只落在字符之间，中文与 emoji 不会被截半；发生截断时追加固定提示并点明原文长度，
/// 让模型知道「还差多少」（D-P3-007）。审计里存同样的截断文本，不留全量。
pub fn truncate(text: &str, max_chars: usize) -> (String, bool) {
    let total = text.chars().count();
    if total <= max_chars {
        return (text.to_owned(), false);
    }
    let head: String = text.chars().take(max_chars).collect();
    (format!("{head}{TRUNCATE_HINT}{total} 字符）"), true)
}
