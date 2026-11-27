//! Skill 注册表：发现结果的快照、启用状态与上限参数（P5-004 / P5-005）。
//!
//! 快照是「目录里有什么」的唯一事实来源：每个 Skill 一条，坏的也在里面（带原因）。注册表在此之上
//! 只加两件事：**启用状态**（内存里的开关集合）与**上限参数**。
//!
//! 锁纪律（与 `agent-tool::ToolRegistry` 一致）：写路径只有 [`SkillRegistry::reload`] 与
//! [`SkillRegistry::set_enabled`]；读路径（渲染索引、按名查找、出清单）只取读锁。本模块的方法都是
//! 同步的，因此锁不会跨 `await` 持有。
//!
//! 本 Crate 不依赖 `types-core`：`skills.toml` 的上限由装配层翻译成 [`SkillLimits`] 再喂进来
//! （D-P5-007）；[`SkillLimits::default`] 只在「尚未装配」的进程里兜底。

use std::collections::BTreeSet;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::availability::ToolAvailability;
use crate::error::SkillError;
use crate::index::{self, SkillIndex};
use crate::load::{self, SkillContent};
use crate::metrics;
use crate::section::SkillSection;
use crate::skill::SkillMetadata;

/// 索引字符数上限的兜底缺省值（与 `skills.toml` 的缺省值一致，装配层总是显式传入）。
pub const DEFAULT_MAX_INDEX_CHARS: usize = 4_000;
/// 单条描述字符数上限的兜底缺省值。
pub const DEFAULT_MAX_DESCRIPTION_CHARS: usize = 200;
/// 单个 `SKILL.md` 字节上限的兜底缺省值（64 KiB）。
pub const DEFAULT_MAX_SKILL_BYTES: u64 = 65_536;
/// 单次加载返回正文字符数上限的兜底缺省值。
pub const DEFAULT_MAX_INLINE_CHARS: usize = 6_000;

/// Skill 的四个上限：`agent-skill` 自己的类型，与配置类型解耦。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillLimits {
    /// 索引（进系统提示的那一段）字符数上限。
    pub max_index_chars: usize,
    /// 单条描述在索引里的字符数上限。
    pub max_description_chars: usize,
    /// 单个 `SKILL.md` 的字节上限。
    pub max_skill_bytes: u64,
    /// 单次加载返回正文字符数上限。
    pub max_inline_chars: usize,
}

impl Default for SkillLimits {
    fn default() -> Self {
        Self {
            max_index_chars: DEFAULT_MAX_INDEX_CHARS,
            max_description_chars: DEFAULT_MAX_DESCRIPTION_CHARS,
            max_skill_bytes: DEFAULT_MAX_SKILL_BYTES,
            max_inline_chars: DEFAULT_MAX_INLINE_CHARS,
        }
    }
}

/// 一条 Skill 的可用性判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillStatus {
    /// 解析成功，可以进索引、可以加载。
    Ready,
    /// 解析失败或同名冲突：**不进索引、不可加载**，但一定在清单里带着原因（判据 E-1）。
    Invalid {
        /// 判 `INVALID` 的可定位原因。
        error: SkillError,
    },
}

impl SkillStatus {
    /// 是否 `READY`。
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// `INVALID` 的原因。
    pub fn error(&self) -> Option<&SkillError> {
        match self {
            Self::Ready => None,
            Self::Invalid { error } => Some(error),
        }
    }
}

/// 快照里的一条 Skill。
///
/// `body` 随快照常驻：一份 `SKILL.md` 不超过 `max_skill_bytes`，正文只在 [`SkillRegistry::load`]
/// 时被取走，不参与索引渲染，也不会进系统提示（D-P5-002）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillEntry {
    /// 逻辑标识：解析成功时是 frontmatter 的 `name`，解析失败时回落到目录名（只为定位用）。
    pub name: String,
    /// 相对应用根目录的目录路径（`skills/<dir>`），错误文案与接口都只用这个形态。
    pub directory: String,
    /// `READY` / `INVALID { error }`。
    pub status: SkillStatus,
    /// frontmatter 的字段；`INVALID` 的条目可能是空结构（解析没走到那一步）。
    pub metadata: SkillMetadata,
    /// 正文原样文本（供 [`SkillRegistry::load`] 使用）。
    pub body: String,
    /// 正文字符数（Unicode 标量值个数）。
    pub body_chars: usize,
    /// ATX 标题切出的章节表。
    pub sections: Vec<SkillSection>,
    /// `references/` 的引用清单（相对应用根目录）。
    pub references: Vec<String>,
}

impl SkillEntry {
    /// 是否 `READY`。
    pub fn is_ready(&self) -> bool {
        self.status.is_ready()
    }

    /// `INVALID` 的原因。
    pub fn error(&self) -> Option<&SkillError> {
        self.status.error()
    }
}

/// 一次发现的完整结果：按 `name` 升序（同名的按目录名升序），两次扫描逐字节可比（判据 D-1）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillSnapshot {
    entries: Vec<SkillEntry>,
}

impl SkillSnapshot {
    /// 用发现结果建快照：排序在这里统一做，调用方不需要关心顺序。
    pub fn new(mut entries: Vec<SkillEntry>) -> Self {
        entries.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.directory.cmp(&right.directory))
        });
        Self { entries }
    }

    /// 全部条目（含 `INVALID`），按 `name` 升序。
    pub fn entries(&self) -> &[SkillEntry] {
        &self.entries
    }

    /// 按名字查找（`INVALID` 条目也能查到）。
    pub fn get(&self, name: &str) -> Option<&SkillEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    /// 条目数量。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否没有任何条目。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 清单用的一条：快照条目 + 当前启用状态（接口与客户端只看这个视图）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillView {
    /// 快照里的条目。
    pub entry: SkillEntry,
    /// 当前是否启用（`disabled` 名单的反面）。
    pub enabled: bool,
}

/// Skill 注册表：快照 + 启用状态 + 上限。
pub struct SkillRegistry {
    /// 发现结果快照；`reload` 整体替换。
    snapshot: RwLock<SkillSnapshot>,
    /// 被禁用的名字；启停只在内存（D-P5-007）。
    disabled: RwLock<BTreeSet<String>>,
    /// 上限参数，装配期定死，运行期不变。
    limits: SkillLimits,
}

impl SkillRegistry {
    /// 建一个空注册表：启动期由 [`SkillRegistry::reload`] 装入发现结果。
    pub fn new(limits: SkillLimits) -> Self {
        Self {
            snapshot: RwLock::new(SkillSnapshot::default()),
            disabled: RwLock::new(BTreeSet::new()),
            limits,
        }
    }

    /// 上限参数。
    pub fn limits(&self) -> &SkillLimits {
        &self.limits
    }

    /// 当前快照的克隆（`P5-012` 的详情接口与用例用它做逐字段比对）。
    pub fn snapshot(&self) -> SkillSnapshot {
        self.read_snapshot().clone()
    }

    /// 快照条目数（含 `INVALID`）：`metrics` 的 `discovered` 用它，不必克隆正文。
    pub fn entry_count(&self) -> usize {
        self.read_snapshot().len()
    }

    /// 整体替换快照（`RELOAD` / 启动期发现）：新增、删除、改名、格式修正都在这一次内生效。
    ///
    /// `disabled` 名单只保留仍然存在的名字（D-P5-008）：被删掉的 Skill 不会在名单里留残渣。
    pub fn reload(&self, snapshot: SkillSnapshot) {
        let names: BTreeSet<String> = snapshot.entries().iter().map(|entry| entry.name.clone()).collect();
        self.write_disabled().retain(|name| names.contains(name));
        *self.write_snapshot() = snapshot;
        metrics::record_reload();
    }

    /// 启用 / 禁用一条 Skill；名字不存在时返回 [`SkillError::UnknownSkill`]。
    ///
    /// 禁用一条 `INVALID` 的 Skill 是允许的（它本来就不进索引），只要名字在清单里。
    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), SkillError> {
        if self.read_snapshot().get(name).is_none() {
            return Err(SkillError::UnknownSkill { name: name.to_owned() });
        }
        let mut disabled = self.write_disabled();
        if enabled {
            disabled.remove(name);
        } else {
            disabled.insert(name.to_owned());
        }
        Ok(())
    }

    /// 一条 Skill 是否启用（未登记的名字也算未启用）。
    pub fn is_enabled(&self, name: &str) -> bool {
        !self.read_disabled().contains(name)
    }

    /// 清单：全部条目（含 `INVALID` 与已禁用项）+ 启用标记，按 `name` 升序。
    pub fn entries(&self) -> Vec<SkillView> {
        let snapshot = self.read_snapshot();
        let disabled = self.read_disabled();
        snapshot
            .entries()
            .iter()
            .map(|entry| SkillView {
                entry: entry.clone(),
                enabled: !disabled.contains(&entry.name),
            })
            .collect()
    }

    /// 当前可用的 Skill 数量（`READY && enabled`）：指标与接口共用这一个口径。
    pub fn enabled_count(&self) -> usize {
        let snapshot = self.read_snapshot();
        let disabled = self.read_disabled();
        snapshot
            .entries()
            .iter()
            .filter(|entry| entry.is_ready() && !disabled.contains(&entry.name))
            .count()
    }

    /// `INVALID` 的条目数量。
    pub fn invalid_count(&self) -> usize {
        self.read_snapshot()
            .entries()
            .iter()
            .filter(|entry| !entry.is_ready())
            .count()
    }

    /// 渲染索引小节（进系统提示的那一段）：只取 `READY && enabled`，按预算整行截断。
    ///
    /// 工具可用性在**渲染时**现场判定（D-P5-009）：MCP 连上之后不需要重载 Skill。
    pub fn index(&self, availability: &dyn ToolAvailability) -> SkillIndex {
        let snapshot = self.read_snapshot();
        let disabled = self.read_disabled();
        let index = index::render(
            snapshot.entries(),
            |name| !disabled.contains(name),
            &self.limits,
            availability,
        );
        // `index_chars` 取最近一次渲染的字符数（`store` 语义，P5-011）。
        metrics::record_index_chars(index.chars);
        index
    }

    /// 按名加载一条 Skill 的正文、章节表与引用清单（P5-005）。
    ///
    /// 三种失败各有独立文案：未知名字、已禁用、`INVALID`（带着解析期的可定位错误）。工具缺失
    /// **不阻断**加载：缺失清单由调用方用 [`crate::availability`] 的文案拼进返回文本（D-P5-009）。
    ///
    /// 成败各记一次 `metrics`（P5-011）：`skill_load` 之外的加载路径（管理面详情）同样计入。
    pub fn load(&self, name: &str) -> Result<SkillContent, SkillError> {
        let result = self.resolve(name);
        match &result {
            Ok(content) => metrics::record_load(content.sections.len()),
            Err(_) => metrics::record_load_failure(),
        }
        result
    }

    /// 加载的实际判定：`load` 只负责在外面记一次成败。
    fn resolve(&self, name: &str) -> Result<SkillContent, SkillError> {
        // 先判启用状态、再取快照：两把锁不同时持有，写路径（`reload` / `set_enabled`）也就不可能
        // 与读路径形成反向等待。
        let enabled = self.is_enabled(name);
        let entry = self.read_snapshot().get(name).cloned();
        let entry = entry.ok_or_else(|| SkillError::UnknownSkill { name: name.to_owned() })?;
        if let Some(error) = entry.error() {
            return Err(SkillError::InvalidSkill {
                name: entry.name.clone(),
                error: Box::new(error.clone()),
            });
        }
        if !enabled {
            return Err(SkillError::DisabledSkill {
                name: entry.name.clone(),
            });
        }
        Ok(load::content(&entry))
    }

    /// 取快照读锁；锁中毒时沿用内部数据（与 `agent-tool::ToolRegistry` 同口径）。
    fn read_snapshot(&self) -> RwLockReadGuard<'_, SkillSnapshot> {
        self.snapshot.read().unwrap_or_else(|error| error.into_inner())
    }

    /// 取快照写锁；中毒处理同 [`SkillRegistry::read_snapshot`]。
    fn write_snapshot(&self) -> RwLockWriteGuard<'_, SkillSnapshot> {
        self.snapshot.write().unwrap_or_else(|error| error.into_inner())
    }

    /// 取禁用名单读锁。
    fn read_disabled(&self) -> RwLockReadGuard<'_, BTreeSet<String>> {
        self.disabled.read().unwrap_or_else(|error| error.into_inner())
    }

    /// 取禁用名单写锁。
    fn write_disabled(&self) -> RwLockWriteGuard<'_, BTreeSet<String>> {
        self.disabled.write().unwrap_or_else(|error| error.into_inner())
    }
}
