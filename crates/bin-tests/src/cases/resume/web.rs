//! `resume.web`（P6A-015 / 验收 `I-5`）：WebUI 能看见中断态，并提示「发送消息可继续」。
//!
//! 两段证据：
//!
//! - 前端用例文件（`tests/resume-*.test.tsx`）：提示文案只有一个产点、提示条渲染出原因、
//!   中断态下输入框可用且没有恢复按钮、一次发送的两条 Run 各归各的助手气泡；
//! - 静态核对：客户端源码里检索不到 `api/run/resume` 与 `resume_on_occupy`（没有引入恢复动作）。
//!
//! 不读日志、不读事件文件：第一段是前端工程的用例结果，第二段是源码检索。

use std::path::Path;

use anyhow::{Context, Result, bail, ensure};

use crate::case::CaseFuture;
use crate::util::context::TestContext;
use crate::util::frontend;
use crate::util::paths;

/// 未引入的恢复动作与配置项（D-P6-009）：客户端源码里不应出现。
const FORBIDDEN: [&str; 2] = ["api/run/resume", "resume_on_occupy"];

/// 用例入口。
pub(super) fn case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { require(context).await })
}

async fn require(_context: &TestContext) -> Result<Vec<String>> {
    frontend::resume_tests().await.context("前端中断态用例未通过")?;

    let root = paths::repo_root()?;
    let sources = [
        root.join("lingting-agent-ui").join("src"),
        root.join("crates").join("lib-bin-tui").join("src"),
        root.join("crates").join("lib-bin-cli").join("src"),
    ];
    let mut scanned = 0;
    for source in &sources {
        scanned += scan(source, &mut |path, text| {
            for forbidden in FORBIDDEN {
                ensure!(
                    !text.contains(forbidden),
                    "客户端源码不应出现 {forbidden}（未引入恢复动作）：{}",
                    path.display()
                );
            }
            Ok(())
        })?;
    }
    ensure!(scanned > 0, "没有找到任何客户端源码文件");

    Ok(vec![
        "前端用例 tests/resume-paused.test.tsx / resume-banner.test.tsx 通过：提示文案、提示条、输入框可用、无恢复按钮、双 Run 归类".to_owned(),
        format!("客户端源码 {scanned} 个文件里 {} 命中数为 0", FORBIDDEN.join(" / ")),
    ])
}

/// 递归扫描目录下的源码文件，文件名与内容都交给回调；返回文件数。
fn scan(dir: &Path, visit: &mut impl FnMut(&Path, &str) -> Result<()>) -> Result<usize> {
    if !dir.is_dir() {
        bail!("未找到客户端源码目录：{}", dir.display());
    }
    let mut count = 0;
    let entries = std::fs::read_dir(dir).with_context(|| format!("读取目录失败：{}", dir.display()))?;
    for entry in entries {
        let path = entry
            .with_context(|| format!("读取目录项失败：{}", dir.display()))?
            .path();
        if path.is_dir() {
            count += scan(&path, visit)?;
            continue;
        }
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if !matches!(extension, "rs" | "ts" | "tsx") {
            continue;
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("读取源码失败：{}", path.display()))?;
        visit(&path, &text)?;
        count += 1;
    }
    Ok(count)
}
