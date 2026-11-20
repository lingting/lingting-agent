//! Run 触发点：会话被占用时执行该会话的 `QUEUED` Run（RT-018、D-P1.1-004）。
//!
//! **一条规则**：Run 只有在所属会话当前被占用时才会被执行；执行者始终是该会话的运行时会话。
//! 因此执行队列随运行时会话创建而启动、随其停止而结束，事件一律写进运行时会话的生产方，
//! 由写者先落盘再转发（RT-015）。
//!
//! 串行保证：同一会话同一时刻最多一个 `RUNNING` Run，队列实现是一条 `mpsc` 串行任务；
//! `QUEUED → RUNNING` 的推进是数据库里的条件更新（`WHERE status='QUEUED'`），不并发推进多个 Run。

use std::sync::Arc;

use agent_core::session::{Run, RunEvent};
use lib_core::id;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::warn;

use service_core::session::SessionService;

use crate::runner::mock;

/// 唤醒通道容量：一次唤醒足以让执行任务重扫队列，多余的通知可以丢掉。
const WAKE_CAPACITY: usize = 1;
/// 取消原因：Run 找不到对应的用户消息（数据不完整，不发起执行）。
const REASON_PROMPT_MISSING: &str = "找不到对应的用户消息";

/// 会话执行队列句柄：由运行时会话持有，会话停止即结束。
pub(crate) struct RunExecutor {
    /// 唤醒端：有新 Run 排队时让执行任务重扫队列。
    wake: mpsc::Sender<()>,
    /// 执行任务。
    task: JoinHandle<()>,
}

impl RunExecutor {
    /// 启动某个会话的执行队列：事件写进运行时会话的生产方。
    ///
    /// 启动后立即尝试执行存量 `QUEUED` Run（`QUEUED` 按 `created_at_ms` 升序），
    /// 因此 `occupancy/create` 之后不需要额外的唤醒。
    pub(crate) fn spawn(session_id: String, producer: mpsc::Sender<RunEvent>, service: Arc<SessionService>) -> Self {
        let (wake, wakeups) = mpsc::channel(WAKE_CAPACITY);
        let task = tokio::spawn(drain(session_id, producer, wakeups, service));
        Self { wake, task }
    }

    /// 有新 Run 排队：队列空时唤醒执行任务；已有待处理通知时忽略（一次唤醒足够重扫）。
    pub(crate) fn notify(&self) {
        let _ = self.wake.try_send(());
    }

    /// 停止执行队列：中止在跑的生产任务并等它退出，之后才允许写者收尾。
    ///
    /// 被中止的 Run 由会话停止流程统一取消（`QUEUED` / `RUNNING` 一律 `CANCELLED`）。
    pub(crate) async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}

/// 执行队列主循环：有 `QUEUED` Run 就逐个跑，空了就等下一次唤醒。
async fn drain(
    session_id: String,
    producer: mpsc::Sender<RunEvent>,
    mut wakeups: mpsc::Receiver<()>,
    service: Arc<SessionService>,
) {
    loop {
        loop {
            let run = match service.start_next_run(&session_id).await {
                Ok(Some(run)) => run,
                Ok(None) => break,
                Err(error) => {
                    warn!(session_id, %error, "取出待执行 Run 失败，等待下一次唤醒");
                    break;
                }
            };
            run_one(&service, run, &producer).await;
            // 生产方关闭说明运行时会话已经在收尾，再跑下一个也没有意义。
            if producer.is_closed() {
                return;
            }
        }
        // 队列空：等下一次唤醒；占用被释放时唤醒端被丢弃，任务随之退出。
        if wakeups.recv().await.is_none() {
            return;
        }
    }
}

/// 执行一个已推进到 `RUNNING` 的 Run；执行前的校验失败就把它落成已取消。
async fn run_one(service: &SessionService, run: Run, producer: &mpsc::Sender<RunEvent>) {
    let prompt = match service.user_prompt(&run.session_id, &run.id).await {
        Ok(Some(prompt)) => prompt,
        Ok(None) => {
            warn!(run_id = %run.id, session_id = %run.session_id, "Run 找不到对应的用户消息，已取消");
            cancel(service, &run.id, REASON_PROMPT_MISSING).await;
            return;
        }
        Err(error) => {
            warn!(run_id = %run.id, %error, "读取用户消息失败，已取消该 Run");
            cancel(service, &run.id, REASON_PROMPT_MISSING).await;
            return;
        }
    };
    let assistant_message_id = match id::next_id() {
        Ok(id) => id,
        Err(error) => {
            warn!(run_id = %run.id, %error, "生成助手消息 ID 失败，已取消该 Run");
            cancel(service, &run.id, REASON_PROMPT_MISSING).await;
            return;
        }
    };
    mock::produce(service, run, prompt, assistant_message_id, producer).await;
}

/// 执行前校验失败：把 Run 落成 `CANCELLED`，失败只记日志（Run 已终态时更新 0 行属正常）。
async fn cancel(service: &SessionService, run_id: &str, reason: &str) {
    if let Err(error) = service.cancel_run_quietly(run_id, reason).await {
        warn!(run_id, %error, "取消 Run 失败");
    }
}
