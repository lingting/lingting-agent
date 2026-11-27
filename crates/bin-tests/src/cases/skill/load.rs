//! `skill.load`（P5-008 / P5-005 / P5-017）：`skill_load` 的三级披露与失败文案。
//!
//! 证据两段：
//!
//! - 进程内：直接调 `SkillLoad`（与模型调用的是同一个工具实现），核对全文 / 章节目录 / 单节三种
//!   返回形态、字符数与章节表的对应关系、四种失败文案；
//! - 请求级：私有服务端 + 私有 Mock，第一轮就要求模型调 `skill_load` 读一个 `INVALID` 的 Skill，
//!   用 `api/tool/audit/list` 的结果断言「错误文案原样回灌、Run 不 FAILED」。
//!
//! 不读日志。

use agent_skill::{SkillLimits, SkillLoad};
use agent_tool::{Tool, ToolFileAccess, ToolRisk};
use anyhow::{Context, Result, ensure};
use lib_bin_cli::sdk::{RunEventKind, ToolCallStatus};
use serde_json::json;

use super::{Available, call_load, case, invalid_entry, ready_entry, registry_of, serve_with, write_skill};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::cases::tool::{audit, events_of};
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::flow::Occupied;
use crate::util::paths::skill_home;

/// 小正文：一节、一段。
const SMALL: &str = "---\nname: alpha\ndescription: 小技能\n---\n\n# 步骤\n\n1. 先读模板。\n";
/// 小正文的正文（`frontmatter` 之后，`trim` 后）。
const SMALL_BODY: &str = "# 步骤\n\n1. 先读模板。";
/// 引用材料（相对应用根目录，工具只列路径、不读内容）。
const REFERENCE: &str = "skills/alpha/references/notes.md";
/// 大正文：三个同级章节，总字数超过 `max_inline_chars`（120），每节都装得下。
const BIG: &str = "---\nname: big\ndescription: 大技能\n---\n\n\
                   ## 步骤\n\n第一步先读模板，确认章节顺序与必填字段，缺一个字段就停下来问清楚。\n\n\
                   ## 细则\n\n第二步再查数据，缺失的字段必须写成未提供，不允许用零值代替。\n\n\
                   ## 收尾\n\n第三步汇总结论，每条结论都要能追溯到明细里的具体字段与时间点。\n";
/// `INVALID`：缺 `name`，工具执行路径上应拿到「技能不可用」+ 解析期的可定位原因。
const BROKEN: &str = "---\ndescription: 坏技能\n---\n\n# 步骤\n\n1. 什么也不做。\n";

/// 加载用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.load", CaseKind::SelfServed, load)]
}

/// 进程内三段 + 请求级一段。
fn load(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let home = skill_home("skill-load")?;
        let mut notes = vec![
            full_text(&home).await?,
            disclosure(&home).await?,
            failures(&home).await?,
        ];
        write_skill(context.home(), "alpha", SMALL)?;
        write_skill(context.home(), "big", BIG)?;
        write_skill(context.home(), "broken", BROKEN)?;
        let mock = MockServer::start().await?;
        notes.extend(
            serve_with(context, &mock, async {
                let mut served = invalid_path(context).await?;
                served.extend(parallel(context).await?);
                Ok(served)
            })
            .await?,
        );
        Ok(notes)
    })
}

/// 只认可用工具的判定器：本章用例的 Skill 不声明工具，判定结果不影响返回文本。
fn availability() -> std::sync::Arc<Available> {
    std::sync::Arc::new(Available::new(&[]))
}

/// 一、小正文：一次加载返回全文 + 引用清单，正文字符数与 `SKILL.md` 逐字符相等。
async fn full_text(home: &std::path::Path) -> Result<String> {
    let mut entry = ready_entry("skills/alpha", SMALL)?;
    entry.references = vec![REFERENCE.to_owned()];
    let tool = SkillLoad::new(registry_of(SkillLimits::default(), vec![entry]), availability());
    ensure!(
        tool.file_access() == ToolFileAccess::Read && tool.risk() == ToolRisk::ReadOnly,
        "`skill_load` 应是只读工具（file_access = Read、risk = ReadOnly）"
    );

    let (text, error) = call_load(&tool, home, json!({"name": "alpha"})).await;
    ensure!(!error, "小正文应一次加载成功：{text}");
    let expected = format!("# 技能：alpha\n\n{SMALL_BODY}\n\n## 参考资料（用 file_read 按路径读取）\n- {REFERENCE}");
    ensure!(
        text == expected,
        "加载结果与固定包装不符：\n实际：{text}\n期望：{expected}"
    );
    ensure!(
        text.matches(SMALL_BODY).count() == 1 && text.contains(SMALL_BODY),
        "正文应原样出现一次（字符数与 SKILL.md 一致）：{text}"
    );
    Ok(format!(
        "小正文：全文 {} 字符（正文 {} 字符 + 固定包装），引用清单 {REFERENCE}",
        text.chars().count(),
        SMALL_BODY.chars().count()
    ))
}

/// 二、大正文：无 `section` 返回章节目录，带 `section` 返回该节；字符数与章节表一致。
async fn disclosure(home: &std::path::Path) -> Result<String> {
    let limits = SkillLimits {
        max_inline_chars: 120,
        ..Default::default()
    };
    let entry = ready_entry("skills/big", BIG)?;
    let sections = entry.sections.clone();
    let tool = SkillLoad::new(registry_of(limits, vec![entry]), availability());

    let (text, error) = call_load(&tool, home, json!({"name": "big"})).await;
    ensure!(!error, "大正文应返回章节目录而不是失败：{text}");
    let items: Vec<String> = sections
        .iter()
        .map(|section| format!("- {}（{} 字）", section.title, section.chars))
        .collect();
    let expected = format!("## 章节目录\n\n{}\n\n请用 section 参数按节读取。", items.join("\n"));
    ensure!(
        text.ends_with(&expected),
        "章节目录应与解析出的章节表一致：\n实际：{text}\n期望：{expected}"
    );
    ensure!(
        text.chars().count() <= limits.max_inline_chars,
        "章节目录也不得超过 max_inline_chars：{}",
        text.chars().count()
    );

    // 按标题取单节：允许省略 `#` 前缀。
    for title in ["步骤", "## 步骤"] {
        let (text, error) = call_load(&tool, home, json!({"name": "big", "section": title})).await;
        ensure!(!error, "取单节应成功（{title}）：{text}");
        ensure!(
            text.starts_with("# 技能：big\n\n## 步骤\n\n第一步先读模板"),
            "单节返回的应是该标题下的正文（{title}）：{text}"
        );
        ensure!(!text.contains("细则"), "单节不应带出后面的同级章节（{title}）：{text}");
        ensure!(
            text.chars().count() <= limits.max_inline_chars,
            "单节也不得超过 max_inline_chars：{}",
            text.chars().count()
        );
    }
    Ok(format!(
        "大正文：无 section 时返回章节目录（{} 节）；有 section 时只返回该节",
        sections.len()
    ))
}

/// 三、四种失败文案：未知技能 / 禁用 / `INVALID` / 未知章节（列出可用章节，最多 10 个）。
async fn failures(home: &std::path::Path) -> Result<String> {
    let mut expanded = String::from("---\nname: many\ndescription: 多节技能\n---\n\n");
    for index in 0..12 {
        expanded.push_str(&format!("# 小节 {index}\n\n内容 {index}。\n\n"));
    }
    let registry = registry_of(
        SkillLimits::default(),
        vec![
            ready_entry("skills/many", &expanded)?,
            invalid_entry("skills/broken", BROKEN)?,
        ],
    );
    let tool = SkillLoad::new(registry.clone(), availability());

    let (unknown, error) = call_load(&tool, home, json!({"name": "ghost"})).await;
    ensure!(
        error && unknown == "工具执行失败：技能不存在：ghost",
        "未知技能的文案不符：{unknown}"
    );

    registry
        .set_enabled("many", false)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let (disabled, error) = call_load(&tool, home, json!({"name": "many"})).await;
    ensure!(
        error && disabled == "工具执行失败：技能已禁用：many",
        "禁用技能的文案不符：{disabled}"
    );
    registry
        .set_enabled("many", true)
        .map_err(|error| anyhow::anyhow!("{error}"))?;

    let (invalid, error) = call_load(&tool, home, json!({"name": "broken"})).await;
    ensure!(
        error
            && invalid.starts_with("工具执行失败：技能不可用：broken：skills/broken/SKILL.md:")
            && invalid.contains("缺少 name"),
        "INVALID 的文案应带着解析期的可定位原因：{invalid}"
    );

    let (missing, error) = call_load(&tool, home, json!({"name": "many", "section": "nope"})).await;
    let listed: Vec<String> = (0..10).map(|index| format!("小节 {index}")).collect();
    ensure!(
        error
            && missing
                == format!(
                    "工具执行失败：技能没有这个章节：nope（可用章节：{}）",
                    listed.join("、")
                ),
        "未知章节的文案应列出前 10 个可用章节（正文顺序）：{missing}"
    );
    ensure!(
        !missing.contains("小节 10"),
        "可用章节最多列 10 个，第 11 个起不应出现：{missing}"
    );
    Ok(format!(
        "失败文案：未知技能 / 禁用 / INVALID / 未知章节（列出 {} 个可用章节）",
        listed.len()
    ))
}

/// 四、请求级：`INVALID` 的 `skill_load` 走一次真实工具执行，错误原样回灌，Run 不 FAILED。
async fn invalid_path(context: &TestContext) -> Result<Vec<String>> {
    let mut occupied = Occupied::start(context, "skill.load").await?;
    let script = json!({"calls": [{"name": "skill_load", "arguments": {"name": "broken"}}]});
    let (sent, drained) = occupied
        .say(
            context,
            &format!(
                "读一下坏技能（{}）
tool:{script}",
                context.token()
            ),
        )
        .await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCompleted,
        "工具失败不应让 Run FAILED：{:?}（{}）",
        drained.terminal.event,
        drained.terminal.error.clone().unwrap_or_default()
    );
    let records = audit(context, &sent.run.id).await?;
    ensure!(records.len() == 1, "一次调用应有一条审计：{records:?}");
    let record = &records[0];
    ensure!(
        record.tool_name == "skill_load" && record.status == ToolCallStatus::Error,
        "审计应记录一次失败的 skill_load：{record:?}"
    );
    let text = record.result.clone().unwrap_or_default();
    ensure!(
        text.contains("技能不可用：broken：") && text.contains("缺少 name"),
        "错误文案应原样回灌给模型：{text}"
    );
    occupied.release(context).await?;
    Ok(vec![format!("请求级：INVALID 的 skill_load 回灌「{text}」且 Run 完成")])
}

/// 五、并行：同一轮里的两个 `skill_load` 都是只读工具，开始事件早于任何结束事件（未被串行）。
async fn parallel(context: &TestContext) -> Result<Vec<String>> {
    let mut occupied = Occupied::start(context, "skill.load").await?;
    let script = json!({"calls": [
        {"name": "skill_load", "arguments": {"name": "alpha"}},
        {"name": "skill_load", "arguments": {"name": "big"}},
    ]});
    let (sent, drained) = occupied
        .say(
            context,
            &format!(
                "同时读两份技能（{}）
tool:{script}",
                context.token()
            ),
        )
        .await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCompleted,
        "并行读取应完成：{:?}",
        drained.terminal.event
    );
    let started = events_of(&drained, RunEventKind::ToolCallStarted);
    let completed = events_of(&drained, RunEventKind::ToolCallCompleted);
    ensure!(
        started.len() == 2 && completed.len() == 2,
        "两个调用都应各有一条开始与结束事件：{} / {}",
        started.len(),
        completed.len()
    );
    let last_start = drained
        .events
        .iter()
        .rposition(|event| event.event == RunEventKind::ToolCallStarted)
        .context("应有开始事件")?;
    let first_done = drained
        .events
        .iter()
        .position(|event| event.event == RunEventKind::ToolCallCompleted)
        .context("应有结束事件")?;
    ensure!(
        last_start < first_done,
        "两个只读调用应并行：开始事件（最后一个在 {}）应早于任何结束事件（第一个在 {}）",
        last_start,
        first_done
    );
    let records = audit(context, &sent.run.id).await?;
    ensure!(
        records.len() == 2 && records.iter().all(|record| record.status == ToolCallStatus::Ok),
        "两次读取都应成功：{records:?}"
    );
    occupied.release(context).await?;
    Ok(vec![format!(
        "并行：两个 skill_load 的开始事件都早于第一个结束事件（事件序号 {} / {}）",
        last_start, first_done
    )])
}
