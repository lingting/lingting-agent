//! `SKILL.md` 的解析入口：`bytes` → [`SkillFile`]（元数据 + 正文 + 章节表 + 引用清单）。
//!
//! 解析器不读配置、不碰文件系统：整文件字节上限由调用方注入（`max_skill_bytes`），路径由调用方
//! 给出**相对应用根目录**的形态（`skills/<dir>/SKILL.md`），因此错误文案天然不含绝对路径
//! （D-P5-018）。引用清单需要读目录，单独由 [`crate::scan_references`] 产出后
//! 用 [`SkillFile::with_references`] 填进来。
//!
//! `SKILL.md` 的形状（D-P5-004）：
//!
//! ```text
//! ---
//! name: pdf-report
//! description: 按模板产出 PDF 报表
//! requires_tools: [file_read, file_search]
//! version: "1"
//! author: lingting
//! ---
//!
//! # 步骤
//!
//! 1. 先读模板……
//! ```
//!
//! frontmatter 必须以独占一行的 `---` 开始（容忍 BOM）与结束，中间是 YAML；未知字段忽略，
//! 不产生告警。`name` 与目录名**允许**不一致（目录只是存放位置，D-P5-004）。

use agent_tool::validate_name as validate_tool_name;

use crate::error::SkillError;
use crate::frontmatter;
use crate::section::{self, SkillSection};

/// frontmatter 起始行号（`SKILL.md` 的首行，即第一个 `---`）。
const FRONTMATTER_LINE: usize = 1;
/// frontmatter 内容的首行（首行 `---` 之后的下一行）。
const FRONTMATTER_BODY_LINE: usize = FRONTMATTER_LINE + 1;
/// frontmatter 分隔符。
const DELIMITER: &str = "---";
/// Skill 名字的最大长度（`^[a-z0-9][a-z0-9_-]{0,63}$` 的量词上限）。
const MAX_NAME_LEN: usize = 64;

/// 一份 `SKILL.md` 解析后的全部结构化数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFile {
    /// frontmatter 的字段。
    pub metadata: SkillMetadata,
    /// frontmatter 之后的正文（原样保留，含首部空行；行号因此可直接对上文件行号）。
    pub body: String,
    /// ATX 标题切分出的章节，按出现顺序。
    pub sections: Vec<SkillSection>,
    /// `references/` 下的引用清单，路径相对应用根目录。
    pub references: Vec<String>,
}

/// frontmatter 的字段表（未知字段不进这里，D-P5-004）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillMetadata {
    /// 逻辑标识，全局唯一，满足 [`validate_name`]。
    pub name: String,
    /// 非空描述；索引展示时按 `max_description_chars` 截断。
    pub description: String,
    /// 声明依赖的工具名；由索引渲染与加载路径判定此刻是否可用（D-P5-009）。
    pub requires_tools: Vec<String>,
    /// 版本，原样展示，不做语义化版本校验。
    pub version: Option<String>,
    /// 作者，原样展示。
    pub author: Option<String>,
}

impl SkillFile {
    /// 填入引用清单（发现层扫描 `references/` 后调用）。
    pub fn with_references(mut self, references: Vec<String>) -> Self {
        self.references = references;
        self
    }
}

/// Skill 名字规则：`^[a-z0-9][a-z0-9_-]{0,63}$`（D-P5-005）。
///
/// 大小写敏感、不做归一化：静默改写名字会让用户按原名加载失败。`lib-bin-cli` 装载
/// `skills.toml` 的 `disabled` 名单时有一份逐字符一致的本地实现（该 crate 不依赖本 crate）。
pub fn validate_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_NAME_LEN {
        return false;
    }
    match bytes.split_first() {
        Some((first, rest)) => {
            (first.is_ascii_lowercase() || first.is_ascii_digit())
                && rest
                    .iter()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-')
        }
        None => false,
    }
}

/// 解析一份 `SKILL.md`。
///
/// `path` 是**相对应用根目录**的路径（用于错误文案），`bytes` 是文件内容，`max_bytes` 是整文件
/// 字节上限（由调用方从 `skills.toml` 注入）。
pub fn parse(path: &str, bytes: &[u8], max_bytes: u64) -> Result<SkillFile, SkillError> {
    let actual = bytes.len() as u64;
    if actual > max_bytes {
        return Err(SkillError::TooLarge {
            path: path.to_owned(),
            max_bytes,
            actual,
        });
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SkillError::NotUtf8 { path: path.to_owned() })?;
    // BOM 只在首行之前，剥掉之后分隔符判断与行号都不受影响。
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let (frontmatter, body, body_line) = split_frontmatter(path, text)?;
    let metadata = frontmatter::parse(path, frontmatter)?;
    if body.trim().is_empty() {
        return Err(SkillError::EmptyBody {
            path: path.to_owned(),
            line: body_line,
        });
    }
    let sections = section::split(body, body_line);
    Ok(SkillFile {
        metadata,
        body: body.to_owned(),
        sections,
        references: Vec::new(),
    })
}

/// 切出 frontmatter 与正文，返回（frontmatter 文本, 正文, 正文首行行号）。
///
/// 分隔符必须独占一行（允许行尾空白）；首行不是 `---` 与找不到结束分隔符是两种不同的错误。
fn split_frontmatter<'a>(path: &str, text: &'a str) -> Result<(&'a str, &'a str, usize), SkillError> {
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return Err(SkillError::MissingFrontmatter { path: path.to_owned() });
    };
    if !is_delimiter(first) {
        return Err(SkillError::MissingFrontmatter { path: path.to_owned() });
    }

    let start = first.len();
    let mut offset = start;
    for line in lines {
        if is_delimiter(line) {
            let end = offset + line.len();
            let body_line = text
                .get(..end)
                .map_or(FRONTMATTER_BODY_LINE, |head| head.lines().count() + 1);
            return Ok((&text[start..offset], &text[end..], body_line));
        }
        offset += line.len();
    }
    Err(SkillError::UnterminatedFrontmatter {
        path: path.to_owned(),
        line: FRONTMATTER_LINE,
    })
}

/// 一行是否就是分隔符（行尾空白与 `\r` 不算内容）。
fn is_delimiter(line: &str) -> bool {
    line.trim_end() == DELIMITER
}

/// frontmatter 里某个键所在的行号（绝对行号）；找不到时回落到 frontmatter 起始行。
///
/// 只用于「缺字段 / 字段不合规」这类没有 YAML 位置可用的错误，让用户能一眼定位。
pub(crate) fn key_line(frontmatter: &str, key: &str) -> usize {
    for (index, line) in frontmatter.lines().enumerate() {
        let head = format!("{key}:");
        if line.trim_start().starts_with(&head) {
            return FRONTMATTER_BODY_LINE + index;
        }
    }
    FRONTMATTER_BODY_LINE
}

/// `requires_tools` 的元素校验：每个都必须是合法工具名（与工具注册表同一规则）。
pub(crate) fn validate_tools(path: &str, frontmatter: &str, tools: &[String]) -> Result<(), SkillError> {
    for name in tools {
        if !validate_tool_name(name) {
            return Err(SkillError::InvalidToolName {
                path: path.to_owned(),
                line: key_line(frontmatter, "requires_tools"),
                name: name.clone(),
            });
        }
    }
    Ok(())
}
