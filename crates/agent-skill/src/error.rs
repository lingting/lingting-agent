//! Skill 侧的错误归一：文案表只有一个产点（D-P5-018）。
//!
//! `Display` 形如 `skills/<dir>/SKILL.md:<行号>: <原因>`（无行号的错误省掉行号段），
//! **不含绝对路径、不含正文、不含 YAML 原文**：路径一律是相对应用根目录的路径，由调用方传入。
//! 解析器的原始错误（英文、带 YAML 片段）只经 `tracing::debug` 落地，既不进事件也不进响应体。

use std::fmt;

/// frontmatter 内 YAML 错误的通用原因（语法或字段类型不对，行号已在文案前缀里）。
pub const INVALID_FRONTMATTER: &str = "frontmatter 不是合法的 YAML，或字段类型不对";
/// 文件超出上限。
pub const TOO_LARGE: &str = "文件超出大小上限";
/// 不是 UTF-8。
pub const NOT_UTF8: &str = "文件不是 UTF-8 文本";
/// 缺少 frontmatter。
pub const MISSING_FRONTMATTER: &str = "缺少 frontmatter（首行必须是独占一行的 ---）";
/// frontmatter 没有结束分隔符。
pub const UNTERMINATED_FRONTMATTER: &str = "frontmatter 缺少结束分隔符（独占一行的 ---）";
/// 缺少 `name`。
pub const MISSING_NAME: &str = "frontmatter 缺少 name";
/// `name` 不合规。
pub const INVALID_NAME: &str = "name 不合规";
/// 缺少 `description`。
pub const MISSING_DESCRIPTION: &str = "frontmatter 缺少 description";
/// `requires_tools` 里有不合规的工具名。
pub const INVALID_TOOL_NAME: &str = "requires_tools 含不合规的工具名";
/// 正文为空。
pub const EMPTY_BODY: &str = "正文不能为空";
/// `references/` 无法读取。
pub const UNREADABLE_REFERENCES: &str = "references 目录无法读取";

/// Skill 侧的唯一错误类型。
///
/// 每个取值对应一句固定文案；拼进来的只有相对路径、行号与用户自己写在 `SKILL.md` 里的标识，
/// 不含绝对路径与正文片段（判据 E-3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillError {
    /// 整文件超出 `max_skill_bytes`。
    TooLarge {
        /// 相对应用根目录的 `SKILL.md` 路径。
        path: String,
        /// 上限（字节）。
        max_bytes: u64,
        /// 实际字节数。
        actual: u64,
    },
    /// 文件不是 UTF-8。
    NotUtf8 {
        /// 相对应用根目录的 `SKILL.md` 路径。
        path: String,
    },
    /// 首行不是独占一行的 `---`。
    MissingFrontmatter {
        /// 相对应用根目录的 `SKILL.md` 路径。
        path: String,
    },
    /// 没有找到结束分隔符。
    UnterminatedFrontmatter {
        /// 相对应用根目录的 `SKILL.md` 路径。
        path: String,
        /// frontmatter 起始行号。
        line: usize,
    },
    /// YAML 语法或字段类型错误；`line` 取 `serde_yaml_ng` 给出的位置。
    InvalidFrontmatter {
        /// 相对应用根目录的 `SKILL.md` 路径。
        path: String,
        /// 出错的行号。
        line: usize,
    },
    /// 缺少 `name`（或为空）。
    MissingName {
        /// 相对应用根目录的 `SKILL.md` 路径。
        path: String,
        /// 行号。
        line: usize,
    },
    /// `name` 不满足 `^[a-z0-9][a-z0-9_-]{0,63}$`。
    InvalidName {
        /// 相对应用根目录的 `SKILL.md` 路径。
        path: String,
        /// 行号。
        line: usize,
        /// 不合规的名字。
        name: String,
    },
    /// 缺少 `description`（或为空）。
    MissingDescription {
        /// 相对应用根目录的 `SKILL.md` 路径。
        path: String,
        /// 行号。
        line: usize,
    },
    /// `requires_tools` 的元素不是合法工具名。
    InvalidToolName {
        /// 相对应用根目录的 `SKILL.md` 路径。
        path: String,
        /// 行号。
        line: usize,
        /// 不合规的工具名。
        name: String,
    },
    /// frontmatter 之后没有内容。
    EmptyBody {
        /// 相对应用根目录的 `SKILL.md` 路径。
        path: String,
        /// 行号。
        line: usize,
    },
    /// `references/` 读不出来（权限、IO 失败）。
    UnreadableReferences {
        /// 相对应用根目录的 `references` 目录路径。
        path: String,
    },
}

impl SkillError {
    /// 错误指向的相对路径（文案前缀）。
    pub fn path(&self) -> &str {
        match self {
            Self::TooLarge { path, .. }
            | Self::NotUtf8 { path }
            | Self::MissingFrontmatter { path }
            | Self::UnterminatedFrontmatter { path, .. }
            | Self::InvalidFrontmatter { path, .. }
            | Self::MissingName { path, .. }
            | Self::InvalidName { path, .. }
            | Self::MissingDescription { path, .. }
            | Self::InvalidToolName { path, .. }
            | Self::EmptyBody { path, .. }
            | Self::UnreadableReferences { path } => path,
        }
    }

    /// 错误对应的行号；没有行号语义的错误（大小、编码、IO）返回 `None`。
    pub fn line(&self) -> Option<usize> {
        match self {
            Self::TooLarge { .. } | Self::NotUtf8 { .. } | Self::MissingFrontmatter { .. } => None,
            Self::UnterminatedFrontmatter { line, .. }
            | Self::InvalidFrontmatter { line, .. }
            | Self::MissingName { line, .. }
            | Self::InvalidName { line, .. }
            | Self::MissingDescription { line, .. }
            | Self::InvalidToolName { line, .. }
            | Self::EmptyBody { line, .. } => Some(*line),
            Self::UnreadableReferences { .. } => None,
        }
    }

    /// 一句中文原因；变量部分只拼用户自己写下的标识与上限数字。
    fn reason(&self) -> String {
        match self {
            Self::TooLarge { max_bytes, actual, .. } => {
                format!("{TOO_LARGE}：{actual} 字节 > {max_bytes} 字节")
            }
            Self::NotUtf8 { .. } => NOT_UTF8.to_owned(),
            Self::MissingFrontmatter { .. } => MISSING_FRONTMATTER.to_owned(),
            Self::UnterminatedFrontmatter { .. } => UNTERMINATED_FRONTMATTER.to_owned(),
            Self::InvalidFrontmatter { .. } => INVALID_FRONTMATTER.to_owned(),
            Self::MissingName { .. } => MISSING_NAME.to_owned(),
            Self::InvalidName { name, .. } => {
                format!("{INVALID_NAME}：{name}（规则 ^[a-z0-9][a-z0-9_-]{{0,63}}$）")
            }
            Self::MissingDescription { .. } => MISSING_DESCRIPTION.to_owned(),
            Self::InvalidToolName { name, .. } => {
                format!("{INVALID_TOOL_NAME}：{name}（规则 ^[a-z][a-z0-9_]{{1,63}}$）")
            }
            Self::EmptyBody { .. } => EMPTY_BODY.to_owned(),
            Self::UnreadableReferences { .. } => UNREADABLE_REFERENCES.to_owned(),
        }
    }
}

impl fmt::Display for SkillError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line() {
            Some(line) => write!(formatter, "{}:{}: {}", self.path(), line, self.reason()),
            None => write!(formatter, "{}: {}", self.path(), self.reason()),
        }
    }
}

impl std::error::Error for SkillError {}
