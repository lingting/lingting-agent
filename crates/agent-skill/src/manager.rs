//! 管理面的唯一领域入口（P5-012 / D-P5-015）：清单、详情与动作参数校验。
//!
//! 这里只做「快照 → 契约类型」的映射与参数口径判定：重扫目录是装配层的事（它持有应用根目录），
//! 启停落回 [`crate::SkillRegistry::set_enabled`]。错误文案只有一份（本模块的 `Display`），接口层
//! 拿去当 400 的 `message`，不另写字面量（D-P5-018）。

use std::fmt;

use crate::availability::{ToolAvailability, missing_tools};
use crate::error::SkillError;
use crate::registry::{SkillEntry, SkillRegistry};
use crate::vo::{SkillAction, SkillDetailVO, SkillStatusKind, SkillVO};

/// 管理动作的唯一失败取值：每一种对应一句对外 400 文案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillManagerError {
    /// 名字不在清单里（含「Skill 功能未启用」与「门面未装配」两种情形）。
    NotFound,
    /// 名字在清单里，但正文取不出来（`INVALID` 与禁用之外的加载失败）。
    LoadFailed(String),
    /// `ENABLE` / `DISABLE` 没给 `name`。
    MissingName,
    /// `RELOAD` 不该带 `name`。
    ReloadWithName,
}

impl fmt::Display for SkillManagerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("技能不存在"),
            Self::LoadFailed(reason) => write!(formatter, "技能加载失败：{reason}"),
            Self::MissingName => formatter.write_str("技能更新缺少 name"),
            Self::ReloadWithName => formatter.write_str("RELOAD 不接受 name"),
        }
    }
}

impl std::error::Error for SkillManagerError {}

/// 动作用到的名字：`ENABLE` / `DISABLE` 必填，`RELOAD` 必须缺省（空串按缺省处理）。
///
/// 参数级校验先于任何查找：缺 `name` 与 `RELOAD` 带 `name` 与「技能存不存在」无关。
pub fn action_name(action: SkillAction, name: Option<&str>) -> Result<Option<&str>, SkillManagerError> {
    let name = name.filter(|name| !name.trim().is_empty());
    match action {
        SkillAction::Enable | SkillAction::Disable => name.map(Some).ok_or(SkillManagerError::MissingName),
        SkillAction::Reload => match name {
            Some(_) => Err(SkillManagerError::ReloadWithName),
            None => Ok(None),
        },
    }
}

/// 清单：全部条目（含 `INVALID` 与已禁用项），按 `name` 升序。
pub fn list(registry: &SkillRegistry, availability: &dyn ToolAvailability) -> Vec<SkillVO> {
    registry
        .entries()
        .into_iter()
        .map(|view| vo(&view.entry, view.enabled, availability))
        .collect()
}

/// 详情：清单字段 + 正文 + 引用清单。
///
/// 禁用项与 `INVALID` 项照常返回元数据（客户端要能显示「为什么不可用」），正文留空；未知名字是
/// [`SkillManagerError::NotFound`]（400，不是 404）。
pub fn detail(
    registry: &SkillRegistry,
    availability: &dyn ToolAvailability,
    name: &str,
) -> Result<SkillDetailVO, SkillManagerError> {
    let view = registry
        .entries()
        .into_iter()
        .find(|view| view.entry.name == name)
        .ok_or(SkillManagerError::NotFound)?;
    let (body, references) = match registry.load(name) {
        Ok(content) => (content.body, content.references),
        // 禁用与 `INVALID`：详情本身可用，原因由 `error` / `enabled` 表达（正文不提供）。
        Err(SkillError::DisabledSkill { .. } | SkillError::InvalidSkill { .. }) => (String::new(), Vec::new()),
        Err(other) => return Err(SkillManagerError::LoadFailed(other.to_string())),
    };
    let vo = vo(&view.entry, view.enabled, availability);
    Ok(SkillDetailVO {
        name: vo.name,
        description: vo.description,
        status: vo.status,
        enabled: vo.enabled,
        requires_tools: vo.requires_tools,
        missing_tools: vo.missing_tools,
        directory: vo.directory,
        reference_count: vo.reference_count,
        error: vo.error,
        body,
        references,
    })
}

/// 快照条目 → 清单视图；缺工具在**这里**现场判定（与索引渲染同一口径，D-P5-009）。
fn vo(entry: &SkillEntry, enabled: bool, availability: &dyn ToolAvailability) -> SkillVO {
    SkillVO {
        name: entry.name.clone(),
        description: entry.metadata.description.clone(),
        status: status_of(entry),
        enabled,
        requires_tools: entry.metadata.requires_tools.clone(),
        missing_tools: missing_tools(&entry.metadata.requires_tools, availability),
        directory: entry.directory.clone(),
        reference_count: u32::try_from(entry.references.len()).unwrap_or(u32::MAX),
        error: entry.error().map(ToString::to_string),
    }
}

/// 注册表状态 → 契约取值。
fn status_of(entry: &SkillEntry) -> SkillStatusKind {
    if entry.is_ready() {
        SkillStatusKind::Ready
    } else {
        SkillStatusKind::Invalid
    }
}
