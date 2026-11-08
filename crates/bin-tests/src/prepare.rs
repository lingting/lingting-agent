//! 用例前置准备：按选中用例声明的要求，在启动服务端之前一次性准备好环境。
//!
//! 只处理**运行期**准备（如构建 WebUI 产物）。SDK 生成不在这里：`lib-bin-cli` 用 `#[path]`
//! 引入生成物，缺生成物时 `bin-tests` 自己编译不出来，所以生成必须发生在构建之前，留在
//! `Makefile` 的 `test` 前置里（见 `.plan/p1-016-webui-tests.md`「既定约束」）。

use crate::case::{Case, Prep};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// 前端工程目录名（仓库根目录下）。
const UI_DIR: &str = "lingting-agent-ui";

/// 按选中用例的要求依次执行准备动作，同类准备只做一次。
pub async fn ensure(cases: &[Case]) -> Result<()> {
    let mut needed: Vec<Prep> = cases.iter().flat_map(|case| case.prep.iter().copied()).collect();
    needed.sort();
    needed.dedup();

    for prep in needed {
        println!("准备：{}", prep.label());
        match prep {
            Prep::WebUiBuild => build_web_ui().await?,
        }
    }
    Ok(())
}

/// 构建 WebUI 产物：静态托管类用例读的是 `lingting-agent-ui/dist`。
async fn build_web_ui() -> Result<()> {
    let dir = repo_root()?.join(UI_DIR);
    if !dir.join("package.json").is_file() {
        bail!("未找到前端工程：{}", dir.display());
    }
    if !dir.join("node_modules").is_dir() {
        bail!("前端依赖未安装，请先执行 make ui-install（{}）", dir.display());
    }
    run_pnpm(&dir, &["build"]).await.context("构建 WebUI 产物失败")
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

/// 仓库根目录：由 `crates/bin-tests` 上溯两级，不依赖测试进程的当前工作目录。
fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("无法定位仓库根目录")
}
