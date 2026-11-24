//! 白名单路径解析（P3-023 / P3-024，D-P3-013）：`canonicalize` + 组件级前缀比较。
//!
//! 判定顺序：相对路径基于应用根目录（`AGENT_HOME`）→ `canonicalize` → 结果必须
//! `starts_with` 某个白名单根。用 `Path::starts_with`（按组件比较）而不是字符串前缀：
//! 后者会把 `/data/agent-tool-evil` 判成在白名单 `/data/agent-tool` 内。
//!
//! 拒绝文案统一且**不回显真实绝对路径**：模型只需要知道「越界了」，不需要知道盘符布局。

use std::path::{Path, PathBuf};

use crate::error::ToolError;

/// 路径被拒绝时的统一文案：不区分「越界」与「不存在」，也不带盘符。
pub(crate) const REJECTED_TEXT: &str = "路径不在允许的范围内或不存在";

/// 一次成功解析的结果。
pub(crate) struct Resolved {
    /// 规范化后的绝对路径：只在工具内部使用，不进入模型可见文本。
    pub path: PathBuf,
    /// 命中的白名单根：结果文本只写相对这个根的路径（D-P3-013）。
    pub root: PathBuf,
}

impl Resolved {
    /// 相对白名单根的路径文本（统一用 `/` 分隔，模型侧不区分平台）。
    pub fn relative(&self) -> String {
        relative_text(&self.root, &self.path)
    }
}

/// 解析并校验一个模型给出的路径。
pub(crate) fn resolve(home: &Path, allow_dirs: &[PathBuf], requested: &str) -> Result<Resolved, ToolError> {
    let requested = requested.trim();
    if requested.is_empty() {
        return Err(ToolError::InvalidArguments("path 不能为空".to_owned()));
    }
    let raw = Path::new(requested);
    let candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        home.join(raw)
    };
    // `canonicalize` 同时承担「解析符号链接」与「必须存在」两件事：指向白名单外的符号链接
    // 会在这里被解析成真实路径，再被下面的前缀判定拦住（D-P3-013）。
    let path = candidate.canonicalize().map_err(|_| rejected())?;
    match roots(home, allow_dirs).into_iter().find(|root| path.starts_with(root)) {
        Some(root) => Ok(Resolved { path, root }),
        None => Err(rejected()),
    }
}

/// 路径文本：相对某个已知根，分隔符统一成 `/`；根自身用 `.` 表示。
pub(crate) fn relative_text(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) if relative.as_os_str().is_empty() => ".".to_owned(),
        Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
        Err(_) => ".".to_owned(),
    }
}

/// 允许的根目录集合：应用根目录恒在，白名单目录能规范化成功才加入。
///
/// 白名单为空时只允许应用根目录（D-P3-013）；白名单里写错的目录（不存在）不加入集合，
/// 结果是「宁可拒绝，也不放宽」。
fn roots(home: &Path, allow_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::with_capacity(allow_dirs.len() + 1);
    roots.push(home.canonicalize().unwrap_or_else(|_| home.to_path_buf()));
    for directory in allow_dirs {
        if let Ok(canonical) = directory.canonicalize() {
            roots.push(canonical);
        }
    }
    roots
}

fn rejected() -> ToolError {
    ToolError::Failed(REJECTED_TEXT.to_owned())
}
