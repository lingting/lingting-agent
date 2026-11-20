//! 会话占用接口：`create` / `delete` / `get`（D-P1.1-002、RT-014）。
//!
//! 语义边界全部在 `manager_agent::runtime`：Handler 只做解包、调用与错误映射；
//! 冲突表达（400 + 固定 message）与「会话不存在 / 会话未被占用」（404）由 `runtime_error` 统一产出。

use crate::runtime::{runtime_error, state};
use anyhow::Result;
use framework_web::{Json, Query, web_api_get, web_api_post};
use types_core::runtime::{OccupancyCreatePO, OccupancyDeletePO, OccupancyGetParams, OccupancyVO};

/// 建立占用：成功即启动该会话的运行时会话并打开事件文件。
///
/// 同一对 `(clientId, sessionId)` 重复调用幂等返回当前占用；三类冲突各自返回一条固定 message。
#[web_api_post(path = "api/occupancy/create")]
pub async fn occupancy_create(Json(po): Json<OccupancyCreatePO>) -> Result<OccupancyVO> {
    state()?.acquire(po).await.map_err(runtime_error)
}

/// 释放占用：幂等；释放即停止运行时会话（追加终态事件并 flush）。
#[web_api_post(path = "api/occupancy/delete")]
pub async fn occupancy_delete(Json(po): Json<OccupancyDeletePO>) -> Result<()> {
    state()?.release_occupancy(&po.client_id).await.map_err(runtime_error)
}

/// 查询占用：无人占用返回 404 `会话未被占用`。
#[web_api_get(path = "api/occupancy/get")]
pub async fn occupancy_get(Query(po): Query<OccupancyGetParams>) -> Result<OccupancyVO> {
    state()?.occupancy(&po.session_id).map_err(runtime_error)
}
