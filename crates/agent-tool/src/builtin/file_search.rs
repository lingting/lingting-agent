//! `file_search`：在白名单内按文件名搜索（P3-024，D-P3-013）。
//!
//! 遍历纪律：排除目录在**遍历阶段**剪枝（命中即不入栈），不做事后过滤；每个目录的条目先按
//! 文件名排序再入栈，因此同一目录两次执行的输出完全一致。不跟随符号链接（防环、防逃逸）。

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::builtin::path;
use crate::context::ToolContext;
use crate::definition::{ToolFileAccess, ToolRisk};
use crate::error::ToolError;
use crate::output::ToolOutput;
use crate::schema::{self, ToolSchema};
use crate::tool::Tool;

/// 遍历期固定剪枝的目录名（代码常量，不做配置项，D-P3-013）。
const EXCLUDED_DIRS: [&str; 16] = [
    "target",
    "node_modules",
    ".git",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    "out",
    ".next",
    ".cache",
    "coverage",
    "vendor",
    "logs",
    "tmp",
    ".gradle",
];
/// 遍历深度上限：起点目录为第 0 层。
const MAX_DEPTH: usize = 8;
/// 结果条数缺省上限。
const DEFAULT_MAX_RESULTS: usize = 50;
/// 结果条数硬上限：`max_results` 再大也按这个值截断。
const HARD_MAX_RESULTS: usize = 200;

/// `file_search` 的参数。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileSearchArgs {
    /// 文件名子串（忽略 ASCII 大小写；不支持正则与通配符）。
    pub name: String,
    /// 搜索起点目录：缺省为应用根目录，其它路径必须落在白名单内。
    pub path: Option<String>,
    /// 结果条数上限：缺省 50，最大 200。
    pub max_results: Option<i64>,
}

/// 参数 schema：由 [`FileSearchArgs`] 派生一次，进程内只算一遍（D-P3-002）。
static SCHEMA: LazyLock<ToolSchema> = LazyLock::new(schema::of::<FileSearchArgs>);

/// 文件名搜索工具。
pub struct FileSearch {
    allow_dirs: Vec<PathBuf>,
}

impl FileSearch {
    /// 装配工具：白名单来自 `[tools.file]`（与 `file_read` 同一条边界）。
    pub fn new(allow_dirs: Vec<PathBuf>) -> Self {
        Self { allow_dirs }
    }
}

#[async_trait::async_trait]
impl Tool for FileSearch {
    fn name(&self) -> &'static str {
        "file_search"
    }

    fn description(&self) -> &'static str {
        "在白名单目录内按文件名子串搜索文件（忽略大小写，不支持正则与通配符），返回匹配文件的相对路径与大小。"
    }

    fn schema(&self) -> &'static ToolSchema {
        &SCHEMA
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn file_access(&self) -> ToolFileAccess {
        ToolFileAccess::Read
    }

    async fn call(&self, ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        let args: FileSearchArgs = schema::parse(args)?;
        let needle = args.name.to_lowercase();
        if needle.is_empty() {
            return Err(ToolError::InvalidArguments("name 不能为空".to_owned()));
        }
        let limit = result_limit(args.max_results)?;
        let start = path::resolve(&ctx.home, &self.allow_dirs, args.path.as_deref().unwrap_or("."))?;
        if !start.path.is_dir() {
            return Err(ToolError::Failed("搜索起点不是目录".to_owned()));
        }
        let (found, truncated) = walk(&start.path, &needle, limit);
        if found.is_empty() {
            return Ok(ToolOutput::ok(format!(
                "没有匹配的文件（搜索根：{}）",
                start.relative()
            )));
        }
        let root = start.root.clone();
        let mut lines: Vec<String> = found
            .iter()
            .map(|(file, size)| format!("{}（{size} 字节）", path::relative_text(&root, file)))
            .collect();
        // 结果按路径字典序：遍历顺序只影响「哪些进了上限」，不影响这里的输出顺序。
        lines.sort();
        if truncated {
            lines.push(format!("（已截断：达到结果上限 {limit} 条）"));
        }
        Ok(ToolOutput::ok(lines.join("\n")))
    }
}

/// 结果条数：缺省 50，负数 / 0 是参数错误，超过 200 按 200 算。
fn result_limit(requested: Option<i64>) -> Result<usize, ToolError> {
    let Some(requested) = requested else {
        return Ok(DEFAULT_MAX_RESULTS);
    };
    if requested <= 0 {
        return Err(ToolError::InvalidArguments("max_results 必须大于 0".to_owned()));
    }
    let requested = usize::try_from(requested).unwrap_or(HARD_MAX_RESULTS);
    Ok(requested.min(HARD_MAX_RESULTS))
}

/// 深度优先遍历：`(目录, 层数)` 显式栈，命中排除目录即不入栈（遍历期剪枝）。
///
/// 返回（匹配到的文件与大小，是否因为达到上限而截断）。条目按文件名排序后再入栈/匹配，
/// 保证同一目录两次执行的「谁先占满上限」也一致。
fn walk(start: &Path, needle: &str, limit: usize) -> (Vec<(PathBuf, u64)>, bool) {
    let mut pending: Vec<(PathBuf, usize)> = vec![(start.to_path_buf(), 0)];
    let mut found: Vec<(PathBuf, u64)> = Vec::new();
    let mut truncated = false;
    while let Some((directory, depth)) = pending.pop() {
        if found.len() >= limit {
            truncated = true;
            break;
        }
        let Ok(entries) = std::fs::read_dir(&directory) else {
            // 读不了的目录（权限、竞态删除）跳过，不影响其余结果。
            continue;
        };
        let mut children: Vec<_> = entries.filter_map(Result::ok).collect();
        children.sort_by_key(|entry| entry.file_name());
        for entry in children {
            if found.len() >= limit {
                truncated = true;
                break;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            // 不跟随符号链接：目录环与「链接指向白名单外」都在这里被挡掉（D-P3-013）。
            if file_type.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if file_type.is_dir() {
                if depth + 1 > MAX_DEPTH || EXCLUDED_DIRS.contains(&name.as_str()) {
                    continue;
                }
                pending.push((entry.path(), depth + 1));
                continue;
            }
            if !file_type.is_file() || !name.contains(needle) {
                continue;
            }
            let size = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
            found.push((entry.path(), size));
        }
    }
    (found, truncated)
}
