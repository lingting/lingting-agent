//! 生成器共用的输出目录处理（P1-007）。
//!
//! 生成器不直接写目标目录：先写同级暂存目录，全部成功后再整体替换目标目录。
//! 因此生成失败时上一次的有效产物保持不变，不会留下半成品。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// 暂存目录后缀，形如 `.src.gen-tmp`。
const STAGING_SUFFIX: &str = ".gen-tmp";
/// 替换期间的旧产物备份后缀，形如 `.src.gen-old`。
const BACKUP_SUFFIX: &str = ".gen-old";

/// 准备暂存目录：清掉上次中断留下的残留，返回已创建的空目录。
pub fn prepare_staging(target: &Path) -> io::Result<PathBuf> {
    let staging = sibling(target, STAGING_SUFFIX);
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    Ok(staging)
}

/// 用暂存目录整体替换目标目录：旧目录先挪到备份位，替换成功后再删除备份。
pub fn commit_staging(staging: &Path, target: &Path) -> io::Result<()> {
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
