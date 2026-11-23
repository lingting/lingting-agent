//! 仓库路径：所有「相对仓库根目录」的定位都从这里取，不依赖进程当前工作目录。

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// 仓库根目录：由 `crates/bin-tests` 上溯两级。
pub(crate) fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("无法定位仓库根目录")
}
