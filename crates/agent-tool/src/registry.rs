//! 工具注册表：注册与查找的唯一入口，「哪些工具能发给模型」由权限过滤后的清单决定。
//!
//! 运行期可变（P4-003，D-P4-004）：MCP 的工具发现是异步事件（连接成功、`tools/list_changed`、
//! 重连），注册与注销都发生在低频路径上，读路径（每轮取清单、每次调用查找）只取读锁。
//!
//! 锁纪律：写锁只出现在工具快照差异处，读锁不跨 `await` 持有（本模块的方法都是同步的，
//! 因此这一点由调用方保证——不要持有 [`ToolRegistry`] 的借用去等异步结果）。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

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

/// 注册表的一条：工具本体 + 注册时给出的条目默认权限。
///
/// 条目默认权限是「server 级权限」的落点（P4 的 MCP）：配置里按 server 给出，注册时随工具进来，
/// 不需要在装载期就知道运行期才出现的工具名（D-P4-007）。
struct Entry {
    /// 工具本体。
    tool: Arc<dyn Tool>,
    /// 注册时给出的缺省权限；`None` 表示按工具自己的风险等级判定（P3 的口径）。
    default_permission: Option<ToolPermission>,
}

/// 进程内工具表：装配期注册内置工具，运行期由工具发现增删。
pub struct ToolRegistry {
    policy: ToolPolicy,
    tools: RwLock<BTreeMap<String, Entry>>,
}

impl ToolRegistry {
    /// 建表：权限只影响「发给模型的清单」，不影响表里有什么。
    pub fn new(policy: ToolPolicy) -> Self {
        Self {
            policy,
            tools: RwLock::new(BTreeMap::new()),
        }
    }

    /// 注册一个工具：名字非法或重名直接拒绝，不覆盖已注册的实现。
    ///
    /// 被拒绝（`deny`）的工具照常注册：执行期仍要能查到它，才能对「模型凭空调用」回 `DENIED`。
    pub fn register(&self, tool: Arc<dyn Tool>) -> Result<(), RegistryError> {
        self.register_with_default(tool, None)
    }

    /// 注册一个工具并给出条目默认权限（`None` = 按风险等级判定）。
    ///
    /// 权限判定的三级顺序见 [`ToolRegistry::permission_of`]。
    pub fn register_with_default(
        &self,
        tool: Arc<dyn Tool>,
        default_permission: Option<ToolPermission>,
    ) -> Result<(), RegistryError> {
        let name = tool.name().to_string();
        if !validate_name(&name) {
            return Err(RegistryError::InvalidName(name));
        }
        let mut tools = self.write();
        if tools.contains_key(&name) {
            return Err(RegistryError::DuplicateName(name));
        }
        tools.insert(
            name,
            Entry {
                tool,
                default_permission,
            },
        );
        Ok(())
    }

    /// 注销一个工具：返回被移除的工具（未注册时返回 `None`）。
    ///
    /// 工具从 server 的工具快照里消失、server 被禁用或服务关闭时调用；执行期随后按
    /// 「未注册」处理，等价于 `DENIED`。
    pub fn unregister(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.write().remove(name).map(|entry| entry.tool)
    }

    /// 执行期查找：克隆 `Arc`，不把注册表内部的借用交给调用方（增删可能随时发生）。
    pub fn lookup(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.read().get(name).map(|entry| Arc::clone(&entry.tool))
    }

    /// 一个工具的实际权限：`[tools.permissions]` 显式值 > 条目默认 > 风险等级缺省（D-P4-004）。
    ///
    /// 未注册的工具返回 `None`：调用方按拒绝处理，与 [`ToolPolicy::configured`] 的「没配过」区分开。
    pub fn permission_of(&self, name: &str) -> Option<ToolPermission> {
        let tools = self.read();
        let entry = tools.get(name)?;
        Some(self.permission_of_entry(name, entry))
    }

    /// 已注册的工具名，按字典序（工具发现的差集核对用）。
    pub fn names(&self) -> Vec<String> {
        self.read().keys().cloned().collect()
    }

    /// 发给模型的工具清单。
    ///
    /// 顺序只由名字字典序决定（与注册顺序无关），同一份配置两次运行产出同序清单；
    /// `deny` 的工具被摘掉，`confirm` 的工具仍在清单里（D-P3-005）。空清单是合法状态。
    ///
    /// 「拒绝」既可能来自 `[tools.permissions]` 的显式配置、MCP 的条目默认权限，也可能来自
    /// 风险等级的缺省等级（`Network` / `Write` 默认拒绝）：只看配置会让 `http_request` 在缺省
    /// 配置下照样进清单，与 D-P3-005 相反。因此这里按三级顺序判一次有效权限。
    pub fn model_definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.read();
        tools
            .iter()
            .filter(|(name, entry)| self.permission_of_entry(name, entry) != ToolPermission::Deny)
            .map(|(_, entry)| ToolDefinition::of(entry.tool.as_ref()))
            .collect()
    }

    /// 已注册的工具数量（含被拒绝的工具）。
    pub fn len(&self) -> usize {
        self.read().len()
    }

    /// 是否没有任何工具注册。
    pub fn is_empty(&self) -> bool {
        self.read().is_empty()
    }

    /// 单条的三级权限判定：显式配置 > 条目默认 > 风险等级缺省。
    fn permission_of_entry(&self, name: &str, entry: &Entry) -> ToolPermission {
        self.policy
            .configured(name)
            .or(entry.default_permission)
            .unwrap_or_else(|| entry.tool.risk().default_permission())
    }

    /// 取读锁；锁中毒时沿用内部数据（与 `manager-agent` 的锁表口径一致）。
    fn read(&self) -> RwLockReadGuard<'_, BTreeMap<String, Entry>> {
        self.tools.read().unwrap_or_else(|error| error.into_inner())
    }

    /// 取写锁；中毒处理同 [`ToolRegistry::read`]。
    fn write(&self) -> RwLockWriteGuard<'_, BTreeMap<String, Entry>> {
        self.tools.write().unwrap_or_else(|error| error.into_inner())
    }
}
