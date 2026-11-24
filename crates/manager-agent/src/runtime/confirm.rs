//! 工具确认的服务端链路（P3-027 / D-P3-023）：决定落库 → 重新排队 → 重连补发。
//!
//! 鉴权口径固定为三层：客户端已注册、该客户端正是占用该会话的客户端、
//! `toolCallId` 等于该会话里某个 Run 的 `pending_tool_call_id`。前两层在内存注册表里判，
//! 第三层与「并发两次确认只有一次命中」都交给数据库的条件更新（`WHERE pending_tool_call_id = ?`）。
//!
//! 本模块只做编排：挂起态的形状在 `service-core`，事件生产仍在运行时会话的生产方上
//! （重连补发与执行方走同一条路径，`seq` 由写者统一分配）。

use agent_core::session::{RunEvent, RunEventKind};
use lib_core::time::now_millis;
use service_core::session::ResumeDecision;
use tracing::{debug, warn};
use types_core::runtime::ClientDecision;

use super::{RuntimeError, RuntimeState};

impl RuntimeState {
    /// 提交一次工具确认，返回本次 pong 的时刻。
    ///
    /// 确认同时刷新 `lastSeen`：确认期间心跳不必被误判超时（D-P3-022）。命中后 Run 回
    /// `QUEUED` 并唤醒会话队列，由队列重新取（D-P3-027）。
    pub async fn submit_confirm(
        &self,
        client_id: &str,
        tool_call_id: &str,
        decision: ClientDecision,
    ) -> Result<i64, RuntimeError> {
        // 一、鉴权第一步：未注册的客户端连确认都不许发；顺带完成「确认也算心跳」。
        let pong_at_ms = self.refresh_heartbeat(client_id)?;
        // 二、鉴权第二步：只有占用该会话的客户端能决定这次调用。
        let Some(occupancy) = self.occupancy_of_client(client_id) else {
            return Err(RuntimeError::NoOccupancy);
        };
        let session_id = occupancy.session_id;
        // 三、待确认的调用必须仍挂在该会话里：重复确认与过期 `toolCallId` 都落固定文案。
        let pending = self
            .session()
            .pending_tool_calls_of_session(&session_id)
            .await
            .map_err(|error| internal(&error))?;
        let Some(pending) = pending.into_iter().find(|item| item.tool_call_id == tool_call_id) else {
            return Err(RuntimeError::PendingToolCallNotFound);
        };
        // 四、条件更新落库：并发两次确认只有一次影响 1 行，另一次按「已处理」拒绝。
        let resumed = self
            .session()
            .resume_run(&pending.run_id, tool_call_id, resume_decision(decision))
            .await
            .map_err(|error| internal(&error))?;
        if !resumed {
            return Err(RuntimeError::PendingToolCallNotFound);
        }
        // 五、唤醒会话队列：Run 已回 `QUEUED`，队列按 `created_at_ms` 重新取（D-P3-027）。
        if !self.notify_run(&session_id) {
            debug!(session_id, "会话已无运行时会话，确认只落库不唤醒");
        }
        Ok(pong_at_ms)
    }

    /// 重连补发：对该客户端占用会话下每个挂起 Run 补发一次 `tool_call_confirm_required`（D-P3-023）。
    ///
    /// 走执行方的同一条生产路径（运行时会话的生产方），`seq` 由写者连续分配；补发是尽力而为，
    /// 失败只记日志——补发不成功不该让「建立下行流」这条请求失败。
    pub async fn resend_confirms(&self, client_id: &str) {
        let Some(occupancy) = self.occupancy_of_client(client_id) else {
            return;
        };
        let session_id = occupancy.session_id;
        // 没有运行时会话就没有事件生产方：会话已收尾，挂起态也已由取消链路清掉。
        let Some(producer) = self.producer(&session_id) else {
            return;
        };
        let pending = match self.session().pending_tool_calls_of_session(&session_id).await {
            Ok(pending) => pending,
            Err(error) => {
                warn!(session_id, %error, "重连补发读取挂起调用失败");
                return;
            }
        };
        for item in pending {
            let event = match self
                .confirm_event(&session_id, &item.run_id, &item.tool_call_id, item.tool_round)
                .await
            {
                Some(event) => event,
                None => continue,
            };
            if producer.send(event).await.is_err() {
                warn!(session_id, "重连补发确认事件失败，写者已退出");
                return;
            }
        }
    }

    /// 按审计记录复原一条确认事件：工具名与脱敏参数都取自挂起时写入的 `PENDING` 记录。
    ///
    /// 记录缺失（理论上不可能：`PENDING` 是挂起的事实源）时返回空，调用方跳过该条。
    async fn confirm_event(
        &self,
        session_id: &str,
        run_id: &str,
        tool_call_id: &str,
        tool_round: u32,
    ) -> Option<RunEvent> {
        let records = match self.session().tool_calls_of_run(run_id).await {
            Ok(records) => records,
            Err(error) => {
                warn!(run_id, %error, "重连补发读取审计记录失败");
                return None;
            }
        };
        let record = records.iter().find(|record| record.tool_call_id == tool_call_id)?;
        Some(RunEvent {
            event: RunEventKind::ToolCallConfirmRequired,
            // 序号由写者覆写。
            seq: 0,
            run_id: run_id.to_string(),
            session_id: session_id.to_string(),
            created_at_ms: now_millis(),
            delta: None,
            message_id: None,
            status: None,
            error: None,
            input_tokens: None,
            output_tokens: None,
            tool_call_id: Some(tool_call_id.to_string()),
            tool_name: Some(record.tool_name.clone()),
            tool_arguments: Some(record.arguments.clone()),
            tool_status: None,
            tool_duration_ms: None,
            tool_round: Some(tool_round),
        })
    }
}

/// 契约里的决定 → 业务层的决定：两处取值一一对应，不存在第二套语义。
fn resume_decision(decision: ClientDecision) -> ResumeDecision {
    match decision {
        ClientDecision::Approve => ResumeDecision::Approve,
        ClientDecision::Reject => ResumeDecision::Reject,
    }
}

/// 业务错误转内部错误：细节只进日志，对外只给固定文案（D-P1.1-005 的口径）。
fn internal(error: &service_core::session::SessionError) -> RuntimeError {
    warn!(%error, "工具确认链路失败");
    RuntimeError::Internal(format!("{error}"))
}
