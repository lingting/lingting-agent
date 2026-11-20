//! 运行时定时任务：心跳扫描、问询投递、超时标记与事件文件轮转（RT-010、RT-016）。
//!
//! 任务只做“判定 + 标记 + 投递”与“统计 + 删除”：真正的清理（反注册、停运行时会话、关下行通道）
//! 在断连处理链里；一次扫描先取快照再逐个判定，不持锁跨 `await`，也不阻塞事件写入。
//! 轮转的统计与删除放在阻塞线程池里，同一时刻只有一份统计。

use framework_proc_timer::timer;
use framework_timer::TimerResult;
use lib_core::lifecycle;
use lib_core::time::now_millis;
use tracing::{info, warn};
use types_core::runtime::ClientCloseReason;

use super::{ClientRecord, ClientState, RuntimeState, journal};

/// 轮转周期：启动后立即跑一次，之后按该周期执行（README 第六节）。
///
/// 宏参数只接受字面量，因此 `#[timer]` 上的 `60000` 必须与本常量保持一致。
pub const ROTATION_INTERVAL_MS: i64 = 60_000;
/// 轮转任务名：与 `#[timer]` 的函数名一致，`wake` 靠它按名唤醒。
const ROTATION_TIMER: &str = "event_rotation";

/// 唤醒一次轮转：每个运行时会话建立后调用，动作在定时任务的独立任务里执行。
pub(crate) fn wake_rotation() {
    framework_timer::wake(ROTATION_TIMER);
}

/// 扫描周期：超时客户端在 `CLIENT_TTL_MS` + 一个周期内被标记（README 第四节）。
///
/// 宏参数只接受字面量，因此 `#[timer]` 上的 `5000` 必须与本常量保持一致。
pub const SCAN_INTERVAL_MS: i64 = 5_000;

/// 扫描一次：先判心跳超时，再推进问询。
///
/// `now_ms` 与 `live` 都由调用方给：定时任务传当前时刻与 `!lifecycle::is_shutdown()`，
/// 用例传人造时刻即可验证 TTL 与问询窗口，不必真实等待。
/// `live == false`（进程正在关闭）时什么都不做：客户端已交给关闭流程处理，投递也会落进无人消费的通道。
pub fn scan(state: &RuntimeState, now_ms: i64, live: bool) {
    if !live {
        return;
    }
    for record in state.clients() {
        scan_client(state, &record, now_ms);
    }
}

/// 单个客户端的判定顺序：超时 → 问询过期 → 到时机投递问询。
fn scan_client(state: &RuntimeState, record: &ClientRecord, now_ms: i64) {
    // 已标记的客户端等处理链消费，重复判定只会反复命中同一状态。
    if record.state != ClientState::Connected {
        return;
    }
    if record.heartbeat_expired(now_ms) {
        state.mark(&record.client_id, ClientCloseReason::HeartbeatTimeout);
        return;
    }
    if record.inquiry_expired(now_ms) {
        state.mark(&record.client_id, ClientCloseReason::InquiryTimeout);
        return;
    }
    // 未到时机、已有未答问询或下行通道不可用时，`inquire` 自己会跳过。
    let _ = state.inquire(&record.client_id, now_ms);
}

/// 定时任务入口：每 [`SCAN_INTERVAL_MS`] 扫一次；运行时状态未注入时直接跳过。
#[timer(interval = 5000)]
async fn client_scan() -> TimerResult {
    let Some(state) = super::state() else {
        return TimerResult::success();
    };
    scan(state, now_millis(), !lifecycle::is_shutdown());
    TimerResult::success()
}

/// 轮转任务入口：统计 `data/events` 的会话目录与字节数，超限即最旧先删。
///
/// 统计与删除都在阻塞线程池里串行完成：不占异步 worker，也不阻塞事件写入。
/// 关闭流程置位后直接返回：客户端与运行时会话已交给断连处理链收尾。
#[timer(interval = 60000)]
async fn event_rotation() -> TimerResult {
    let Some(state) = super::state() else {
        return TimerResult::success();
    };
    if lifecycle::is_shutdown() {
        return TimerResult::success();
    }
    let config = state.journal_config();
    if config.unlimited() {
        return TimerResult::success();
    }
    let root = match state.events_root() {
        Ok(root) => root.to_path_buf(),
        Err(error) => {
            warn!(%error, "推导事件文件根目录失败，跳过轮转");
            return TimerResult::success();
        }
    };
    // 保护判据只有占用表快照：不去探测文件状态，避免与正在写入的会话竞态。
    let protected = state.occupied_sessions();
    match tokio::task::spawn_blocking(move || journal::rotate(&root, config, &protected)).await {
        Ok(Ok(summary)) if !summary.removed.is_empty() => info!(
            removed = summary.removed.len(),
            failed = summary.failed.len(),
            sessions = summary.sessions,
            bytes = summary.bytes,
            "事件文件轮转完成"
        ),
        Ok(Ok(summary)) if !summary.failed.is_empty() => {
            warn!(failed = summary.failed.len(), "事件文件轮转未能删除全部候选目录");
        }
        Ok(Ok(_)) => {}
        Ok(Err(error)) => warn!(%error, "统计事件文件目录失败，跳过轮转"),
        Err(error) => warn!(%error, "轮转任务执行失败"),
    }
    TimerResult::success()
}
