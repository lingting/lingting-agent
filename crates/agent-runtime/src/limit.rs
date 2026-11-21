//! 全局并发上限（P2-025）：先进入会话执行队列（已串行），再取全局许可。
//!
//! 同会话串行由 P1.1 的执行队列保证（D-P2-007 第 1 条），这里只补全局 `Semaphore`：
//! 超出 `agent.max_concurrent_runs` 时排队，超过 `agent.run_queue_timeout_ms` 即放弃执行。

use std::time::Duration;

use service_core::session::RunCancel;
use tokio::sync::{Semaphore, SemaphorePermit};

/// 取全局许可的结果。
pub(crate) enum Acquired<'a> {
    /// 已持有许可：整个 Run 期间都必须持有它（释放即代表 Run 结束）。
    Permit(SemaphorePermit<'a>),
    /// 排队超时：Run 落 `FAILED`（「执行排队超时」），不发起上游请求。
    Timeout,
    /// 排队期间被取消：不发终态事件之外的事件，也不落失败。
    Cancelled,
}

/// 取全局许可：等待期间同时受排队超时与取消信号约束。
pub(crate) async fn acquire<'a>(limit: &'a Semaphore, timeout_ms: u64, cancel: &RunCancel) -> Acquired<'a> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Acquired::Cancelled,
        acquired = tokio::time::timeout(Duration::from_millis(timeout_ms), limit.acquire()) => match acquired {
            Ok(Ok(permit)) => Acquired::Permit(permit),
            // 信号量已关闭（进程收尾中）与超时同义：都不再执行，按排队超时收尾。
            Ok(Err(_)) | Err(_) => Acquired::Timeout,
        },
    }
}
