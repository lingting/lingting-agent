//! 全局并发上限（P2-025）：先进入会话执行队列（已串行），再取全局许可。
//!
//! 同会话串行由 P1.1 的执行队列保证（D-P2-007 第 1 条），这里只补全局 `Semaphore`：
//! 超出 `agent.max_concurrent_runs` 时排队，超过 `agent.run_queue_timeout_ms` 即放弃执行。
//!
//! 排队时长进 [`crate::metrics`]：它是「全局许可是不是瓶颈」的唯一直接证据（P2.1-003）。
//! 数据只经指标快照读出去，这里只计时与推进水位（阈值日志已删除，P2.2-004）。

use std::time::{Duration, Instant};

use service_core::session::RunCancel;
use tokio::sync::{Semaphore, SemaphorePermit};

use crate::metrics;

/// 取全局许可的结果。
pub(crate) enum Acquired<'a> {
    /// 已持有许可：整个 Run 期间都必须持有它（释放即代表 Run 结束）。
    Permit(RunPermit<'a>),
    /// 排队超时：Run 落 `FAILED`（「执行排队超时」），不发起上游请求。
    Timeout,
    /// 排队期间被取消：不发终态事件之外的事件，也不落失败。
    Cancelled,
}

/// 全局许可的持有者：释放时把「正在执行」水位减一。
///
/// 单独包一层是因为 `SemaphorePermit` 没有可挂载的析构点，而「同时在跑多少个 Run」
/// 必须在许可真正释放的瞬间回落，否则水位会虚高。
pub(crate) struct RunPermit<'a> {
    /// 真正的信号量许可：随本结构一起释放。
    _permit: SemaphorePermit<'a>,
}

impl Drop for RunPermit<'_> {
    fn drop(&mut self) {
        metrics::run_finished();
    }
}

/// 取全局许可：等待期间同时受排队超时与取消信号约束。
pub(crate) async fn acquire<'a>(limit: &'a Semaphore, timeout_ms: u64, cancel: &RunCancel) -> Acquired<'a> {
    metrics::queued_enter();
    let started = Instant::now();
    let acquired = tokio::select! {
        biased;
        () = cancel.cancelled() => Acquired::Cancelled,
        acquired = tokio::time::timeout(Duration::from_millis(timeout_ms), limit.acquire()) => match acquired {
            Ok(Ok(permit)) => {
                metrics::run_started();
                Acquired::Permit(RunPermit { _permit: permit })
            }
            // 信号量已关闭（进程收尾中）与超时同义：都不再执行，按排队超时收尾。
            Ok(Err(_)) | Err(_) => Acquired::Timeout,
        },
    };
    metrics::queued_leave();
    match &acquired {
        Acquired::Permit(_) => metrics::record_acquired_wait(started.elapsed()),
        Acquired::Timeout => metrics::record_timeout(),
        Acquired::Cancelled => metrics::record_cancelled(),
    }
    acquired
}
