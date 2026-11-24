//! 工具接口（P3-019）：只读查询一次 Run 的工具审计记录。
//!
//! 路径与动词沿用 `docs/api` 的规范：资源段小写单数（`tool`），动作段只用契约里固定的动词
//! （`list`）；`runId` 走查询参数，没有路径占位符（D-008）。
//!
//! 审计记录在写出侧就已脱敏（D-P3-011），Handler 不再做任何二次处理。

use anyhow::Result;
use framework_web::{Query, web_api_get};
use manager_agent::{ToolAuditListParams, ToolCallVO};

use crate::session::api::api_error;

/// 按 Run 查询工具审计记录，按产生时间升序。
///
/// 未知 `runId` 返回空列表（审计是「发生过什么」的流水，不是会话对象，不做存在性校验）；
/// `runId` 缺失是参数错误，由框架的查询参数转换返回 400。
#[web_api_get(path = "api/tool/audit/list")]
pub async fn tool_audit_list(Query(po): Query<ToolAuditListParams>) -> Result<Vec<ToolCallVO>> {
    manager_agent::list_tool_calls(po).await.map_err(api_error)
}
