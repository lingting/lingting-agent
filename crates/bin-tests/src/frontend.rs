//! 前端工程命令：定位目录、检查依赖、构建产物、跑单元与端到端用例。
//!
//! `prepare`（构建产物）、`web.ui` 与 `web.e2e` 用例共用这里的入口。
//! 目录一律由 `paths::repo_root` 定位，不依赖测试进程的当前工作目录。

use crate::paths;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
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
///
/// 产物比所有构建输入都新时直接复用，避免每次跑测试都重新构建（一次约 1.3 秒）。
pub(crate) async fn build() -> Result<()> {
    let dir = installed_dir()?;
    let dist = dir.join("dist");
    if is_fresh(&dir, &dist)? {
        println!("跳过：WebUI 产物已是最新（{}）", dist.display());
        return Ok(());
    }
    println!("构建 WebUI 产物：{}", dist.display());
    run_pnpm(&dir, &["build"], &[]).await.context("构建 WebUI 产物失败")
}

/// 跑前端单元用例（`vp test run`）。
pub(crate) async fn test() -> Result<()> {
    let dir = installed_dir()?;
    // 用例文件数不多，限 4 个 worker：既不浪费高核机器，也不让 jsdom 争 CPU。
    run_pnpm(&dir, &["exec", "vp", "test", "run", "--maxWorkers", "4"], &[])
        .await
        .context("前端单元用例失败")
}

/// 跑端到端用例（`playwright test`）。
///
/// 服务端由调用方启动，地址通过 `WEBUI_BASE_URL` 传给 Playwright 配置；浏览器二进制需事先
/// 安装（`make ui-e2e-install`），本函数不自动下载。
pub(crate) async fn e2e(base_url: &str) -> Result<()> {
    let dir = installed_dir()?;
    run_pnpm(&dir, &["exec", "playwright", "test"], &[("WEBUI_BASE_URL", base_url)])
        .await
        .context("前端端到端用例失败")
}

/// 产物是否比所有构建输入都新：`dist/index.html` 不早于源码树里最新的修改。
fn is_fresh(ui_dir: &Path, dist: &Path) -> Result<bool> {
    let Ok(index) = fs::metadata(dist.join("index.html")).and_then(|meta| meta.modified()) else {
        return Ok(false);
    };
    Ok(newest_modified(ui_dir)?.is_some_and(|newest| index >= newest))
}

/// 递归取最新的修改时间，跳过不参与构建的目录与文件。
fn newest_modified(dir: &Path) -> Result<Option<SystemTime>> {
    let mut newest = None;
    let entries = fs::read_dir(dir).with_context(|| format!("读取目录失败：{}", dir.display()))?;
    for entry in entries {
        let path = entry
            .with_context(|| format!("读取目录项失败：{}", dir.display()))?
            .path();
        let name = path.file_name().and_then(|value| value.to_str()).unwrap_or_default();
        if path.is_dir() {
            if matches!(name, "node_modules" | "dist" | "tests") {
                continue;
            }
            newest = keep_newest(newest, newest_modified(&path)?);
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) == Some("tsbuildinfo") {
            continue;
        }
        if let Ok(modified) = path.metadata().and_then(|meta| meta.modified()) {
            newest = keep_newest(newest, Some(modified));
        }
    }
    Ok(newest)
}

/// 取两个「最新修改时间」里更新的那个。
fn keep_newest(current: Option<SystemTime>, candidate: Option<SystemTime>) -> Option<SystemTime> {
    match (current, candidate) {
        (Some(current), Some(candidate)) if current >= candidate => Some(current),
        (_, Some(candidate)) => Some(candidate),
        (current, None) => current,
    }
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
