//! `web.sdk-only`：前端不得手写 API path，所有调用必须来自生成的 SDK（P1-016 验收标准 2）。

use crate::case::CaseFuture;
use crate::context::TestContext;
use crate::frontend;
use anyhow::{Context, Result, ensure};
use std::fs;
use std::path::Path;

/// 前端源码目录名（前端工程下）。
const UI_SRC: &str = "src";

/// 视为「手写接口 path」的片段：引号（或模板串）后直接跟 `api/`。
///
/// 只认这种形态：`@/api/client`、`./api/x` 这类模块路径前面不是引号，不会被误报。
const ENDPOINT_MARKERS: [&str; 3] = ["\"api/", "'api/", "`api/"];

/// 扫描前端源码的非注释行，断言没有手写接口 path。
pub(super) fn sdk_only(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let root = frontend::dir()?.join(UI_SRC);
        ensure!(root.is_dir(), "未找到前端源码目录：{}", root.display());

        let mut scanned = 0usize;
        let mut violations: Vec<String> = Vec::new();
        scan(&root, &mut scanned, &mut violations)?;

        ensure!(
            violations.is_empty(),
            "前端源码出现手写接口 path（应改用生成的 ApiDefinitions）：\n{}",
            violations.join("\n")
        );
        Ok(vec![format!("扫描 {scanned} 个前端源码文件，未发现手写接口 path")])
    })
}

/// 递归扫描目录里的 TS/TSX 源码，收集手写接口 path 的位置。
fn scan(dir: &Path, scanned: &mut usize, violations: &mut Vec<String>) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("读取目录失败：{}", dir.display()))?;
    for entry in entries {
        let path = entry
            .with_context(|| format!("读取目录项失败：{}", dir.display()))?
            .path();
        if path.is_dir() {
            scan(&path, scanned, violations)?;
            continue;
        }
        if !is_source(&path) {
            continue;
        }
        *scanned += 1;
        let text = fs::read_to_string(&path).with_context(|| format!("读取源码失败：{}", path.display()))?;
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
                continue;
            }
            if ENDPOINT_MARKERS.iter().any(|marker| line.contains(marker)) {
                violations.push(format!("{}:{}：{trimmed}", path.display(), index + 1));
            }
        }
    }
    Ok(())
}

/// 是否是需要扫描的前端源码文件。
fn is_source(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| matches!(extension, "ts" | "tsx"))
}
