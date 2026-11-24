//! 工具产出：工具永远返回一段模型可读文本（错误也是文本）。

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
