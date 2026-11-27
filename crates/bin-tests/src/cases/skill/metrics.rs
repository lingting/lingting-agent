//! `skill.metrics`（P5-011 的取证）：`metrics` 快照的 `skills` 段八项。
//!
//! 口径：状态三项（`discovered` / `invalid` / `enabled`）取自注册表现状，其余五项是自进程启动的
//! 累计量。数字全部来自服务端的 `GET api/probe/metrics`（`metrics` 特性构建），用例不读日志；
//! 未开特性时按「未采集：原因」显式报告（AGENTS.md 的测试数据采集条款）。

use anyhow::{Result, ensure};
use lib_bin_cli::sdk::{RunEventKind, SkillAction};

use super::{case, get, serve_with, update, write_skill};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::flow::Occupied;
use crate::util::server;

/// `alpha`：两份正文之一，只有一节（`sections_served` 因此可精确断言）。
const ALPHA: &str = "---\nname: alpha\ndescription: 第一份技能\n---\n\n# 步骤\n\n1. 先读模板。\n";
/// `beta`：第二份可用技能（不加载，只参与索引与计数）。
const BETA: &str = "---\nname: beta\ndescription: 第二份技能\n---\n\n# 步骤\n\n1. 只做一件事。\n";
/// `broken`：缺 `name`，判 `INVALID`（用于 `invalid = 1` 与一次失败加载）。
const BROKEN: &str = "---\ndescription: 坏技能\n---\n\n# 步骤\n\n1. 什么也不做。\n";

/// 索引小节的期望文本：由用例按 D-P5-003 的固定格式独立拼出（不调用渲染代码），
/// 因此 `index_chars` 的断言是「复算」而不是「同一个数字读两遍」。
const EXPECTED_INDEX: &str = "## 可用 Skill\n\n\
                              以下技能提供任务步骤与约束；需要正文时调用 skill_load 获取。\n\n\
                              - alpha：第一份技能\n\
                              - beta：第二份技能";

/// 索引与加载指标用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.metrics", CaseKind::SelfServed, metrics)]
}

/// 八项数值：状态三项、加载三项、索引长度与重载次数。
fn metrics(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let home = context.home().to_path_buf();
        write_skill(&home, "alpha", ALPHA)?;
        write_skill(&home, "beta", BETA)?;
        write_skill(&home, "broken", BROKEN)?;

        let notes = serve_with(context, &mock, async { scope(context).await }).await?;
        Ok(notes)
    })
}

/// 服务端存活期间：跑一轮 Run（触发一次索引渲染）、一次成功加载、一次失败加载、一次 `RELOAD`，
/// 然后取快照。
async fn scope(context: &TestContext) -> Result<Vec<String>> {
    // 一、一轮 Run：ContextBuilder 现场渲染索引，`index_chars` 因此有值。
    let mut occupied = Occupied::start(context, "skill.metrics").await?;
    let (_, drained) = occupied
        .say(context, &format!("看一次技能索引 {}", context.token()))
        .await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCompleted,
        "指标用例的 Run 应完成：{:?}",
        drained.terminal.event
    );

    // 二、一次成功加载（管理面详情同样走注册表加载）与一次失败加载（`INVALID`）。
    let loaded = get(context, "alpha").await?;
    ensure!(!loaded.body.trim().is_empty(), "成功加载应有正文：{loaded:?}");
    let failed = get(context, "broken").await?;
    ensure!(
        failed.body.is_empty() && failed.error.is_some(),
        "失败加载应带原因、正文为空：{failed:?}"
    );

    // 三、一次 `RELOAD`：`reloads` 在启动那一次之上再 +1。
    update(context, None, SkillAction::Reload).await?;

    // 四、快照必须在服务端仍存活时取。
    let outcome = match server::capture(context.base_url()).await {
        Ok(snapshot) => {
            let skills = snapshot.skills;
            ensure!(
                skills.discovered == 3 && skills.invalid == 1 && skills.enabled == 2,
                "状态三项不符（3 个 Skill、1 个坏、2 个可用）：{skills:?}"
            );
            ensure!(
                skills.loads == 1 && skills.load_failures == 1,
                "一次成功 + 一次失败加载后应为 loads=1、load_failures=1：{skills:?}"
            );
            ensure!(
                skills.sections_served == 1,
                "成功加载交出的章节数应等于 alpha 的章节数（1）：{skills:?}"
            );
            let expected_chars = EXPECTED_INDEX.chars().count() as u64;
            ensure!(
                skills.index_chars == expected_chars,
                "index_chars 应等于独立复算的索引字符数（{expected_chars}）：{skills:?}"
            );
            ensure!(skills.reloads == 2, "reloads 应为启动 1 次 + RELOAD 1 次：{skills:?}");
            vec![format!(
                "metrics.skills：discovered={} invalid={} enabled={} loads={} load_failures={} sections_served={} index_chars={} reloads={}",
                skills.discovered,
                skills.invalid,
                skills.enabled,
                skills.loads,
                skills.load_failures,
                skills.sections_served,
                skills.index_chars,
                skills.reloads
            )]
        }
        // 未启用 `metrics` 特性时接口不存在：显式记「未采集」，不算通过也不算失败。
        Err(reason) => vec![format!("未采集：{reason}")],
    };
    occupied.release(context).await?;
    Ok(outcome)
}
