//! `skill.api`（P5-012 的取证）：三条管理接口的字段、排序、错误文案与动作语义。
//!
//! 两段：
//!
//! - 第一段：`alpha`（声明一个可用工具 + 一份引用材料）、`beta`（声明一个不存在的工具）、`broken`
//!   （缺 `name`，`INVALID`），核对清单 / 详情 / 启停 / 重载与四种固定 400 文案；
//! - 第二段：同一应用目录写入 `skills.toml` 的 `enabled = false` 后重启，核对空清单口径。

use anyhow::{Result, ensure};

use super::{
    case, find, get, get_rejected, list, names, serve_plain, update, update_rejected, write_file, write_reference,
    write_skill,
};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;
use lib_bin_cli::sdk::{SkillAction, SkillStatusKind};

/// `alpha`：声明一个可用工具（`calculator` 默认 `allow`），带一份引用材料。
const ALPHA: &str = "---\nname: alpha\ndescription: 第一份技能\nrequires_tools: [calculator]\n---\n\n\
                     # 步骤\n\n1. 先读模板。\n2. 再查数据。\n";
/// `alpha` 的正文（frontmatter 之后的原样文本，`trim` 后）。
const ALPHA_BODY: &str = "# 步骤\n\n1. 先读模板。\n2. 再查数据。";
/// `beta`：声明一个此刻不存在的工具，用于缺工具标注。
const BETA: &str = "---\nname: beta\ndescription: 第二份技能\nrequires_tools: [missing_tool]\n---\n\n\
                    # 步骤\n\n1. 只做一件事。\n";
/// `broken`：frontmatter 缺 `name`，判 `INVALID`，名字回落到目录名。
const BROKEN: &str = "---\ndescription: 坏技能\n---\n\n# 步骤\n\n1. 什么也不做。\n";
/// `gamma`：`RELOAD` 之前不存在，重扫之后出现。
const GAMMA: &str = "---\nname: gamma\ndescription: 第三份技能\n---\n\n# 步骤\n\n1. 后加的技能。\n";
/// 引用材料的相对路径（相对应用根目录）。
const REFERENCE: &str = "skills/alpha/references/notes.md";

/// 管理接口用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.api", CaseKind::SelfServed, api)]
}

/// 三条接口的字段、排序、错误文案与动作语义。
fn api(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let home = context.home().to_path_buf();
        write_skill(&home, "alpha", ALPHA)?;
        write_reference(&home, "alpha", "notes.md", "参考资料：先读模板再查数据。")?;
        write_skill(&home, "beta", BETA)?;
        write_skill(&home, "broken", BROKEN)?;

        let enabled = serve_plain(context, async { enabled_scope(context, &home).await }).await?;
        // 第二阶段：`skills.toml` 关掉 Skill 功能后重启，管理面只剩空清单。
        write_file(&home.join("skills.toml"), "enabled = false\n")?;
        let disabled = serve_plain(context, async { disabled_scope(context).await }).await?;
        Ok([enabled, disabled].concat())
    })
}

/// 第一段：默认配置下的清单、详情、动作与错误文案。
async fn enabled_scope(context: &TestContext, home: &std::path::Path) -> Result<Vec<String>> {
    let mut notes = Vec::new();

    // 一、清单：含 `INVALID`、按名字升序，字段与 `docs/api/skill.md` 逐项一致。
    let entries = list(context).await?;
    ensure!(
        names(&entries) == ["alpha", "beta", "broken"],
        "清单应按 name 升序：{:?}",
        names(&entries)
    );
    let alpha = find(&entries, "alpha")?;
    ensure!(
        alpha.status == SkillStatusKind::Ready
            && alpha.enabled
            && alpha.description == "第一份技能"
            && alpha.requires_tools == ["calculator"]
            && alpha.missing_tools.is_empty()
            && alpha.directory == "skills/alpha"
            && alpha.reference_count == 1
            && alpha.error.is_none(),
        "alpha 的字段不符：{alpha:?}"
    );
    let beta = find(&entries, "beta")?;
    ensure!(
        beta.status == SkillStatusKind::Ready
            && beta.enabled
            && beta.requires_tools == ["missing_tool"]
            && beta.missing_tools == ["missing_tool"],
        "beta 应标注缺失的工具：{beta:?}"
    );
    let broken = find(&entries, "broken")?;
    let error = broken.error.clone().unwrap_or_default();
    ensure!(
        broken.status == SkillStatusKind::Invalid && broken.enabled && broken.description.is_empty(),
        "INVALID 条目的状态与字段不符：{broken:?}"
    );
    ensure!(
        error.starts_with("skills/broken/SKILL.md:") && error.contains("缺少 name"),
        "INVALID 的原因应可定位：{error}"
    );
    let absolute = home.to_string_lossy().to_string();
    ensure!(!error.contains(&absolute), "错误文案不得含绝对路径：{error}");
    notes.push(format!("list：{:?}（含 INVALID，按 name 升序）", names(&entries)));

    // 二、详情：清单字段 + 正文 + 引用清单，取自同一次读取。
    let detail = get(context, "alpha").await?;
    ensure!(
        detail.body.trim() == ALPHA_BODY,
        "详情正文应与 SKILL.md 逐字符相等：{:?}",
        detail.body
    );
    ensure!(
        detail.references == [REFERENCE],
        "引用清单应为相对应用根目录的路径：{:?}",
        detail.references
    );
    ensure!(
        detail.reference_count == 1 && detail.status == SkillStatusKind::Ready && detail.enabled,
        "详情的清单字段应与 list 一致：{detail:?}"
    );
    let invalid = get(context, "broken").await?;
    ensure!(
        invalid.status == SkillStatusKind::Invalid && invalid.body.is_empty() && invalid.error.is_some(),
        "INVALID 的详情应返回元数据与原因、正文留空：{invalid:?}"
    );
    notes.push("get：正文逐字符相等、引用清单为相对路径、INVALID 带原因且正文为空".to_owned());

    // 三、固定 400 文案（参数级校验先于查找）。
    ensure!(
        get_rejected(context, "nope").await? == "技能不存在",
        "未知 name 的文案不符"
    );
    ensure!(
        update_rejected(context, Some("alpha"), SkillAction::Reload).await? == "RELOAD 不接受 name",
        "RELOAD 带 name 的文案不符"
    );
    ensure!(
        update_rejected(context, None, SkillAction::Enable).await? == "技能更新缺少 name",
        "ENABLE 缺 name 的文案不符"
    );
    ensure!(
        update_rejected(context, None, SkillAction::Disable).await? == "技能更新缺少 name",
        "DISABLE 缺 name 的文案不符"
    );
    notes.push("400：技能不存在 / RELOAD 不接受 name / 技能更新缺少 name".to_owned());

    // 四、启停：返回动作完成后的整张清单，与紧随其后的 list 逐字段相等，且幂等。
    let disabled_alpha = update(context, Some("alpha"), SkillAction::Disable).await?;
    ensure!(
        !find(&disabled_alpha, "alpha")?.enabled && find(&disabled_alpha, "alpha")?.status == SkillStatusKind::Ready,
        "DISABLE 之后应只剩启用标记变化：{:?}",
        find(&disabled_alpha, "alpha")?
    );
    ensure!(
        disabled_alpha == list(context).await?,
        "DISABLE 的返回值应与紧随其后的 list 逐字段相等"
    );
    ensure!(
        update(context, Some("alpha"), SkillAction::Disable).await? == disabled_alpha,
        "连续两次 DISABLE 的清单应逐字段相等"
    );
    let enabled_alpha = update(context, Some("alpha"), SkillAction::Enable).await?;
    ensure!(
        find(&enabled_alpha, "alpha")?.enabled && enabled_alpha == list(context).await?,
        "ENABLE 之后应恢复启用且等于 list"
    );
    ensure!(
        update(context, Some("alpha"), SkillAction::Enable).await? == enabled_alpha,
        "连续两次 ENABLE 的清单应逐字段相等"
    );
    notes.push("动作：ENABLE / DISABLE 幂等，返回值即动作完成后的整张清单".to_owned());

    // 五、RELOAD：新增的 Skill 出现、`disabled` 名单保留、两次结果逐字段相等。
    update(context, Some("beta"), SkillAction::Disable).await?;
    write_skill(home, "gamma", GAMMA)?;
    let reloaded = update(context, None, SkillAction::Reload).await?;
    ensure!(
        names(&reloaded) == ["alpha", "beta", "broken", "gamma"],
        "RELOAD 之后应出现新增的 Skill：{:?}",
        names(&reloaded)
    );
    ensure!(
        !find(&reloaded, "beta")?.enabled,
        "RELOAD 之后 disabled 名单应保留：{:?}",
        find(&reloaded, "beta")?
    );
    let reloaded_again = update(context, None, SkillAction::Reload).await?;
    ensure!(reloaded == reloaded_again, "两次 RELOAD 的清单应逐字段相等");
    ensure!(
        list(context).await? == reloaded_again,
        "RELOAD 的返回值应与紧随其后的 list 逐字段相等"
    );
    notes.push("RELOAD：新增生效、disabled 保留、两次幂等".to_owned());
    Ok(notes)
}

/// 第二段：`skills.toml` 的 `enabled = false` 时管理面只剩空清单。
async fn disabled_scope(context: &TestContext) -> Result<Vec<String>> {
    ensure!(list(context).await?.is_empty(), "enabled = false 时清单应为空");
    ensure!(
        get_rejected(context, "alpha").await? == "技能不存在",
        "enabled = false 时 get 应一律「技能不存在」"
    );
    ensure!(
        update_rejected(context, Some("alpha"), SkillAction::Enable).await? == "技能不存在",
        "enabled = false 时 ENABLE 应一律「技能不存在」"
    );
    ensure!(
        update(context, None, SkillAction::Reload).await?.is_empty(),
        "enabled = false 时 RELOAD 应是空操作（空清单）"
    );
    Ok(vec![
        "enabled = false：清单为空、get 与启停一律 400、RELOAD 空操作".to_owned(),
    ])
}
