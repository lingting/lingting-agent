//! `skill.invalid`（P5-018）：坏 Skill 不可能被悄悄用上，但一定在管理面上带着原因出现。
//!
//! 证据：一条 `INVALID` 的 Skill，请求体与索引里都没有它，`skill_load` 不可执行（工具级失败，
//! Run 不 FAILED），而 `api/skill/list` 与 `api/skill/get` 都带着可定位原因。
//!
//! 数据来源是接口响应、客户端事件、Mock 的 `/mock/last-request` 与 `api/tool/audit/list`；不读日志。

use anyhow::{Result, ensure};
use lib_bin_cli::sdk::{RunEventKind, SkillStatusKind, ToolCallStatus};
use serde_json::json;

use super::{case, find, get, list, serve_with, system_messages, write_skill};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::cases::tool::audit;
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::flow::Occupied;

/// `alpha`：合法技能，用于证明索引本身工作正常。
const ALPHA: &str = "---\nname: alpha\ndescription: 第一份技能\n---\n\n# 步骤\n\n1. 先读模板。\n";
/// `broken`：缺 `name`，判 `INVALID`。
const BROKEN: &str = "---\ndescription: 坏技能\n---\n\n# 步骤\n\n1. 什么也不做。\n";

/// 坏 Skill 用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.invalid", CaseKind::SelfServed, invalid)]
}

/// 请求体 / 索引 / 工具执行 / 管理面四处取证。
fn invalid(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        write_skill(context.home(), "alpha", ALPHA)?;
        write_skill(context.home(), "broken", BROKEN)?;
        let mock = MockServer::start().await?;
        let notes = serve_with(context, &mock, async { scope(context, &mock).await }).await?;
        Ok(notes)
    })
}

/// 服务端存活期间跑一轮 Run 并核对四处。
async fn scope(context: &TestContext, mock: &MockServer) -> Result<Vec<String>> {
    let mut notes = Vec::new();

    // 一、管理面：`INVALID` 条目带着可定位原因出现，详情没有正文。
    let entries = list(context).await?;
    let broken = find(&entries, "broken")?;
    let error = broken.error.clone().unwrap_or_default();
    ensure!(
        broken.status == SkillStatusKind::Invalid && broken.enabled,
        "INVALID 条目的状态不对：{broken:?}"
    );
    ensure!(
        error.starts_with("skills/broken/SKILL.md:") && error.contains("缺少 name"),
        "INVALID 的原因应可定位：{error}"
    );
    let detail = get(context, "broken").await?;
    ensure!(
        detail.status == SkillStatusKind::Invalid && detail.body.is_empty() && detail.error.is_some(),
        "INVALID 的详情应只有元数据与原因：{detail:?}"
    );
    notes.push(format!("list：broken 为 INVALID，原因「{error}」"));

    // 二、请求体：索引里只有可用项，`INVALID` 的字样一次都不出现。
    let mut occupied = Occupied::start(context, "skill.invalid").await?;
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
        "坏 Skill 不应让 Run FAILED：{:?}",
        drained.terminal.event
    );
    let request = crate::mock::llm::observe::last_request(mock).await?;
    let system = system_messages(&request);
    ensure!(system.len() == 1, "应只有一条 system 消息：{system:?}");
    ensure!(
        system[0].contains("- alpha：第一份技能") && !system[0].contains("broken"),
        "索引只含可用项：{}",
        system[0]
    );
    notes.push("请求体：索引只有 alpha，没有 broken".to_owned());

    // 三、工具执行：`skill_load` 报「技能不可用」+ 解析期原因，不回灌正文。
    let records = audit(context, &sent.run.id).await?;
    ensure!(records.len() == 1, "一次调用应有一条审计：{records:?}");
    let text = records[0].result.clone().unwrap_or_default();
    ensure!(
        records[0].status == ToolCallStatus::Error
            && text.contains("技能不可用：broken：")
            && text.contains("缺少 name"),
        "`skill_load` 对 INVALID 的返回不符：{records:?}"
    );
    ensure!(!text.contains("什么也不做"), "INVALID 的正文不应被加载出来：{text}");
    notes.push(format!("skill_load：{text}"));
    occupied.release(context).await?;
    Ok(notes)
}
