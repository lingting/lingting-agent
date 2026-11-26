//! Skill 发现：把 `<home>/<root>` 下的目录扫成一张确定性快照（P5-004）。
//!
//! 布局唯一（D-P5-006）：只认 `<root>/<dir>/SKILL.md` 一层，不递归找 Skill。
//!
//! - 目录缺失 / 读不出来 = 零 Skill，不是错误；
//! - 没有 `SKILL.md` 的目录**不进清单**（它没有声明自己是 Skill，只是一坨文件）；
//! - `SKILL.md` 存在但解析失败 → `INVALID`，带着可定位错误；
//! - 两个目录声明同一个 `name` → **两份都** `INVALID`，文案同时给出两个目录（D-P5-005）；
//! - 不做符号链接跟随：Skill 目录本身与 `references/` 下的链接都不跟。
//!
//! 产出顺序由 [`SkillSnapshot::new`] 统一排序（`name` 升序，同名的按目录名），因此两次扫描逐字段
//! 可比。文案里的目录一律是**相对应用根目录**的形态，绝对形式的 `root` 只取最后一段，保证不吐
//! 绝对路径（D-P5-018）。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::SkillError;
use crate::reference::scan_references;
use crate::registry::{SkillEntry, SkillLimits, SkillSnapshot, SkillStatus};
use crate::skill::{SkillMetadata, parse};

/// Skill 的文件名（大小写不敏感匹配）。
const SKILL_FILE: &str = "skill.md";
/// 文案里用的 `SKILL.md` 正规写法。
const SKILL_FILE_DISPLAY: &str = "SKILL.md";
/// 绝对 `root` 无法相对化时，文案里用的兜底根目录名。
const FALLBACK_ROOT: &str = "skills";

/// 扫一遍发现根目录，返回排好序的快照。
///
/// `root` 相对应用根目录（`home`）解析，绝对路径按原样；整文件字节上限从 `limits` 取，解析器本身
/// 不读配置。
pub fn discover(home: &Path, root: &Path, limits: &SkillLimits) -> SkillSnapshot {
    let root_dir = if root.is_absolute() {
        root.to_path_buf()
    } else {
        home.join(root)
    };
    let relative_root = relative_root(root);
    let mut entries = scan(&root_dir, &relative_root, limits);
    mark_duplicates(&mut entries);
    SkillSnapshot::new(entries)
}

/// 扫描一层子目录：每个带 `SKILL.md` 的目录产出一条。
fn scan(root_dir: &Path, relative_root: &str, limits: &SkillLimits) -> Vec<SkillEntry> {
    let mut entries = Vec::new();
    let Ok(reader) = fs::read_dir(root_dir) else {
        return entries;
    };
    for item in reader {
        let Ok(item) = item else {
            continue;
        };
        let Some(directory_name) = directory_name(&item) else {
            continue;
        };
        let Some(path) = skill_file(&item.path()) else {
            continue;
        };
        let relative_dir = format!("{relative_root}/{directory_name}");
        entries.push(read_entry(&path, &relative_dir, &directory_name, limits));
    }
    entries
}

/// 子目录名：只接受真正的目录（符号链接指向的目录也不跟）。
fn directory_name(item: &fs::DirEntry) -> Option<String> {
    let metadata = fs::symlink_metadata(item.path()).ok()?;
    if !metadata.is_dir() {
        return None;
    }
    let name = item.file_name().to_string_lossy().into_owned();
    (!name.is_empty()).then_some(name)
}

/// 目录下的 `SKILL.md`：文件名大小写不敏感、必须是普通文件；有多个匹配时取名字最小的那个，
/// 保证同一份目录两次扫描得到同一个结果。
fn skill_file(directory: &Path) -> Option<PathBuf> {
    let reader = fs::read_dir(directory).ok()?;
    let mut found: Option<String> = None;
    for item in reader.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        if name.to_ascii_lowercase() != SKILL_FILE {
            continue;
        }
        let metadata = fs::symlink_metadata(item.path()).ok()?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        if found.as_deref().is_none_or(|current| name.as_str() < current) {
            found = Some(name);
        }
    }
    found.map(|name| directory.join(name))
}

/// 读一条 Skill：解析失败、读不出来、引用扫描失败都落成 `INVALID`（坏的也进清单）。
fn read_entry(path: &Path, relative_dir: &str, directory_name: &str, limits: &SkillLimits) -> SkillEntry {
    let relative_path = format!("{relative_dir}/{SKILL_FILE_DISPLAY}");
    let invalid = |error: SkillError| invalid_entry(directory_name, relative_dir, error);
    let Ok(bytes) = fs::read(path) else {
        return invalid(SkillError::UnreadableFile { path: relative_path });
    };
    let file = match parse(&relative_path, &bytes, limits.max_skill_bytes) {
        Ok(file) => file,
        Err(error) => return invalid(error),
    };
    let skill_dir = path.parent().unwrap_or(path);
    let references = match scan_references(skill_dir, relative_dir) {
        Ok(references) => references,
        Err(error) => return invalid(error),
    };
    let file = file.with_references(references);
    SkillEntry {
        name: file.metadata.name.clone(),
        directory: relative_dir.to_owned(),
        status: SkillStatus::Ready,
        metadata: file.metadata,
        body_chars: file.body.chars().count(),
        body: file.body,
        sections: file.sections,
        references: file.references,
    }
}

/// 解析没走完的条目：名字回落到目录名（只为清单可定位），其余字段保持空。
fn invalid_entry(directory_name: &str, relative_dir: &str, error: SkillError) -> SkillEntry {
    SkillEntry {
        name: directory_name.to_owned(),
        directory: relative_dir.to_owned(),
        status: SkillStatus::Invalid { error },
        metadata: SkillMetadata::default(),
        body: String::new(),
        body_chars: 0,
        sections: Vec::new(),
        references: Vec::new(),
    }
}

/// 同名冲突：所有声明同一个名字的条目**全部**判 `INVALID`，每条错误都给出另一个目录。
///
/// 不做「先到先得」：静默丢掉一份会让用户看不到原因（D-P5-005）。解析失败的条目没有声明名字，
/// 不参与冲突判定。
fn mark_duplicates(entries: &mut [SkillEntry]) {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.is_ready() {
            groups.entry(entry.name.clone()).or_default().push(index);
        }
    }
    for (name, mut indexes) in groups {
        if indexes.len() < 2 {
            continue;
        }
        indexes.sort_by(|left, right| entries[*left].directory.cmp(&entries[*right].directory));
        let first = entries[indexes[0]].directory.clone();
        let second = entries[indexes[1]].directory.clone();
        for (position, index) in indexes.iter().enumerate() {
            let own = entries[*index].directory.clone();
            let other = if position == 0 { second.clone() } else { first.clone() };
            entries[*index].status = SkillStatus::Invalid {
                error: SkillError::DuplicateName {
                    name: name.clone(),
                    first: own,
                    second: other,
                },
            };
        }
    }
}

/// 发现根目录在文案里的相对形态：相对路径原样（`/` 分隔），绝对路径只取最后一段。
fn relative_root(root: &Path) -> String {
    let path = if root.is_absolute() {
        root.file_name().map_or_else(|| Path::new(FALLBACK_ROOT), Path::new)
    } else {
        root
    };
    let parts: Vec<String> = path
        .iter()
        .map(|part| part.to_string_lossy().into_owned())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        FALLBACK_ROOT.to_owned()
    } else {
        parts.join("/")
    }
}
