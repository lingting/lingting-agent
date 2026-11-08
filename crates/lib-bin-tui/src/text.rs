//! 文本宽度与折行：中英文混排时按显示宽度计算，避免宽字符把界面撑歪。
//!
//! 消息区自己折行、`Paragraph` 不再开 `wrap`，这样滚动位置可以直接由行数算出，
//! 不依赖 Ratatui 的 unstable 行数统计接口。

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// 文本的显示宽度（东亚宽字符按 2 列）。
pub fn width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

/// 按显示宽度折行；`width` 为 0 时不折行。
///
/// 保留原文的换行：原文的每一段单独折行，空段折成空行。
pub fn wrap(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![value.to_string()];
    }
    let mut lines = Vec::new();
    for source in value.split('\n') {
        let mut line = String::new();
        let mut used = 0;
        for value in source.chars() {
            let char_width = UnicodeWidthChar::width(value).unwrap_or(0);
            // 单个字符就超过行宽时也要写出去，否则会死循环。
            if used + char_width > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push(value);
            used += char_width;
        }
        lines.push(line);
    }
    lines
}

/// 从文本头部跳过指定显示宽度，返回剩余部分。
///
/// 用于输入框横向裁剪：光标超出行宽时，让可见区域跟着光标走。
pub fn skip_columns(value: &str, skip: usize) -> String {
    let mut used = 0;
    value
        .chars()
        .skip_while(|value| {
            if used >= skip {
                return false;
            }
            used += UnicodeWidthChar::width(*value).unwrap_or(0);
            true
        })
        .collect()
}
