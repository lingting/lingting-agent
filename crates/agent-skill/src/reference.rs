//! 引用清单扫描：`references/` 子目录下递归的普通文件，产出**相对应用根目录**的路径（D-P5-012）。
//!
//! 安全口径（判据 D-6 / E-4）：
//!
//! - 不跟随符号链接：`references` 目录本身是符号链接时按「没有引用材料」处理，扫描中遇到的符号
//!   链接一律跳过（目录与文件都不跟）；
//! - 只收普通文件：目录本身不进清单，管道 / 套接字等特殊文件跳过；
//! - 路径里不含 `..` / `.` 分量：产出前逐分量检查，不合规的条目直接丢弃——引用路径要能直接喂给
//!   既有的 `file_read`，越权的路径不能出现在清单里；
//! - 顺序确定：按相对路径字典序，超过 [`MAX_REFERENCES`] 条时截断（先排序再截断）。
//!
//! 引用材料本身不加载、不执行：这里只给清单，正文由模型按需用 `file_read` 取（D-P5-012）。

use std::fs;
use std::path::Path;

use crate::error::SkillError;

/// 引用清单的条目上限。
pub const MAX_REFERENCES: usize = 200;
/// 引用材料所在的子目录名。
const REFERENCES_DIR: &str = "references";

/// 扫描一个 Skill 目录下的引用材料。
///
/// `skill_dir` 是该 Skill 的目录（文件系统路径），`relative_dir` 是它相对应用根目录的路径
/// （形如 `skills/pdf-report`）——产出的每一条都以它开头，错误文案里也不会出现绝对路径。
pub fn scan_references(skill_dir: &Path, relative_dir: &str) -> Result<Vec<String>, SkillError> {
    let root = skill_dir.join(REFERENCES_DIR);
    let unreadable = || SkillError::UnreadableReferences {
        path: format!("{relative_dir}/{REFERENCES_DIR}"),
    };

    // 目录不存在 = 没有引用材料，不是错误；符号链接不跟随（`symlink_metadata` 不解析链接）。
    let metadata = match fs::symlink_metadata(&root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(unreadable()),
    };
    if !metadata.is_dir() {
        return Ok(Vec::new());
    }

    let mut found = Vec::new();
    walk(&root, "", &mut found).map_err(|_| unreadable())?;
    found.sort();
    found.truncate(MAX_REFERENCES);
    Ok(found
        .into_iter()
        .map(|item| format!("{relative_dir}/{REFERENCES_DIR}/{item}"))
        .filter(|item| safe_path(item))
        .collect())
}

/// 递归收集普通文件的相对路径（以 `/` 分隔，相对 `references/`）。
fn walk(directory: &Path, prefix: &str, found: &mut Vec<String>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        // 符号链接（无论指向文件还是目录）一律不跟。
        if metadata.file_type().is_symlink() {
            continue;
        }
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if metadata.is_dir() {
            walk(&entry.path(), &relative, found)?;
        } else if metadata.is_file() {
            found.push(relative);
        }
    }
    Ok(())
}

/// 路径的每个分量都不能是 `.` / `..`（引用清单要能直接喂给文件工具）。
fn safe_path(path: &str) -> bool {
    !path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
}
