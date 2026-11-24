//! 工具权限：按名字给出「允许 / 拒绝 / 需确认」。
//!
//! Batch A 只提供最小形态（按名字查表，供注册表生成模型可见清单）；`[tools] permissions`
//! 的装载、未配置时按风险等级取默认值，以及 `deny > confirm > allow > 默认` 的判定顺序
//! 在 P3-005 补齐（README §4.3、D-P3-005）。

use std::collections::BTreeMap;

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
    pub fn permission_of(&self, name: &str) -> Option<ToolPermission> {
        self.permissions.get(name).copied()
    }

    /// 是否被显式拒绝：被拒绝的工具不进模型可见清单（D-P3-005）。
    pub fn is_denied(&self, name: &str) -> bool {
        self.permission_of(name) == Some(ToolPermission::Deny)
    }
}
