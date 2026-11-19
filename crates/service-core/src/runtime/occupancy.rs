//! 会话占用：一对一约束、建立与释放（RT-014）。
//!
//! 校验顺序固定为「注册 → 会话存在 → 客户端一对一 → 会话独占」（D-P1.1-002），
//! 因此同一请求只可能命中一条冲突，客户端不必按优先级猜测。
//!
//! 建立顺序固定为「先登记占用，再建运行时会话」：登记是纯内存动作、可回滚；文件与任务不可回滚。
//! 释放顺序固定为「停止运行时会话（追加终态事件 + flush）→ 撤回登记」。

use lib_core::{id, time};
use tracing::warn;
use types_core::runtime::{OccupancyCreatePO, OccupancyVO};

use super::{RuntimeError, RuntimeState, SessionStop};
use crate::holder;

impl RuntimeState {
    /// 建立占用：成功即启动运行时会话并打开事件文件。
    ///
    /// 同一对 `(clientId, sessionId)` 重复调用是幂等的：直接返回当前占用，不重建会话、不重开文件。
    pub async fn acquire(&self, params: OccupancyCreatePO) -> Result<OccupancyVO, RuntimeError> {
        if self.client(&params.client_id).is_none() {
            return Err(RuntimeError::ClientNotRegistered);
        }
        if !holder::session().has_session(&params.session_id) {
            return Err(RuntimeError::SessionNotFound(params.session_id));
        }

        // 同一会话的建立与释放串行：避免两个请求同时判定“空闲”后各建一份事件文件。
        let lock = self.session_lock(&params.session_id);
        let _guard = lock.lock().await;

        if let Some(current) = self.occupancy_of_session(&params.session_id) {
            return if current.client_id == params.client_id {
                Ok(current)
            } else {
                Err(RuntimeError::SessionOccupied)
            };
        }
        if self.occupancy_of_client(&params.client_id).is_some() {
            return Err(RuntimeError::ClientBusy);
        }

        let occupancy = OccupancyVO {
            session_id: params.session_id,
            client_id: params.client_id,
            runtime_session_id: next_id()?,
            acquired_at_ms: time::now_millis(),
        };
        // 先登记占用：此后任何时刻“有占用记录 ⇔ 有运行时会话”，失败补偿立即执行。
        self.occupy(occupancy.clone());
        if let Err(error) = self.start_session(&occupancy).await {
            self.release(&occupancy.client_id);
            warn!(
                %error,
                session_id = %occupancy.session_id,
                "建立运行时会话失败，已撤回占用登记"
            );
            return Err(RuntimeError::Internal(error.to_string()));
        }
        Ok(occupancy)
    }

    /// 释放占用：停止运行时会话（追加终态事件 + flush）后撤回登记。
    ///
    /// 幂等：该客户端没有占用时同样返回成功。停止失败只记日志，登记照常撤回：
    /// 留一条没有会话的占用记录比泄漏一个文件句柄更糟。
    pub async fn release_occupancy(&self, client_id: &str) -> Result<(), RuntimeError> {
        if self.client(client_id).is_none() {
            return Err(RuntimeError::ClientNotRegistered);
        }
        let Some(occupancy) = self.occupancy_of_client(client_id) else {
            return Ok(());
        };

        let lock = self.session_lock(&occupancy.session_id);
        let _guard = lock.lock().await;
        if let Some(result) = self
            .stop_session(&occupancy.session_id, SessionStop::OccupancyReleased)
            .await
            && let Err(error) = result
        {
            warn!(%error, session_id = %occupancy.session_id, "停止运行时会话失败，继续撤回占用登记");
        }
        self.release(client_id);
        Ok(())
    }

    /// 查询占用：无人占用返回 [`RuntimeError::NotOccupied`]（对外 404）。
    pub fn occupancy(&self, session_id: &str) -> Result<OccupancyVO, RuntimeError> {
        self.occupancy_of_session(session_id).ok_or(RuntimeError::NotOccupied)
    }
}

/// 生成运行时会话 ID（也是事件文件名），底层错误收敛为内部错误。
fn next_id() -> Result<String, RuntimeError> {
    id::next_id().map_err(|error| RuntimeError::Internal(format!("生成 ID 失败：{error:#}")))
}
