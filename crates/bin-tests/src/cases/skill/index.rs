//! `skill.index`（P5-007 / P5-005 / P5-017）：索引进系统提示的形态与预算。
//!
//! 证据两段：
//!
//! - 请求级：起一次私有服务端 + 私有 Mock，跑完一轮 Run 后读 `/mock/last-request`，核对 system 消息
//!   的条数、内容（`AGENTS.md` 在前、恰一个空行、索引小节在后）与 `tools` 清单；
//! - 进程内：直接渲染 `SkillRegistry::index`，核对禁用项不出现在索引、缺工具标注、超
//!   `max_index_chars` 的整行截断与「另有 N 个」。
//!
//! 不读日志。

use agent_skill::{SkillIndex, SkillLimits};
use anyhow::{Context, Result, ensure};
use lib_bin_cli::sdk::{RunEventKind, SkillAction};
use serde_json::Value;

use super::{
    Available, case, list, ready_entry, registry_of, serve_with, system_messages, tool_names, update, write_file,
    write_skill,
};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::mock::llm::{MockServer, observe};
use crate::util::context::TestContext;
use crate::util::flow::Occupied;

/// `AGENTS.md` 的内容（合成时应被 `trim` 后放在索引之前）。
const AGENTS: &str = "项目约定：只读，不改仓库里的文档。";
/// `alpha` 正文里的独有句子（P-2 的「不含正文」口径：≥ 16 字符、不与描述相同）。
const ALPHA_BODY_MARK: &str = "正文独有：本技能先读模板再查数据";
/// `alpha`：声明一个此刻可用的内置工具（`calculator` 默认允许）。
const ALPHA: &str = "---\nname: alpha\ndescription: 第一份技能\nrequires_tools: [calculator]\n---\n\n\
                     # 步骤\n\n正文独有：本技能先读模板再查数据。\n";
/// `beta`：声明一个此刻不存在的工具，索引行要带缺工具标注。
const BETA: &str = "---\nname: beta\ndescription: 第二份技能\nrequires_tools: [missing_tool]\n---\n\n\
                    # 步骤\n\n1. 只做一件事。\n";
/// `broken`：缺 `name`，判 `INVALID`，永不进索引。
const BROKEN: &str = "---\ndescription: 坏技能\n---\n\n# 步骤\n\n1. 什么也不做。\n";
/// 索引小节的固定前缀（D-P5-003 的格式由用例独立拼出，不调用渲染代码）。
const INDEX_FORMAT: &str = "## 可用 Skill\n\n以下技能提供任务步骤与约束；需要正文时调用 skill_load 获取。";

/// 索引用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.index", CaseKind::SelfServed, index)]
}

/// 请求级 + 进程内两段。
fn index(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let home = context.home().to_path_buf();
        write_file(&home.join("AGENTS.md"), AGENTS)?;
        write_skill(&home, "alpha", ALPHA)?;
        write_skill(&home, "beta", BETA)?;
        write_skill(&home, "broken", BROKEN)?;

        let mock = MockServer::start().await?;
        let mut all = serve_with(context, &mock, async { request_scope(context, &mock).await }).await?;

        // 只配 Skill（无 AGENTS.md）：system 消息只有索引小节，且仍只有一条。
        std::fs::remove_file(home.join("AGENTS.md")).with_context(|| "删除 AGENTS.md 失败")?;
        all.extend(serve_with(context, &mock, async { skills_only_scope(context, &mock).await }).await?);

        // `skills.toml` 的 `enabled = false`：请求体与 P2 一致（system 只有 AGENTS.md，无 skill_load）。
        write_file(&home.join("AGENTS.md"), AGENTS)?;
        write_file(
            &home.join("skills.toml"),
            "enabled = false
",
        )?;
        all.extend(serve_with(context, &mock, async { disabled_scope(context, &mock).await }).await?);

        all.extend(render_scope()?);
        Ok(all)
    })
}

/// 一、请求级：system 消息的条数与逐字符内容、`tools` 清单、禁用项的即时生效。
async fn request_scope(context: &TestContext, mock: &MockServer) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    let mut occupied = Occupied::start(context, "skill.index").await?;

    // 第一轮：`AGENTS.md` 与索引合成同一条 system 消息。
    let expected =
        format!("{AGENTS}\n\n{INDEX_FORMAT}\n\n- alpha：第一份技能\n- beta：第二份技能（缺少工具：missing_tool）");
    let recorded = run_once(context, mock, &mut occupied, "列一下可用技能").await?;
    let systems = system_messages(&recorded);
    ensure!(
        systems.len() == 1,
        "两者都有时只能有一条 system 消息：{}",
        systems.len()
    );
    ensure!(
        systems[0] == expected,
        "system 消息应是「AGENTS.md + 空行 + 索引小节」：\n实际：{}\n期望：{expected}",
        systems[0]
    );
    let tools = tool_names(&recorded);
    ensure!(
        tools.contains(&"skill_load".to_owned()) && tools.contains(&"calculator".to_owned()),
        "`skill_load` 与可用工具都应出现在请求的 tools 里：{tools:?}"
    );
    ensure!(
        !tools.contains(&"missing_tool".to_owned()),
        "缺失的工具不应出现在 tools 里：{tools:?}"
    );
    let request_text = recorded.to_string();
    ensure!(!request_text.contains("broken"), "INVALID 项不应进请求体");
    // P-2：未调用 `skill_load` 时，请求体里不含正文（正文只在工具结果里进上下文）。
    ensure!(
        !request_text.contains(ALPHA_BODY_MARK),
        "未加载时请求体不应含 Skill 正文：{ALPHA_BODY_MARK}"
    );
    notes.push(format!(
        "请求体：一条 system 消息，共 {} 字符；tools={tools:?}",
        systems[0].chars().count()
    ));

    // 第二轮：禁用 `beta` 后索引里立刻没有它（同一次 Run 的现场渲染）。
    update(context, Some("beta"), SkillAction::Disable).await?;
    let expected = format!("{AGENTS}\n\n{INDEX_FORMAT}\n\n- alpha：第一份技能");
    let recorded = run_once(context, mock, &mut occupied, "再看一次技能").await?;
    let systems = system_messages(&recorded);
    ensure!(
        systems.len() == 1 && systems[0] == expected,
        "禁用后索引应立即不含该项：{systems:?}"
    );
    notes.push("禁用：索引立刻不含 beta，system 消息仍只有一条".to_owned());

    // 第三轮：全部禁用后 system 消息只剩 `AGENTS.md`（与 P2 逐字节一致，不产生空小节）。
    update(context, Some("alpha"), SkillAction::Disable).await?;
    let recorded = run_once(context, mock, &mut occupied, "没有技能可用时").await?;
    let systems = system_messages(&recorded);
    ensure!(
        systems.len() == 1 && systems[0] == AGENTS,
        "没有可用 Skill 时 system 消息应只剩 AGENTS.md：{systems:?}"
    );
    notes.push("全部禁用：system 消息与 P2 逐字节一致（只剩 AGENTS.md）".to_owned());
    occupied.release(context).await?;
    Ok(notes)
}

/// 二、只配 Skill（无 `AGENTS.md`）：system 消息以索引小节开头，且仍只有一条。
async fn skills_only_scope(context: &TestContext, mock: &MockServer) -> Result<Vec<String>> {
    let mut occupied = Occupied::start(context, "skill.index").await?;
    let recorded = run_once(context, mock, &mut occupied, "只看技能索引").await?;
    let expected = format!(
        "{INDEX_FORMAT}

- alpha：第一份技能
- beta：第二份技能（缺少工具：missing_tool）"
    );
    let systems = system_messages(&recorded);
    ensure!(
        systems.len() == 1 && systems[0] == expected,
        "没有 AGENTS.md 时 system 消息应只有索引小节：{systems:?}"
    );
    occupied.release(context).await?;
    Ok(vec![format!(
        "无 AGENTS.md：system 消息以 `## 可用 Skill` 开头（{} 字符）",
        systems[0].chars().count()
    )])
}

/// 三、`skills.toml` 的 `enabled = false`：请求体与 P2 一致，`skill_load` 不在工具清单里。
async fn disabled_scope(context: &TestContext, mock: &MockServer) -> Result<Vec<String>> {
    let mut occupied = Occupied::start(context, "skill.index").await?;
    let recorded = run_once(context, mock, &mut occupied, "技能功能关闭时").await?;
    let systems = system_messages(&recorded);
    ensure!(
        systems.len() == 1 && systems[0] == AGENTS,
        "enabled = false 时 system 消息应只有 AGENTS.md：{systems:?}"
    );
    let tools = tool_names(&recorded);
    ensure!(
        !tools.contains(&"skill_load".to_owned()),
        "enabled = false 时不应注册 skill_load：{tools:?}"
    );
    ensure!(list(context).await?.is_empty(), "enabled = false 时清单应为空");
    occupied.release(context).await?;
    Ok(vec![format!("enabled = false：system 只有 AGENTS.md，tools={tools:?}")])
}

/// 跑一轮 Run 并返回最近一次模型请求体。
async fn run_once(context: &TestContext, mock: &MockServer, occupied: &mut Occupied, text: &str) -> Result<Value> {
    let (_, drained) = occupied.say(context, &format!("{text}（{}）", context.token())).await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCompleted,
        "Run 应完成：{:?}",
        drained.terminal.event
    );
    observe::last_request(mock).await
}

/// 二、进程内：禁用项、缺工具标注、整行截断与「另有 N 个」。
fn render_scope() -> Result<Vec<String>> {
    let mut notes = Vec::new();
    let available = Available::new(&["calculator"]);
    let registry = registry_of(
        SkillLimits::default(),
        vec![ready_entry("skills/alpha", ALPHA)?, ready_entry("skills/beta", BETA)?],
    );

    // 索引行格式：与 D-P5-003 的样例逐字符一致（缺工具标注在行尾）。
    let before = registry.index(&available);
    let expected = format!("{INDEX_FORMAT}\n\n- alpha：第一份技能\n- beta：第二份技能（缺少工具：missing_tool）");
    ensure!(
        before.section == expected,
        "索引小节与 D-P5-003 的样例不符：\n实际：{}\n期望：{expected}",
        before.section
    );
    ensure!(
        before.chars == before.section.chars().count() && before.shown == 2 && before.hidden == 0,
        "`chars` 应由小节文本复算、`shown` 等于启用项数：{before:?}"
    );
    registry
        .set_enabled("beta", false)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let disabled = registry.index(&available);
    ensure!(
        !disabled.section.contains("beta") && disabled.shown == 1,
        "禁用后索引应立即不含该项：{}",
        disabled.section
    );
    registry
        .set_enabled("beta", true)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    ensure!(registry.index(&available) == before, "启用后索引应逐字符恢复到禁用前");
    notes.push("进程内：禁用 → 启用往返后索引逐字符一致".to_owned());

    // 空索引：没有可用 Skill 时不产生小节（因此不产生空 system 消息）。
    let empty = SkillIndex::default();
    ensure!(
        empty.section.is_empty() && empty.chars == 0 && empty.shown == 0,
        "空注册表应渲染出空小节：{empty:?}"
    );

    // 预算：超 `max_index_chars` 时整行截断，`shown + hidden` 等于启用项总数。
    let limits = SkillLimits {
        max_index_chars: 160,
        ..Default::default()
    };
    let mut entries = Vec::new();
    for index in 0..6 {
        let directory = format!("skills/bulk-{index}");
        let text = format!(
            "---\nname: bulk-{index}\ndescription: 第 {index} 份批量技能，描述稍长以便把索引撑出预算\n---\n\n# 步骤\n\n1. 批量。\n"
        );
        entries.push(ready_entry(&directory, &text)?);
    }
    let bulk = registry_of(limits, entries);
    let index = bulk.index(&Available::new(&[]));
    ensure!(
        index.chars <= limits.max_index_chars,
        "索引字符数应守住上限（{}）：{}",
        limits.max_index_chars,
        index.chars
    );
    ensure!(
        index.shown + index.hidden == 6,
        "`shown + hidden` 应等于启用项总数：{index:?}"
    );
    ensure!(index.hidden > 0, "预算 240 字符应触发截断：{index:?}");
    let summary = format!(
        "- （另有 {} 个技能因篇幅未展示，可用 skill_load 按名字加载）",
        index.hidden
    );
    ensure!(
        index.section.ends_with(&summary),
        "截断后应追加「另有 N 个」：{}",
        index.section
    );
    // 整行截断：小节里的每条索引行都是完整的一行（描述完整，不出现半行）。
    for line in index.section.lines().filter(|line| line.starts_with("- bulk-")) {
        ensure!(
            line.ends_with("预算") && line.contains("：第 "),
            "索引行应是完整的一行（不出现半行）：{line}"
        );
    }
    notes.push(format!(
        "预算 {} 字符：shown={} hidden={} chars={}",
        limits.max_index_chars, index.shown, index.hidden, index.chars
    ));
    Ok(notes)
}
