//! 前端工程命令：定位目录、检查依赖、构建产物、跑单元与端到端用例。
//!
//! `prepare`（构建产物）、`web.ui` 与 `web.e2e` 用例共用这里的入口。
//! 目录一律由 `paths::repo_root` 定位，不依赖测试进程的当前工作目录。

use crate::paths;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// 前端工程目录名（仓库根目录下）。
const UI_DIR: &str = "lingting-agent-ui";

/// 前端工程目录，并确认它确实存在。
pub(crate) fn dir() -> Result<PathBuf> {
    let dir = paths::repo_root()?.join(UI_DIR);
    if !dir.join("package.json").is_file() {
        bail!("未找到前端工程：{}", dir.display());
    }
    Ok(dir)
}

/// 构建 WebUI 产物：静态托管类用例读的是 `lingting-agent-ui/dist`。
pub(crate) async fn build() -> Result<()> {
    let dir = installed_dir()?;
    run_pnpm(&dir, &["build"], &[]).await.context("构建 WebUI 产物失败")
}

/// 跑前端单元用例（`pnpm test` → `vp test run`）。
pub(crate) async fn test() -> Result<()> {
    let dir = installed_dir()?;
    run_pnpm(&dir, &["test"], &[]).await.context("前端单元用例失败")
}

/// 跑端到端用例（`pnpm test:e2e` → `playwright test`）。
///
/// 服务端由调用方启动，地址通过 `WEBUI_BASE_URL` 传给 Playwright 配置；浏览器二进制需事先
/// 安装（`make ui-e2e-install`），本函数不自动下载。
pub(crate) async fn e2e(base_url: &str) -> Result<()> {
    let dir = installed_dir()?;
    run_pnpm(&dir, &["test:e2e"], &[("WEBUI_BASE_URL", base_url)])
        .await
        .context("前端端到端用例失败")
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
async fn run_pnpm(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<()> {
    let mut command = pnpm();
    for (name, value) in env {
        command.env(name, value);
    }
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
