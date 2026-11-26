//! 调用结果映射：把 MCP 的 `tools/call` 结果变成 P3 的 [`ToolOutput`]（D-P4-017）。
//!
//! 只有文本会原样进模型：`image` / `audio` / `resource` 段替换成固定占位文本，不内联二进制
//! （既不占 token，也不让模型看到无法解析的 base64）。`isError == true` 归错误产出，但文本仍是
//! server 给的内容——失败原因必须回灌，Run 才能继续。

use agent_tool::ToolOutput;
use rmcp::model::{CallToolResult, ContentBlock};

/// 图片段的固定占位文本。
pub const OMITTED_IMAGE: &str = "[图片内容已省略]";
/// 音频段的固定占位文本。
pub const OMITTED_AUDIO: &str = "[音频内容已省略]";
/// 资源段的固定占位文本（内嵌资源与资源链接都按这一句处理）。
pub const OMITTED_RESOURCE: &str = "[资源内容已省略]";
/// 完全没有内容时的兜底文本。
pub const EMPTY_CONTENT: &str = "（MCP 工具没有返回内容）";

/// 映射一次 `tools/call` 的结果。
///
/// 顺序固定：文本段按 server 给的顺序用换行拼接；`content` 为空才退到 `structuredContent`
/// 的 JSON 文本；仍为空才用固定文案。总文本一定非空（空字符串会让模型以为工具没说话）。
pub fn map(result: CallToolResult) -> ToolOutput {
    let mut parts: Vec<String> = Vec::with_capacity(result.content.len());
    for block in &result.content {
        parts.push(match block {
            ContentBlock::Text(text) => text.text.clone(),
            ContentBlock::Image(_) => OMITTED_IMAGE.to_owned(),
            ContentBlock::Audio(_) => OMITTED_AUDIO.to_owned(),
            // `ContentBlock` 是 `#[non_exhaustive]`：未知段按资源处理（同样不内联）。
            _ => OMITTED_RESOURCE.to_owned(),
        });
    }
    let mut text = parts.join("\n");
    if text.trim().is_empty()
        && let Some(structured) = &result.structured_content
        && !structured.is_null()
    {
        text = structured.to_string();
    }
    if text.trim().is_empty() {
        text = EMPTY_CONTENT.to_owned();
    }
    if result.is_error.unwrap_or(false) {
        ToolOutput::error(text)
    } else {
        ToolOutput::ok(text)
    }
}
