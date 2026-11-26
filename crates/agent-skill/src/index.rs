//! 索引渲染：把「启用且可用」的 Skill 拼成系统提示里的小节（P5-005 / D-P5-003）。
//!
//! 格式固定（用例逐字符核对），只含「名字 + 描述 + 缺工具标注」，不含正文片段（D-P5-002）：
//!
//! ```text
//! ## 可用 Skill
//!
//! 以下技能提供任务步骤与约束；需要正文时调用 skill_load 获取。
//!
//! - pdf-report：生成 PDF 报表：给定指标与时间范围，产出图表与结论文档。
//! - db-migrate：迁移检查与回滚预案（缺少工具：sqlite3）
//! - （另有 12 个技能因篇幅未展示，可用 skill_load 按名字加载）
//! ```
//!
//! 预算口径（D-P5-010）：按**整行**停止追加，不输出半行；`chars` 含标题、说明、每条索引行与
//! 结尾的「另有 N 个」，因此恒有 `chars ≤ max_index_chars`。预算小到连标题与说明都放不下时返回
//! 空小节——宁可不展示，也不破预算。

use crate::availability::{self, ToolAvailability};
use crate::registry::{SkillEntry, SkillLimits};

/// 索引小节标题。
pub const INDEX_HEADING: &str = "## 可用 Skill";
/// 索引小节开头的一句说明。
pub const INDEX_INTRO: &str = "以下技能提供任务步骤与约束；需要正文时调用 skill_load 获取。";
/// 索引行的名字与描述之间的分隔符。
const NAME_SEPARATOR: &str = "：";
/// 截断描述时的省略号。
const ELLIPSIS: char = '…';

/// 一次索引渲染的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillIndex {
    /// 小节文本；没有任何可用 Skill 时为空串。
    pub section: String,
    /// 实际展示的 Skill 数量。
    pub shown: usize,
    /// 因篇幅未展示的数量（`shown + hidden` 等于启用且可用的总数）。
    pub hidden: usize,
    /// 小节字符数（`chars()` 计数，含标题、说明与结尾那行）。
    pub chars: usize,
}

/// 渲染索引小节；`views` 来自 [`crate::SkillRegistry::entries`]。
/// 渲染索引小节；`entries` 是快照的全部条目，`enabled` 给出启用状态（注册表只取读锁传进来，
/// 因此渲染不会克隆正文）。
pub(crate) fn render(
    entries: &[SkillEntry],
    enabled: impl Fn(&str) -> bool,
    limits: &SkillLimits,
    availability: &dyn ToolAvailability,
) -> SkillIndex {
    let lines: Vec<String> = entries
        .iter()
        .filter(|entry| entry.is_ready() && enabled(&entry.name))
        .map(|entry| line(entry, limits, availability))
        .collect();
    if lines.is_empty() {
        return SkillIndex::default();
    }

    let prefix = format!("{INDEX_HEADING}\n\n{INDEX_INTRO}\n\n");
    let max = limits.max_index_chars;
    let mut chars = prefix.chars().count();
    let mut shown = 0;
    for candidate in &lines {
        let add = extra(candidate, shown);
        if chars + add > max {
            break;
        }
        chars += add;
        shown += 1;
    }

    let mut hidden = lines.len() - shown;
    // 还有没展示的：给「另有 N 个」腾出一整行的空间（必要时从尾部回退整行）。
    while hidden > 0 {
        let summary = summary_line(hidden);
        let add = extra(&summary, shown);
        if chars + add <= max {
            let mut items: Vec<&str> = lines[..shown].iter().map(String::as_str).collect();
            items.push(summary.as_str());
            return build(&prefix, &items, shown, hidden);
        }
        if shown == 0 {
            // 连「另有 N 个」都放不下：不输出任何内容，预算优先。
            return SkillIndex::default();
        }
        shown -= 1;
        hidden += 1;
        chars -= extra(&lines[shown], shown);
    }

    let items: Vec<&str> = lines.iter().map(String::as_str).collect();
    build(&prefix, &items, shown, 0)
}

/// 一条索引行（名字 + 描述 + 缺工具后缀）。
fn line(entry: &SkillEntry, limits: &SkillLimits, availability: &dyn ToolAvailability) -> String {
    let missing = availability::missing_tools(&entry.metadata.requires_tools, availability);
    format!(
        "- {}{NAME_SEPARATOR}{}{}",
        entry.name,
        describe(&entry.metadata.description, limits.max_description_chars),
        availability::index_suffix(&missing)
    )
}

/// 结尾那行：固定文案，只有数字变化。
fn summary_line(hidden: usize) -> String {
    format!("- （另有 {hidden} 个技能因篇幅未展示，可用 skill_load 按名字加载）")
}

/// 追加一行需要多少个字符（非首行要多一个换行）。
fn extra(line: &str, shown: usize) -> usize {
    line.chars().count() + usize::from(shown > 0)
}

/// 拼出小节：前缀 + 各行（`\n` 分隔），`chars` 由拼好的文本复算。
fn build(prefix: &str, items: &[&str], shown: usize, hidden: usize) -> SkillIndex {
    let section = format!("{prefix}{}", items.join("\n"));
    SkillIndex {
        chars: section.chars().count(),
        section,
        shown,
        hidden,
    }
}

/// 索引里的描述：先折成单行（换行会破坏一条一行的格式），再按上限截断并加省略号。
///
/// 截断后的长度**含**省略号（恒 `≤ max`），因此索引每一行的宽度都可预算。
fn describe(text: &str, max: usize) -> String {
    let flat = flatten(text);
    if flat.chars().count() <= max {
        return flat;
    }
    if max == 0 {
        return String::new();
    }
    let mut truncated: String = flat.chars().take(max - 1).collect();
    truncated.push(ELLIPSIS);
    truncated
}

/// 把任意空白（含换行）折成单个空格，并去掉首尾空白。
fn flatten(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(character);
    }
    out
}
