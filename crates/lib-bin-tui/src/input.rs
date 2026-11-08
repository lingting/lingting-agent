//! 输入框：单行编辑缓冲（字符序列 + 光标位置）。
//!
//! 只做编辑，不做渲染：显示宽度、横向裁剪与光标定位都归 [`crate::ui`]。

/// 输入缓冲。
#[derive(Debug, Default)]
pub struct Input {
    /// 已输入的字符；按字符而非字节存放，退格不会切坏多字节字符。
    chars: Vec<char>,
    /// 光标位置：`0` 在行首，`chars.len()` 在行尾。
    cursor: usize,
}

impl Input {
    /// 全部文本。
    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// 光标之前的文本，用于计算光标列。
    pub fn before_cursor(&self) -> String {
        self.chars[..self.cursor].iter().collect()
    }

    /// 清空。
    pub fn clear(&mut self) {
        self.chars.clear();
        self.cursor = 0;
    }

    /// 在光标处插入一个字符。
    pub fn insert(&mut self, value: char) {
        self.chars.insert(self.cursor, value);
        self.cursor += 1;
    }

    /// 删除光标前一个字符。
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    /// 删除光标处字符。
    pub fn delete(&mut self) {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    /// 光标左移。
    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// 光标右移。
    pub fn move_right(&mut self) {
        if self.cursor < self.chars.len() {
            self.cursor += 1;
        }
    }

    /// 光标移到行首。
    pub fn home(&mut self) {
        self.cursor = 0;
    }

    /// 光标移到行尾。
    pub fn end(&mut self) {
        self.cursor = self.chars.len();
    }
}
