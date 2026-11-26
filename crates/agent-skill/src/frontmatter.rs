//! frontmatter 的字段表与校验（D-P5-004）。
//!
//! 只认已知字段，**未知字段忽略**：生态里现成的 Skill 常带 `license` / `allowed-tools` /
//! `metadata` 之类的自有字段，直接拒绝会让它们无法使用，与「兼容开放的 Agent Skills 格式」相反。
//!
//! 字段表：
//!
//! | 字段 | 必填 | 规则 |
//! |---|---|---|
//! | `name` | 是 | `^[a-z0-9][a-z0-9_-]{0,63}$`，全局唯一（唯一性由注册表判定，D-P5-005） |
//! | `description` | 是 | 非空；索引展示时按 `max_description_chars` 截断 |
//! | `requires_tools` | 否 | 字符串数组，每个元素过 `agent_tool::validate_name` |
//! | `version` | 否 | 字符串，原样展示，不做语义化版本校验 |
//! | `author` | 否 | 字符串，原样展示 |

use serde::Deserialize;

use crate::error::SkillError;
use crate::skill::{SkillMetadata, key_line, validate_name, validate_tools};

/// frontmatter 内 YAML 内容的首行（文件第 2 行）。
const CONTENT_FIRST_LINE: usize = 2;

/// 只认已知字段的反序列化目标；未知字段由 serde 默认行为忽略。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawMetadata {
    name: Option<String>,
    description: Option<String>,
    requires_tools: Vec<String>,
    version: Option<String>,
    author: Option<String>,
}

/// 解析 frontmatter 文本（不含首尾分隔符行）并做字段校验。
///
/// `path` 是相对应用根目录的 `SKILL.md` 路径，只用于错误文案。
pub(crate) fn parse(path: &str, frontmatter: &str) -> Result<SkillMetadata, SkillError> {
    let raw: RawMetadata = serde_yaml_ng::from_str(frontmatter).map_err(|error| {
        // 原始错误（英文、可能带 YAML 片段）只进 debug 日志（D-P5-018）。
        tracing::debug!(path, error = %error, "frontmatter 解析失败");
        let line = error
            .location()
            .map_or(CONTENT_FIRST_LINE, |location| CONTENT_FIRST_LINE + location.line() - 1);
        SkillError::InvalidFrontmatter {
            path: path.to_owned(),
            line,
        }
    })?;

    let name = raw.name.unwrap_or_default();
    if name.is_empty() {
        return Err(SkillError::MissingName {
            path: path.to_owned(),
            line: key_line(frontmatter, "name"),
        });
    }
    if !validate_name(&name) {
        return Err(SkillError::InvalidName {
            path: path.to_owned(),
            line: key_line(frontmatter, "name"),
            name,
        });
    }

    let description = raw.description.unwrap_or_default();
    if description.is_empty() {
        return Err(SkillError::MissingDescription {
            path: path.to_owned(),
            line: key_line(frontmatter, "description"),
        });
    }

    validate_tools(path, frontmatter, &raw.requires_tools)?;

    Ok(SkillMetadata {
        name,
        description,
        requires_tools: raw.requires_tools,
        version: raw.version,
        author: raw.author,
    })
}
