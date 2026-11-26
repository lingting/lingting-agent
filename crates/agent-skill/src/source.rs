//! 索引来源：把「注册表 + 工具可用性」打包成一次可渲染的对象（P5-007）。
//!
//! `agent-context` 只认识这一个类型：它不知道快照、不知道 `skills.toml`，也不需要知道工具注册表。
//! 每次组装都现场渲染（无缓存），因此改完 Skill 或 MCP 连上工具之后，下一次请求即生效
//! （D-P5-008 / D-P5-009）。
//!
//! 渲染结果只含「名字 + 描述 + 缺工具标注」，正文不在这里：正文只能经 `skill_load` 进入上下文
//! （D-P5-002）。

use std::sync::Arc;

use crate::availability::ToolAvailability;
use crate::index::SkillIndex;
use crate::registry::SkillRegistry;

/// 系统提示里那段 Skill 索引的来源。
pub struct SkillIndexSource {
    /// 唯一的事实来源：快照 + 启用状态 + 上限。
    registry: Arc<SkillRegistry>,
    /// 工具可用性判定器：索引渲染时现场判定缺工具（D-P5-009）。
    availability: Arc<dyn ToolAvailability>,
}

impl SkillIndexSource {
    /// 由装配层构造：注册表与判定器都来自同一次装配。
    pub fn new(registry: Arc<SkillRegistry>, availability: Arc<dyn ToolAvailability>) -> Self {
        Self { registry, availability }
    }

    /// 渲染一次索引小节（含预算截断与「另有 N 个」那行）。
    pub fn index(&self) -> SkillIndex {
        self.registry.index(self.availability.as_ref())
    }

    /// 只取小节文本：没有任何可用 Skill 时是空串，调用方据此决定是否发系统消息。
    pub fn section(&self) -> String {
        self.index().section
    }
}
