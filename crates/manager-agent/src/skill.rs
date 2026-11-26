//! Skill 的装配侧适配：工具可用性判定（P5-006 / D-P5-009）。
//!
//! 判定口径只有一条：**此刻能不能真的执行**。未注册的工具算不可用（MCP 还没连上、或 server 的
//! 工具快照里没有它），权限为 `deny` 的工具也算不可用——它不会出现在模型的工具清单里。
//!
//! 判定发生在渲染索引与加载正文时（不是发现时），因此 MCP 在运行期连上之后，缺失标记会自动消失，
//! 不需要重载 Skill。

use std::sync::Arc;

use agent_skill::ToolAvailability;
use agent_tool::{ToolPermission, ToolRegistry};

/// 按工具注册表的现状回答「这个工具可用吗」。
pub struct RegistryAvailability(Arc<ToolRegistry>);

impl RegistryAvailability {
    /// 用装配期建好的注册表建一个判定器。
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self(registry)
    }
}

impl ToolAvailability for RegistryAvailability {
    fn available(&self, name: &str) -> bool {
        // `permission_of` 只对已注册的工具返回 `Some`，因此这一句同时覆盖「未注册」与「被拒绝」。
        matches!(self.0.permission_of(name), Some(permission) if permission != ToolPermission::Deny)
    }
}
