//! 前端工程命令：定位目录、检查依赖、构建产物与跑前端用例。
//!
//! `prepare`（构建产物）与 `web.ui` 用例（跑前端用例）共用这里的入口。
//! 目录一律由 `CARGO_MANIFEST_DIR` 定位，不依赖测试进程的当前工作目录。

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// 前端工程目录名（仓库根目录下）。
const UI_DIR: &str = "lingting-agent-ui";

/// 仓库根目录：由 `crates/bin-tests` 上溯两级。
pub(crate) fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("无法定位仓库根目录")
}

/// 前端工程目录，并确认它确实存在。
pub(crate) fn dir() -> Result<PathBuf> {
    let dir = repo_root()?.join(UI_DIR);
    if !dir.join("package.json").is_file() {
        bail!("未找到前端工程：{}", dir.display());
    }
    Ok(dir)
}

/// 构建 WebUI 产物：静态托管类用例读的是 `lingting-agent-ui/dist`。
pub(crate) async fn build() -> Result<()> {
    let dir = installed_dir()?;
    run_pnpm(&dir, &["build"]).await.context("构建 WebUI 产物失败")
}

/// 跑前端用例（`pnpm test` → `vp test run`）。
pub(crate) async fn test() -> Result<()> {
    let dir = installed_dir()?;
    run_pnpm(&dir, &["test"]).await.context("前端用例失败")
}

/// 取前端工程目录并确认依赖已安装。
fn installed_dir() -> Result<PathBuf> {
    let dir = dir()?;
    if !dir.join("node_modules").is_dir() {
        bail!("前端依赖未安装，请先执行 make ui-install（{}）", dir.display());
    }
    Ok(dir)
}

/// 执行一条 pnpm 子命令：输出直通控制台，退出码非 0 视为失败。
async fn run_pnpm(dir: &Path, args: &[&str]) -> Result<()> {
    let mut command = pnpm();
    let status = command
        .args(args)
        .current_dir(dir)
        .status()
        .await
        .context("启动 pnpm 失败（确认已安装 pnpm）")?;
    if !status.success() {
        bail!("pnpm {} 失败：{status}", args.join(" "));
    }
    Ok(())
}

/// pnpm 命令：Windows 上是 `pnpm.cmd`/`pnpm.ps1`，必须经 `cmd /C` 才找得到。
fn pnpm() -> Command {
    if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.arg("/C").arg("pnpm");
        command
    } else {
        Command::new("pnpm")
    }
}
