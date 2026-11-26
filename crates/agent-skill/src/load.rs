//! 加载：注册表按名交出「正文 + 章节表 + 引用清单」（P5-005）。
//!
//! 这里只做数据搬运：**不判定工具可用性**（那归 [`crate::availability`]，由调用方在渲染结果时拼
//! 上缺失小节，D-P5-009），也**不执行** Skill 声明的任何工具（D-P5-001）。
//!
//! 正文的篇幅口径（`max_inline_chars`、章节二次取用）属于 `skill_load` 的适配器（P5-008），
//! 由它在这份数据之上决定返回全文还是章节目录。

use crate::registry::SkillEntry;
use crate::section::SkillSection;
use crate::skill::SkillMetadata;

/// 一次加载拿到的全部内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillContent {
    /// Skill 的名字。
    pub name: String,
    /// frontmatter 的字段。
    pub metadata: SkillMetadata,
    /// 正文原样文本（含首部空行，行号因此可直接对上文件行号）。
    pub body: String,
    /// 章节表（标题、层级、行号、字符数）。
    pub sections: Vec<SkillSection>,
    /// 引用清单（相对应用根目录）。
    pub references: Vec<String>,
}

/// 由快照条目组装加载结果；调用方保证条目是 `READY`。
pub(crate) fn content(entry: &SkillEntry) -> SkillContent {
    SkillContent {
        name: entry.name.clone(),
        metadata: entry.metadata.clone(),
        body: entry.body.clone(),
        sections: entry.sections.clone(),
        references: entry.references.clone(),
    }
}
