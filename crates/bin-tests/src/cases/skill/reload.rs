//! `skill.reload`（P5-018）：新增 / 删除 / 改内容都靠 `RELOAD` 生效，且两次 `RELOAD` 幂等。
//!
//! 证据是 `api/skill/*` 的响应：不需要重启服务端，也不需要改动进程内状态。
//! 不读日志。

use anyhow::{Context, ensure};
use lib_bin_cli::sdk::SkillAction;

use super::{case, find, get, list, names, serve_plain, update, write_skill};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;
use crate::util::paths::skill_dir;

/// `alpha`：启动时就存在的技能。
const ALPHA: &str = "---\nname: alpha\ndescription: 第一份技能\n---\n\n# 步骤\n\n1. 先读模板。\n";
/// `gamma`：服务端起来之后才写进目录的技能。
const GAMMA: &str = "---\nname: gamma\ndescription: 第三份技能\n---\n\n# 步骤\n\n1. 后加的技能。\n";
/// `gamma` 改内容之后的样子。
const GAMMA_EDITED: &str = "---\nname: gamma\ndescription: 第三份技能（改过）\n---\n\n# 步骤\n\n1. 内容改了。\n";

/// 重载用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.reload", CaseKind::SelfServed, reload)]
}

/// 四条路径：新增、改内容、删除、幂等 + `disabled` 名单保留。
fn reload(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let home = context.home().to_path_buf();
        write_skill(&home, "alpha", ALPHA)?;

        serve_plain(context, async {
            let mut notes = Vec::new();

            // 一、新增：不重载看不到，重载后出现且能加载。
            ensure!(names(&list(context).await?) == ["alpha"], "启动时只应有 alpha");
            write_skill(&home, "gamma", GAMMA)?;
            ensure!(
                names(&list(context).await?) == ["alpha"],
                "没重载之前新增的 Skill 不应出现"
            );
            let reloaded = update(context, None, SkillAction::Reload).await?;
            ensure!(
                names(&reloaded) == ["alpha", "gamma"],
                "RELOAD 之后应出现新增的 Skill：{:?}",
                names(&reloaded)
            );
            ensure!(
                get(context, "gamma").await?.body.trim().ends_with("1. 后加的技能。"),
                "RELOAD 之后新增的 Skill 正文应可加载"
            );
            notes.push("新增：重载后出现且正文可加载".to_owned());

            // 二、改内容：重载后详情跟着变。
            write_skill(&home, "gamma", GAMMA_EDITED)?;
            ensure!(
                get(context, "gamma").await?.description == "第三份技能",
                "没重载之前内容不应变化"
            );
            update(context, None, SkillAction::Reload).await?;
            let detail = get(context, "gamma").await?;
            ensure!(
                detail.description == "第三份技能（改过）" && detail.body.trim().ends_with("1. 内容改了。"),
                "RELOAD 之后内容应生效：{detail:?}"
            );
            notes.push("改内容：重载后详情跟着变".to_owned());

            // 三、删除：目录移掉之后重载即消失。
            std::fs::remove_dir_all(skill_dir(&home, "alpha")).with_context(|| "删除 alpha 目录失败")?;
            ensure!(
                names(&list(context).await?).len() == 2,
                "没重载之前被删掉的 Skill 仍应在清单里"
            );
            let reloaded = update(context, None, SkillAction::Reload).await?;
            ensure!(
                names(&reloaded) == ["gamma"],
                "RELOAD 之后被删掉的 Skill 应消失：{:?}",
                names(&reloaded)
            );
            notes.push("删除：重载后从清单消失".to_owned());

            // 四、幂等 + `disabled` 名单保留：连续两次 RELOAD 的清单逐字段相等。
            update(context, Some("gamma"), SkillAction::Disable).await?;
            let first = update(context, None, SkillAction::Reload).await?;
            ensure!(
                !find(&first, "gamma")?.enabled,
                "RELOAD 之后 disabled 名单应保留：{:?}",
                find(&first, "gamma")?
            );
            let second = update(context, None, SkillAction::Reload).await?;
            ensure!(first == second, "两次 RELOAD 的清单应逐字段相等");
            ensure!(
                list(context).await? == second,
                "RELOAD 的返回值应与紧随其后的 list 逐字段相等"
            );
            notes.push("幂等：两次 RELOAD 逐字段相等，disabled 保留".to_owned());
            Ok(notes)
        })
        .await
    })
}
