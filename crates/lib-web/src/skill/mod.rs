//! Skill 管理接口（P5-012）：清单、详情与启停 / 重载。
//!
//! 路径沿用 D-P5-015：资源段 `skill`，动作段只有 `list` / `get` / `update`，没有路径占位符
//! （`name` 走查询参数或请求体，D-008）。
//!
//! Handler 只做「参数解包 + 门面调用 + 错误映射」：清单、详情与动作口径在 `agent-skill` 的
//! `manager` 模块，`RELOAD` 的重扫在 `manager-agent`（它持有应用根目录）。错误映射只在本文件发生一次。

pub mod api;
pub mod dto;

use framework_web::WebError;
use manager_agent::SkillManagerError;

/// 管理动作的唯一错误出口：已知的领域错误 → 400 + 固定文案，其余 → 500。
///
/// 400 的文案取自 [`SkillManagerError`] 的 `Display`（`agent-skill/src/manager.rs` 是唯一产点），
/// 接口层不另写一份字面量；500 的对外文案由框架统一成 `服务器内部错误`，细节只进日志。
pub(crate) fn api_error(error: anyhow::Error) -> anyhow::Error {
    let Some(kind) = error.downcast_ref::<SkillManagerError>() else {
        return WebError::internal("Skill 管理接口内部错误", error).into();
    };
    WebError::parameter(kind.to_string(), error).into()
}
