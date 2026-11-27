//! `skill.budget`（P5-009 / P5-017）：索引与正文的预算口径。
//!
//! 证据两段：
//!
//! - 进程内：渲染索引 / 调用 `skill_load`，核对「索引 ≤ `max_index_chars`」「正文 ≤ `max_inline_chars`」
//!   「同一 Skill 在 1 个与 50 个场景下返回逐字符一致」；
//! - 请求级：两次私有服务端 + 私有 Mock，一次放小正文、一次放 10 倍大的正文，比较
//!   `run_started.input_tokens`（正文只进后续轮次，不改变首轮预算）。
//!
//! 不读日志。

use agent_skill::{SkillLimits, SkillLoad};
use anyhow::{Context, Result, ensure};
use lib_bin_cli::sdk::RunEventKind;
use serde_json::json;

use super::{Available, call_load, case, ready_entry, registry_of, serve_with, write_skill};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::flow::Occupied;
use crate::util::paths::skill_home;

/// 小正文（约 20 字）：索引与正文的预算都被它撑不破。
const SMALL: &str = "---\nname: alpha\ndescription: 预算技能\n---\n\n# 步骤\n\n先读模板。\n";
/// 10 倍大的正文：章节表不变，正文按同样口径重复。
const REPEAT: usize = 120;

/// 预算用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.budget", CaseKind::SelfServed, budget)]
}

/// 进程内三段 + 请求级一段。
fn budget(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let home = skill_home("skill-budget")?;
        let mut notes = vec![
            index_bounds(&home).await?,
            inline_bounds(&home).await?,
            scale(&home).await?,
        ];
        notes.extend(tokens(context).await?);
        Ok(notes)
    })
}

/// 一、索引：`chars ≤ max_index_chars`，预算小到装不下任何一行时返回空小节（不输出半行）。
async fn index_bounds(_home: &std::path::Path) -> Result<String> {
    let mut notes = Vec::new();
    for max in [4_000usize, 200, 120, 60, 10] {
        let limits = SkillLimits {
            max_index_chars: max,
            ..Default::default()
        };
        let mut entries = Vec::new();
        for index in 0..5 {
            let directory = format!("skills/bulk-{index}");
            let text = format!(
                "---\nname: bulk-{index}\ndescription: 第 {index} 份技能，描述固定长度以便复算预算\n---\n\n# 步骤\n\n1. 做事。\n"
            );
            entries.push(ready_entry(&directory, &text)?);
        }
        let registry = registry_of(limits, entries);
        let index = registry.index(&Available::new(&[]));
        ensure!(
            index.chars <= max && index.chars == index.section.chars().count(),
            "max_index_chars = {max} 时索引应守住上限：{index:?}"
        );
        if index.section.is_empty() {
            // 极端输入：连一行都装不下时返回空小节（宁可不展示，也不输出半行）。
            ensure!(
                index.chars == 0 && index.shown == 0 && index.hidden == 0,
                "max_index_chars = {max} 时应返回空小节：{index:?}"
            );
        } else {
            ensure!(
                index.shown + index.hidden == 5,
                "max_index_chars = {max} 时 shown + hidden 应等于启用项总数：{index:?}"
            );
            // 整行截断：出现的每条索引行都是完整的一行。
            for line in index.section.lines().filter(|line| line.starts_with("- bulk-")) {
                ensure!(line.ends_with("预算"), "max_index_chars = {max} 时不应出现半行：{line}");
            }
        }
        notes.push(format!(
            "索引上限 {max}：chars={} shown={} hidden={}",
            index.chars, index.shown, index.hidden
        ));
    }
    Ok(notes.join("｜"))
}

/// 二、正文：返回值恒 `≤ max_inline_chars`（含包装行），章节截断有明确标注。
async fn inline_bounds(home: &std::path::Path) -> Result<String> {
    let mut notes = Vec::new();
    let body = format!("## 步骤\n\n{}", "步骤说明。".repeat(30));
    let text = format!("---\nname: wide\ndescription: 长正文\n---\n\n{body}\n");
    for max in [6_000usize, 300, 120, 40] {
        let limits = SkillLimits {
            max_inline_chars: max,
            ..Default::default()
        };
        let registry = registry_of(limits, vec![ready_entry("skills/wide", &text)?]);
        let tool = SkillLoad::new(registry, std::sync::Arc::new(Available::new(&[])));
        let (loaded, error) = call_load(&tool, home, json!({"name": "wide"})).await;
        ensure!(!error, "max_inline_chars = {max} 时加载应成功：{loaded}");
        ensure!(
            loaded.chars().count() <= max,
            "max_inline_chars = {max} 时返回值超预算：{} 字符",
            loaded.chars().count()
        );
        // 指定章节同样受这一条约束，且截断有标注。
        let (section, error) = call_load(&tool, home, json!({"name": "wide", "section": "步骤"})).await;
        ensure!(!error, "max_inline_chars = {max} 时取单节应成功：{section}");
        ensure!(
            section.chars().count() <= max,
            "max_inline_chars = {max} 时单节超预算：{} 字符",
            section.chars().count()
        );
        if max == 40 {
            ensure!(
                loaded.is_empty() || !loaded.contains("步骤说明。"),
                "预算连说明都放不下时应什么都不给，而不是给出超预算的提示：{loaded}"
            );
        }
        notes.push(format!(
            "正文上限 {max}：全文 {} 字符、单节 {} 字符",
            loaded.chars().count(),
            section.chars().count()
        ));
    }
    Ok(notes.join("｜"))
}

/// 三、同一 Skill 在「只启用它」与「启用 50 个」两种场景下，`skill_load` 的返回逐字符一致。
async fn scale(home: &std::path::Path) -> Result<String> {
    let target = ready_entry("skills/alpha", SMALL)?;
    let alone = SkillLoad::new(
        registry_of(SkillLimits::default(), vec![target.clone()]),
        std::sync::Arc::new(Available::new(&[])),
    );
    let mut crowd = vec![target];
    for index in 0..49 {
        let directory = format!("skills/filler-{index:02}");
        let text = format!("---\nname: filler-{index:02}\ndescription: 陪衬技能 {index}\n---\n\n# 步骤\n\n1. 陪衬。\n");
        crowd.push(ready_entry(&directory, &text)?);
    }
    let crowded = SkillLoad::new(
        registry_of(SkillLimits::default(), crowd),
        std::sync::Arc::new(Available::new(&[])),
    );
    let (one, error) = call_load(&alone, home, json!({"name": "alpha"})).await;
    ensure!(!error, "单技能场景应加载成功：{one}");
    let (many, error) = call_load(&crowded, home, json!({"name": "alpha"})).await;
    ensure!(!error, "50 技能场景应加载成功：{many}");
    ensure!(one == many, "索引规模不应影响正文：\n单技能：{one}\n50 技能：{many}");
    Ok(format!(
        "规模：1 个与 50 个技能场景下返回逐字符一致（{} 字符）",
        one.chars().count()
    ))
}

/// 四、10 倍大正文不改变 `run_started.input_tokens`（正文只进后续轮次）。
async fn tokens(context: &TestContext) -> Result<Vec<String>> {
    let home = context.home().to_path_buf();
    let mock = MockServer::start().await?;

    write_skill(&home, "alpha", SMALL)?;
    let small = serve_with(context, &mock, async { started_tokens(context).await }).await?;
    write_skill(&home, "alpha", &wide()?)?;
    let wide = serve_with(context, &mock, async { started_tokens(context).await }).await?;

    ensure!(
        small == wide,
        "10 倍大正文不应改变首轮 input_tokens（索引未变）：{small} != {wide}"
    );
    Ok(vec![format!(
        "首轮 input_tokens：小正文 {small}，10 倍正文 {wide}（逐值相等）"
    )])
}

/// 跑一轮 Run，返回 `run_started` 事件的 `input_tokens`。
async fn started_tokens(context: &TestContext) -> Result<i64> {
    let mut occupied = Occupied::start(context, "skill.budget").await?;
    let (_, drained) = occupied
        .say(context, &format!("量一下预算（{}）", context.token()))
        .await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCompleted,
        "Run 应完成：{:?}",
        drained.terminal.event
    );
    let started = drained
        .events
        .iter()
        .find(|event| event.event == RunEventKind::RunStarted)
        .context("应有一条 run_started 事件")?;
    let tokens = started.input_tokens.context("run_started 应带 input_tokens")?;
    occupied.release(context).await?;
    Ok(tokens)
}

/// 10 倍大的 `SKILL.md`：名字与描述不变，只有正文字数放大。
fn wide() -> Result<String> {
    let body = "补充说明。".repeat(REPEAT);
    let text = format!("---\nname: alpha\ndescription: 预算技能\n---\n\n## 步骤\n\n{body}\n");
    ensure!(
        text.chars().count() > SMALL.chars().count() * 5,
        "10 倍正文应显著大于小正文：{} 字符",
        text.chars().count()
    );
    Ok(text)
}
