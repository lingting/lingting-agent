//! 仓库路径与 Skill 用例的应用根目录：所有「相对仓库根目录」的定位都从这里取，
//! 不依赖进程当前工作目录。

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::util::support::temp_directory;

/// 仓库根目录：由 `crates/bin-tests` 上溯两级。
pub(crate) fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("无法定位仓库根目录")
}

/// Skill 用例的临时应用根目录：建好 `<home>/skills/` 布局，返回 `home`。
///
/// 布局与真实应用根目录一致（`skills.toml`、`skills/<dir>/SKILL.md`），因此发现层、注册表与
/// 装配层在用例里看到的是同一条路径形态（`skills/<dir>`），不出现绝对路径。
pub(crate) fn skill_home(name: &str) -> Result<PathBuf> {
    let home = temp_directory(name)?;
    std::fs::create_dir_all(skill_root(&home)).with_context(|| format!("创建 Skill 目录失败：{}", home.display()))?;
    Ok(home)
}

/// 发现根目录：`<home>/skills`。
pub(crate) fn skill_root(home: &Path) -> PathBuf {
    home.join("skills")
}

/// 一个 Skill 的目录：`<home>/skills/<dir>`。
pub(crate) fn skill_dir(home: &Path, directory: &str) -> PathBuf {
    skill_root(home).join(directory)
}
