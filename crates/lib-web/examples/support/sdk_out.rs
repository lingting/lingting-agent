//! 生成器共用的输出目录处理（P1-007）。
//!
//! 生成器不直接写目标目录：先写同级暂存目录，全部成功后再整体替换目标目录。
//! 因此生成失败时上一次的有效产物保持不变，不会留下半成品。
//!
//! 替换前先按文件内容逐字节比对：暂存目录与目标目录完全一致时只删暂存目录、不碰目标目录。
//! 生成文件的 mtime 一旦刷新，cargo 的 `#[path]` 依赖与前端构建的依赖图都会无谓重编译；
//! 只要有任一文件不同就整体替换目录，不做逐文件同步——下游按 crate / 模块粒度重编，
//! 只同步部分新文件没有收益，反而会留下新旧混杂的目录。

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// 暂存目录后缀，形如 `.src.gen-tmp`。
const STAGING_SUFFIX: &str = ".gen-tmp";
/// 替换期间的旧产物备份后缀，形如 `.src.gen-old`。
const BACKUP_SUFFIX: &str = ".gen-old";

/// 目标目录的内容快照：相对路径（`/` 分隔）→ 文件内容。
type Snapshot = BTreeMap<String, Vec<u8>>;

/// 一次提交的结果。
pub struct Commit {
    /// 目标目录是否被整体替换；`false` 表示产物与上次完全一致，目标目录未被触碰。
    pub replaced: bool,
    /// 相对暂存目录的新增文件。
    pub added: Vec<String>,
    /// 与目标目录内容不同的文件。
    pub updated: Vec<String>,
    /// 目标目录中存在、暂存目录中已消失的文件。
    pub removed: Vec<String>,
}

impl Commit {
    /// 打印提交结果：无变化时明确说明未写盘，有变化时列出差异文件。
    pub fn report(&self, target: &Path) {
        if !self.replaced {
            println!("  内容无变化，未写盘 → {}", target.display());
            return;
        }
        for path in &self.added {
            println!("  新增 {path}");
        }
        for path in &self.updated {
            println!("  更新 {path}");
        }
        for path in &self.removed {
            println!("  删除 {path}");
        }
    }
}

/// 准备暂存目录：清掉上次中断留下的残留，返回已创建的空目录。
pub fn prepare_staging(target: &Path) -> io::Result<PathBuf> {
    let staging = sibling(target, STAGING_SUFFIX);
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    Ok(staging)
}

/// 按内容比对后提交：完全一致时丢弃暂存目录，否则整体替换目标目录。
pub fn commit(staging: &Path, target: &Path) -> io::Result<Commit> {
    let staged = snapshot(staging)?;
    let current = if target.is_dir() {
        snapshot(target)?
    } else {
        Snapshot::new()
    };
    let commit = compare(&staged, &current);
    if commit.replaced {
        replace(staging, target)?;
    } else {
        fs::remove_dir_all(staging)?;
    }
    Ok(commit)
}

/// 读取目录下全部普通文件的内容快照（递归、不含符号链接）。
///
/// 比对用逐字节比较而不是内容哈希：长度不同立即判定，长度相同才逐字节扫一遍，
/// 省掉哈希计算，且生成物都在 KB ~ 百 KB 量级，一次性读入内存没有压力。
fn snapshot(root: &Path) -> io::Result<Snapshot> {
    let mut files = Snapshot::new();
    collect(root, root, &mut files)?;
    Ok(files)
}

fn collect(root: &Path, dir: &Path, files: &mut Snapshot) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let key = relative.to_string_lossy().replace('\\', "/");
            files.insert(key, fs::read(&path)?);
        }
    }
    Ok(())
}

/// 比对两份快照，得出新增 / 更新 / 删除清单。
fn compare(staged: &Snapshot, current: &Snapshot) -> Commit {
    let mut commit = Commit {
        replaced: false,
        added: Vec::new(),
        updated: Vec::new(),
        removed: Vec::new(),
    };
    for (path, content) in staged {
        match current.get(path) {
            None => commit.added.push(path.clone()),
            Some(existing) if existing != content => commit.updated.push(path.clone()),
            Some(_) => {}
        }
    }
    for path in current.keys() {
        if !staged.contains_key(path) {
            commit.removed.push(path.clone());
        }
    }
    commit.replaced = !(commit.added.is_empty() && commit.updated.is_empty() && commit.removed.is_empty());
    commit
}

/// 用暂存目录整体替换目标目录：旧目录先挪到备份位，替换成功后再删除备份。
fn replace(staging: &Path, target: &Path) -> io::Result<()> {
    let backup = sibling(target, BACKUP_SUFFIX);
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    if target.exists() {
        fs::rename(target, &backup)?;
    }
    if let Err(error) = fs::rename(staging, target) {
        // 回滚：替换失败时把旧产物放回原位，保住上一次的有效输出。
        if backup.exists() {
            let _ = fs::rename(&backup, target);
        }
        return Err(error);
    }
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    Ok(())
}

/// 目标目录的同级隐藏目录，保证 rename 落在同一文件系统内。
fn sibling(target: &Path, suffix: &str) -> PathBuf {
    let name = target.file_name().and_then(|name| name.to_str()).unwrap_or("sdk");
    target.with_file_name(format!(".{name}{suffix}"))
}
