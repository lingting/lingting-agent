//! `skill_load`：Skill 侧唯一的工具（P5-008 / D-P5-011）。
//!
//! 它只做**检索**：从注册表取快照里的正文与章节表，按篇幅决定返回全文、章节目录还是某一节。
//! 不读文件系统、不派生进程、不执行 Skill 声明的任何工具（D-P5-001 / D-P5-013）。
//!
//! 结果文本结构固定（用例逐段核对）：
//!
//! ```text
//! # 技能：db-migrate
//!
//! <正文 / 章节目录 / 指定章节>
//!
//! ## 缺少的工具
//! - sqlite3
//!
//! 涉及这些工具的步骤必须停止，并明确告知用户无法执行。
//!
//! ## 参考资料（用 file_read 按路径读取）
//! - skills/db-migrate/references/rollback.md
//! ```
//!
//! 篇幅口径（D-P5-010）：整段返回文本（含首行、缺工具小节与参考资料清单）不超过
//! `max_inline_chars`；超长的正文按三级披露退回章节目录，指定章节则截断并写明剩余字符数。

use std::sync::{Arc, LazyLock};

use agent_tool::{Tool, ToolContext, ToolError, ToolFileAccess, ToolOutput, ToolRisk, ToolSchema, schema};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::availability::{self, ToolAvailability};
use crate::error::SkillError;
use crate::load::SkillContent;
use crate::registry::SkillRegistry;
use crate::section;

/// 工具名（满足 `agent_tool::validate_name`）。
pub const TOOL_NAME: &str = "skill_load";
/// 工具说明：模型据此判断「什么时候该查技能」。
const DESCRIPTION: &str = "加载技能的完整指令；技能较大时先给章节目录，再用 `section` 取某一节。";
/// 结果首行的前缀。
const HEADING_PREFIX: &str = "# 技能：";
/// 章节目录的标题。
const CONTENTS_HEADING: &str = "## 章节目录";
/// 章节目录之后的一句提示（`section` 参数是取单节的唯一入口）。
const CONTENTS_HINT: &str = "请用 section 参数按节读取。";
/// 参考资料小节的标题：读取交给既有 `file_read`，本工具不内联文件内容（D-P5-012）。
const REFERENCES_HEADING: &str = "## 参考资料（用 file_read 按路径读取）";
/// 正文为空时的明确文案（不返回空字符串）。
const EMPTY_BODY: &str = "（该技能正文为空）";
/// 正文没有章节标题、又超出篇幅时的引导语：此时无法给出章节目录，只能给截断后的正文。
const NO_SECTIONS: &str = "（该技能正文没有章节标题，以下为正文，已按篇幅截断）";
/// 未知章节的提示里最多列出的章节数（章节再多也只是为了让模型改对一次）。
const MAX_SECTION_HINTS: usize = 10;

/// 参数 schema：由 [`SkillLoadArgs`] 派生一次，进程内只算一遍（D-P3-002）。
static SCHEMA: LazyLock<ToolSchema> = LazyLock::new(schema::of::<SkillLoadArgs>);

/// `skill_load` 的参数。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillLoadArgs {
    /// 技能名（`SKILL.md` frontmatter 的 `name`）。
    pub name: String,
    /// 可选的章节标题（可省略 `#` 前缀）；省略时按篇幅返回全文或章节目录。
    #[serde(default)]
    pub section: Option<String>,
}

/// 只读的 Skill 检索工具。
pub struct SkillLoad {
    /// 快照、启用状态与上限的唯一入口。
    registry: Arc<SkillRegistry>,
    /// 工具可用性判定器：缺工具只提示、不阻断（D-P5-009）。
    availability: Arc<dyn ToolAvailability>,
}

impl SkillLoad {
    /// 装配工具：注册表与判定器都来自同一次装配（`manager-agent` 注册它）。
    pub fn new(registry: Arc<SkillRegistry>, availability: Arc<dyn ToolAvailability>) -> Self {
        Self { registry, availability }
    }
}

#[async_trait::async_trait]
impl Tool for SkillLoad {
    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn schema(&self) -> &'static ToolSchema {
        &SCHEMA
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn file_access(&self) -> ToolFileAccess {
        // 只读应用目录内的 Skill 文件（正文与引用清单都来自这些文件，D-P5-011）。
        ToolFileAccess::Read
    }

    async fn call(&self, _ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        let args: SkillLoadArgs = schema::parse(args)?;
        let content = self.registry.load(&args.name).map_err(failure)?;
        let missing = availability::missing_tools(&content.metadata.requires_tools, self.availability.as_ref());
        let text = render(
            &content,
            args.section.as_deref(),
            &missing,
            self.registry.limits().max_inline_chars,
        )
        .map_err(failure)?;
        Ok(ToolOutput::ok(text))
    }
}

/// Skill 侧错误 → 工具结果：`Display` 来自 `error.rs` 的单一产点，模型能读懂且不含绝对路径。
///
/// 未知技能 / 禁用 / `INVALID` / 未知章节都落在这条路径上：Run 不 `FAILED`，模型可以继续下一轮
/// （D-P5-011）。
fn failure(error: SkillError) -> ToolError {
    ToolError::Failed(error.to_string())
}
/// 拼出一次加载的结果文本；`max` 是整段返回值的字符上限。
fn render(content: &SkillContent, section: Option<&str>, missing: &[String], max: usize) -> Result<String, SkillError> {
    let header = format!("{HEADING_PREFIX}{}", content.name);
    let tail = tail(content, missing);
    let budget = payload_budget(&header, &tail, max);

    let payload = match section {
        Some(title) => section_payload(content, title, budget)?,
        None => body_payload(content, budget),
    };
    let text = compose(&header, &payload, &tail);
    // 兜底：包装行（超长的引用清单、极小的上限）也可能把总量顶过上限，最后再截一次。
    Ok(clip(&text, max, Clip::Result))
}

/// 首行与尾部小节之后的可用预算。
fn payload_budget(header: &str, tail: &str, max: usize) -> usize {
    let mut fixed = header.chars().count() + 2;
    if !tail.is_empty() {
        fixed += 2 + tail.chars().count();
    }
    max.saturating_sub(fixed)
}

/// 尾部小节：缺工具（有缺失时）与参考资料清单（有引用时）。
fn tail(content: &SkillContent, missing: &[String]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(section) = availability::missing_section(missing) {
        parts.push(section);
    }
    if let Some(references) = references(&content.references) {
        parts.push(references);
    }
    parts.join("\n\n")
}

/// 参考资料清单：只有相对应用根目录的路径字符串，不读取、不执行（D-P5-012 / D-P5-013）。
fn references(references: &[String]) -> Option<String> {
    if references.is_empty() {
        return None;
    }
    let mut text = String::from(REFERENCES_HEADING);
    for path in references {
        text.push_str("\n- ");
        text.push_str(path);
    }
    Some(text)
}

/// 不带 `section`：装得下就返回全文，装不下退回章节目录。
fn body_payload(content: &SkillContent, budget: usize) -> String {
    let body = content.body.trim();
    if body.is_empty() {
        return EMPTY_BODY.to_owned();
    }
    if body.chars().count() <= budget {
        return body.to_owned();
    }
    contents(content, budget)
}

/// 章节目录：每节 `标题（n 字）`，末尾一句「用 section 按节读取」。
fn contents(content: &SkillContent, budget: usize) -> String {
    if content.sections.is_empty() {
        // 没有章节可列：给截断后的正文，比一句「请按节读取」更有用（P5-009 第 4 条）。
        let text = format!("{NO_SECTIONS}\n\n{}", content.body.trim());
        return clip(&text, budget, Clip::Result);
    }
    let items: Vec<String> = content
        .sections
        .iter()
        .map(|section| format!("- {}（{} 字）", section.title, section.chars))
        .collect();
    let text = format!("{CONTENTS_HEADING}\n\n{}\n\n{CONTENTS_HINT}", items.join("\n"));
    clip(&text, budget, Clip::Result)
}

/// 带 `section`：按标题精确匹配（允许省略 `#` 前缀与前后空白），只返回该节（含子节）。
fn section_payload(content: &SkillContent, title: &str, budget: usize) -> Result<String, SkillError> {
    let wanted = normalize(title);
    let found = content.sections.iter().position(|item| item.title == wanted);
    let Some(index) = found else {
        return Err(SkillError::UnknownSection {
            name: content.name.clone(),
            section: title.trim().to_owned(),
            available: content
                .sections
                .iter()
                .take(MAX_SECTION_HINTS)
                .map(|item| item.title.clone())
                .collect(),
        });
    };
    let text = section::text(&content.body, index).trim();
    if text.is_empty() {
        return Ok(EMPTY_BODY.to_owned());
    }
    Ok(clip(text, budget, Clip::Section))
}

/// 章节名的比较形态：去掉前后空白与 `#` 前缀（模型可能照着 `## 步骤` 抄过来）。
fn normalize(title: &str) -> &str {
    title.trim().trim_start_matches('#').trim()
}

/// 拼结果：首行、正文段、尾部小节之间各空一行；空段不占位（不留空行）。
fn compose(header: &str, payload: &str, tail: &str) -> String {
    [header, payload, tail]
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 截断风格：决定说明文案（`skill_load` 的两种「不完整」意思不同）。
#[derive(Clone, Copy)]
enum Clip {
    /// 指定章节被截断：说明这一节还剩多少字符。
    Section,
    /// 整段结果被截断：说明原文总长。
    Result,
}

/// 按字符预算截断并附一句说明；说明计入预算，因此结果恒 `≤ budget`。
///
/// 说明里的数字依赖保留长度、保留长度又依赖说明长度，这里用 `total` 的位数做上界估一次：
/// `rest ≤ total` 时说明只会更短，结果不可能超预算。
fn clip(text: &str, budget: usize, style: Clip) -> String {
    let total = text.chars().count();
    if total <= budget {
        return text.to_owned();
    }
    // 预算连说明都放不下：宁可什么都不给，也不返回一句超预算的提示。
    let note_cost = note(style, total).chars().count();
    if note_cost >= budget {
        return String::new();
    }
    let keep = budget - note_cost;
    let head: String = text.chars().take(keep).collect();
    format!("{head}{}", note(style, total - keep))
}

/// 截断说明（前缀固定，只有剩余字符数变化）。
fn note(style: Clip, rest: usize) -> String {
    match style {
        Clip::Section => format!("\n\n（本节已截断，剩余 {rest} 字符）"),
        Clip::Result => format!("\n\n（已截断，剩余 {rest} 字符）"),
    }
}
