//! 工具注册表：注册与查找的唯一入口，「哪些工具能发给模型」由权限过滤后的清单决定。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::definition::ToolDefinition;
use crate::permission::{ToolPermission, ToolPolicy};
use crate::tool::{Tool, validate_name};

/// 注册失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// 工具名不符合 `^[a-z][a-z0-9_]{1,63}$`。
    InvalidName(String),
    /// 同名工具已注册：不覆盖先注册的实现。
    DuplicateName(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(name) => write!(formatter, "工具名不合法：{name}"),
            Self::DuplicateName(name) => write!(formatter, "工具名重复注册：{name}"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// 进程内只读工具表：装配期一次性注册，运行期只读（`Arc<ToolRegistry>`）。
pub struct ToolRegistry {
    policy: ToolPolicy,
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// 建表：权限只影响「发给模型的清单」，不影响表里有什么。
    pub fn new(policy: ToolPolicy) -> Self {
        Self {
            policy,
            tools: BTreeMap::new(),
        }
    }

    /// 注册一个工具：名字非法或重名直接拒绝，不覆盖已注册的实现。
    ///
    /// 被拒绝（`deny`）的工具照常注册：执行期仍要能查到它，才能对「模型凭空调用」回 `DENIED`。
    pub fn register(&mut self, tool: Arc<dyn Tool>) -> Result<(), RegistryError> {
        let name = tool.name().to_string();
        if !validate_name(&name) {
            return Err(RegistryError::InvalidName(name));
        }
        if self.tools.contains_key(&name) {
            return Err(RegistryError::DuplicateName(name));
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    /// 执行期查找。
    pub fn lookup(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// 权限表：执行器据此做「执行期二次判定」（`deny` 的工具仍可能被模型凭空调用）。
    pub fn policy(&self) -> &ToolPolicy {
        &self.policy
    }

    /// 发给模型的工具清单。
    ///
    /// 顺序只由名字字典序决定（与注册顺序无关），同一份配置两次运行产出同序清单；
    /// `deny` 的工具被摘掉，`confirm` 的工具仍在清单里（D-P3-005）。空清单是合法状态。
    ///
    /// 「拒绝」既可能来自 `[tools.permissions]` 的显式配置，也可能来自风险等级的缺省等级
    /// （`Network` / `Write` 默认拒绝）：只看配置会让 `http_request` 在缺省配置下照样进清单，
    /// 与 D-P3-005 相反。因此这里按「名字 + 工具自己的风险等级」判一次有效权限。
    pub fn model_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .filter(|(name, tool)| self.policy.permission_of(name, tool.risk()) != ToolPermission::Deny)
            .map(|(_, tool)| ToolDefinition::of(tool.as_ref()))
            .collect()
    }

    /// 已注册的工具数量（含被拒绝的工具）。
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// 是否没有任何工具注册。
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}
