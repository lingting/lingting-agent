//! Skill 管理接口的三个 handler（P5-012，形态见 D-P5-015）。
//!
//! 读接口只转发门面快照（`INVALID` 与已禁用项都在清单里）；写接口只转发一个动作，三个 `action`
//! 都返回**动作完成后**的整张清单，客户端不必二次查询。

use anyhow::Result;
use framework_web::{Json, Query, web_api_get, web_api_post};

use crate::skill::api_error;
use crate::skill::dto::{SkillDetailVO, SkillGetParams, SkillUpdatePO, SkillVO};

/// 列出全部 Skill（含 `INVALID` 与已禁用项），按 `name` 升序。
///
/// 未装配或 `skills.toml` 的 `enabled = false` 时返回空清单，不报错：管理面看到的就是「没有 Skill」。
#[web_api_get(path = "api/skill/list")]
pub async fn skill_list() -> Vec<SkillVO> {
    manager_agent::skill_list()
}

/// 一条 Skill 的详情（清单字段 + 正文 + 引用清单）。
///
/// 未知 `name` 是 400 `技能不存在`，不是 404：`name` 是请求参数里的标识，与 MCP 的 `serverId`
/// 同一套判定口径（`docs/api/skill.md`）。
#[web_api_get(path = "api/skill/get")]
pub async fn skill_get(Query(po): Query<SkillGetParams>) -> Result<SkillDetailVO> {
    manager_agent::skill_get(&po).map_err(api_error)
}

/// 执行一个管理动作（`ENABLE` / `DISABLE` / `RELOAD`），返回动作完成后的整张清单。
///
/// `RELOAD` 必须不带 `name`（否则 400 `RELOAD 不接受 name`），`ENABLE` / `DISABLE` 必须带 `name`
/// （否则 400 `技能更新缺少 name`）。
#[web_api_post(path = "api/skill/update")]
pub async fn skill_update(Json(po): Json<SkillUpdatePO>) -> Result<Vec<SkillVO>> {
    manager_agent::skill_update(&po).map_err(api_error)
}
