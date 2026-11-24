//! 工具权限：按名字给出「允许 / 拒绝 / 需确认」。
//!
//! 判定顺序固定（README §4.3）：`deny` > `confirm` > `allow` > 工具默认。
//! 「配置里显式写下的值」永远胜出；没写过的工具按 [`ToolRisk`] 的缺省等级判定，
//! 因此「显式 `allow` 能让默认拒绝的工具可用」与「显式 `deny` 永远优先」同时成立（D-P3-005）。
//!
//! 本模块只做判定，不做装载：`[tools.permissions]` 的解析与校验在 `lib-bin-cli::config`，
//! 装配层把配置值逐条喂进 [`ToolPolicy::new`]（`agent-tool` 不认识 `types-core` 的配置类型）。

use std::collections::BTreeMap;

use crate::definition::ToolRisk;

/// 单个工具的权限判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermission {
    /// 允许：进模型可见清单，可执行。
    Allow,
    /// 拒绝：不进模型可见清单，执行期二次判定为 `DENIED`。
    Deny,
    /// 需确认：进模型可见清单，执行前挂起 Run 等客户端确认。
    Confirm,
}

/// 按工具名索引的权限表。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolPolicy {
    permissions: BTreeMap<String, ToolPermission>,
}

impl ToolPolicy {
    /// 什么都不配：所有工具按各自默认等级判定。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 由「工具名 → 权限」构造。
    pub fn new(permissions: impl IntoIterator<Item = (String, ToolPermission)>) -> Self {
        Self {
            permissions: permissions.into_iter().collect(),
        }
    }

    /// 配置里显式写下的权限；未配置返回 `None`。
    pub fn configured(&self, name: &str) -> Option<ToolPermission> {
        self.permissions.get(name).copied()
    }

    /// 判定一个工具的权限：配置里的值优先，否则按风险等级的缺省等级（D-P3-005）。
    pub fn permission_of(&self, name: &str, default_risk: ToolRisk) -> ToolPermission {
        self.configured(name)
            .unwrap_or_else(|| default_risk.default_permission())
    }

    /// 是否被显式拒绝：被拒绝的工具不进模型可见清单（D-P3-005）。
    pub fn is_denied(&self, name: &str) -> bool {
        self.configured(name) == Some(ToolPermission::Deny)
    }
}
