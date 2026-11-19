//! 运行时定时任务：心跳扫描、问询投递与超时标记（RT-010）。
//!
//! 任务只做“判定 + 标记 + 投递”：真正的清理（反注册、停运行时会话、关下行通道）在断连处理链里；
//! 一次扫描先取客户端快照再逐个判定，不持锁跨 `await`，也不阻塞事件写入。

use framework_proc_timer::timer;
use framework_timer::TimerResult;
use lib_core::lifecycle;
use lib_core::time::now_millis;
use types_core::runtime::ClientCloseReason;

use super::{ClientRecord, ClientState, RuntimeState};

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
