//! `skill.discover`（P5-004 / P5-016）：发现快照的确定性口径。
//!
//! 证据来自进程内 `agent_skill::discover` 返回的 [`SkillSnapshot`]（服务端启动期走同一条路径），
//! 不读日志。四段：
//!
//! - 目录缺失 / 空目录 / 只有无关子目录：空清单且无错误；
//! - 缺 `SKILL.md` 的目录不进清单；内容非法的进清单且 `INVALID` 带可定位原因；
//! - 同名冲突：两份都 `INVALID`，文案含两个目录；改名后两份都 `READY`；
//! - 两次扫描逐字段一致（顺序、字段、错误文案）。

use std::path::Path;

use agent_skill::{SkillLimits, SkillSnapshot, SkillStatus, discover};
use anyhow::{Context, Result, ensure};

use super::case;
use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;
use crate::util::paths::{skill_dir, skill_home, skill_root};
use crate::util::support::temp_directory;

/// 发现根目录（相对应用根目录）。
const ROOT: &str = "skills";

/// `alpha`：合法技能。
const ALPHA: &str = "---\nname: alpha\ndescription: 第一份技能\n---\n\n# 步骤\n\n1. 先读模板。\n";
/// `broken`：缺 `name`，判 `INVALID`，名字回落到目录名。
const BROKEN: &str = "---\ndescription: 坏技能\n---\n\n# 步骤\n\n1. 什么也不做。\n";
/// `same`：两个目录声明同一个名字，用于同名冲突。
const SAME: &str = "---\nname: same\ndescription: 同名技能\n---\n\n# 步骤\n\n1. 冲突。\n";
/// `same` 改名后的版本。
const SAME_RENAMED: &str = "---\nname: other\ndescription: 同名技能\n---\n\n# 步骤\n\n1. 不再冲突。\n";

/// 发现用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.discover", CaseKind::Shared, discovery)]
}

/// 四段断言。
fn discovery(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = vec![empty_cases()?, status_cases()?, duplicate_cases()?];

        // 四、同一目录两次扫描逐字段一致（顺序、字段、错误文案）。
        let home = skill_home("skill-discover-stable")?;
        write(&skill_dir(&home, "beta"), "SKILL.md", ALPHA)?;
        write(&skill_dir(&home, "gamma"), "SKILL.md", BROKEN)?;
        let first = scan(&home);
        let second = scan(&home);
        ensure!(first == second, "两次扫描应逐字段一致：{first:?} != {second:?}");
        ensure!(
            first
                .entries()
                .iter()
                .map(|entry| entry.directory.clone())
                .collect::<Vec<_>>()
                == ["skills/beta", "skills/gamma"],
            "快照应按 name 升序（同名的按目录名）：{:?}",
            first.entries()
        );
        notes.push(format!(
            "两次扫描一致：{:?}",
            first
                .entries()
                .iter()
                .map(|entry| (entry.name.as_str(), entry.directory.as_str()))
                .collect::<Vec<_>>()
        ));
        Ok(notes)
    })
}

/// 一、目录缺失 / 空目录 / 只有无关子目录 / 缺 `SKILL.md`：都不进清单。
fn empty_cases() -> Result<String> {
    // 目录缺失：`skills/` 根本没建。
    let missing = temp_directory("skill-discover-missing")?;
    ensure!(scan(&missing).is_empty(), "发现根目录缺失时应是空清单");

    // 空目录。
    let empty = skill_home("skill-discover-empty")?;
    ensure!(scan(&empty).is_empty(), "空目录应得到空清单");

    // 只有无关子目录（含一层无关文件与二层 `SKILL.md`）。
    let unrelated = skill_home("skill-discover-unrelated")?;
    write(&skill_root(&unrelated).join("notes"), "readme", "只是文件")?;
    write(&skill_root(&unrelated).join("nested/deeper"), "SKILL.md", ALPHA)?;
    let snapshot = scan(&unrelated);
    ensure!(
        snapshot.is_empty(),
        "无关子目录与二层的 SKILL.md 都不应被发现：{:?}",
        snapshot.entries()
    );
    Ok("目录缺失 / 空目录 / 只有无关子目录（含二层 SKILL.md）：空清单".to_owned())
}

/// 二、缺 `SKILL.md` 不进清单；内容非法的进清单且 `INVALID` 带可定位原因。
fn status_cases() -> Result<String> {
    let home = skill_home("skill-discover-status")?;
    write(&skill_dir(&home, "plain"), "notes.md", "不是技能")?;
    write(&skill_dir(&home, "alpha"), "SKILL.md", ALPHA)?;
    write(&skill_dir(&home, "broken"), "SKILL.md", BROKEN)?;
    let snapshot = scan(&home);
    ensure!(
        snapshot
            .entries()
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>()
            == ["alpha", "broken"],
        "缺 SKILL.md 的目录不应进清单：{:?}",
        snapshot.entries()
    );
    let alpha = snapshot.get("alpha").context("快照里应有 alpha")?;
    ensure!(
        alpha.status == SkillStatus::Ready
            && alpha.directory == "skills/alpha"
            && alpha.body_chars == "\n# 步骤\n\n1. 先读模板。\n".chars().count(),
        "READY 条目的状态 / 目录 / 正文字符数不符：{alpha:?}"
    );
    let broken = snapshot.get("broken").context("坏技能也应进清单（名字回落到目录名）")?;
    let error = broken.error().context("INVALID 必须带原因")?.to_string();
    ensure!(
        error.starts_with("skills/broken/SKILL.md:") && error.contains("缺少 name"),
        "INVALID 的原因应可定位到文件与行：{error}"
    );
    ensure!(
        !error.contains(&home.to_string_lossy().to_string()),
        "错误文案不得含绝对路径：{error}"
    );
    Ok(format!("缺 SKILL.md 不进清单；INVALID 带原因：{error}"))
}

/// 三、同名冲突：两份都 `INVALID` 且文案含两个目录；改名后两份都 `READY`。
fn duplicate_cases() -> Result<String> {
    let home = skill_home("skill-discover-duplicate")?;
    write(&skill_dir(&home, "dup-a"), "SKILL.md", SAME)?;
    write(&skill_dir(&home, "dup-b"), "SKILL.md", SAME)?;
    let snapshot = scan(&home);
    ensure!(
        snapshot.len() == 2,
        "两份同名技能都应留在清单里：{:?}",
        snapshot.entries()
    );
    for entry in snapshot.entries() {
        ensure!(
            matches!(entry.status, SkillStatus::Invalid { .. }),
            "同名冲突的两份都应判 INVALID：{entry:?}"
        );
        let message = entry.error().context("冲突条目应带原因")?.to_string();
        let (own, other) = if entry.directory == "skills/dup-a" {
            ("skills/dup-a", "skills/dup-b")
        } else {
            ("skills/dup-b", "skills/dup-a")
        };
        ensure!(
            message.contains(own) && message.contains(other) && message.contains("same"),
            "冲突文案应同时给出两个目录与名字：{message}"
        );
    }

    // 改名之后重新扫描：两份都是 `READY`。
    write(&skill_dir(&home, "dup-b"), "SKILL.md", SAME_RENAMED)?;
    let fixed = scan(&home);
    ensure!(
        fixed.entries().iter().all(agent_skill::SkillEntry::is_ready) && fixed.len() == 2,
        "改名后两份都应 READY：{:?}",
        fixed.entries()
    );
    ensure!(
        fixed
            .entries()
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>()
            == ["other", "same"],
        "改名后的快照应按 name 升序：{:?}",
        fixed.entries()
    );
    Ok("同名冲突：两份 INVALID 且文案含两个目录；改名后两份 READY".to_owned())
}

/// 扫一遍发现根目录。
fn scan(home: &Path) -> SkillSnapshot {
    discover(home, Path::new(ROOT), &SkillLimits::default())
}

/// 写入一份 `SKILL.md`（父目录一并创建）。
fn write(directory: &Path, name: &str, text: &str) -> Result<()> {
    std::fs::create_dir_all(directory).with_context(|| format!("创建目录失败：{}", directory.display()))?;
    let path = directory.join(name);
    std::fs::write(&path, text).with_context(|| format!("写入文件失败：{}", path.display()))
}
