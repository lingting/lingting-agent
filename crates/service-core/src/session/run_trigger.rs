//! Run 触发点：会话被占用时执行该会话的 `QUEUED` Run（RT-018、D-P1.1-004）。
//!
//! **一条规则**：Run 只有在所属会话当前被占用时才会被执行；执行者始终是该会话的运行时会话。
//! 因此执行队列随运行时会话创建而启动、随其停止而结束，事件一律写进运行时会话的生产方，
//! 由写者先落盘再转发（RT-015）。
//!
//! 串行保证：同一会话同一时刻最多一个 `RUNNING` Run，队列实现是一条 `mpsc` 串行任务；
//! `QUEUED → RUNNING` 的推进在同一把存储锁内完成，不并发推进多个 Run。

use std::sync::Arc;

use agent_core::session::{Run, RunEvent};
use lib_core::{id, time};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::warn;

use crate::holder;
use crate::session::store::Store;
use crate::session::stream;

/// 唤醒通道容量：一次唤醒足以让执行任务重扫队列，多余的通知可以丢掉。
const WAKE_CAPACITY: usize = 1;

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
    pub(crate) fn spawn(session_id: String, producer: mpsc::Sender<RunEvent>) -> Self {
        let (wake, wakeups) = mpsc::channel(WAKE_CAPACITY);
        let task = tokio::spawn(drain(session_id, producer, wakeups));
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
async fn drain(session_id: String, producer: mpsc::Sender<RunEvent>, mut wakeups: mpsc::Receiver<()>) {
    let store = holder::session().store();
    loop {
        while let Some(run) = store.start_next_run(&session_id, time::now_millis()) {
            run_one(&store, run, &producer).await;
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
async fn run_one(store: &Arc<Store>, run: Run, producer: &mpsc::Sender<RunEvent>) {
    let Some(prompt) = store.user_prompt(&run.id, &run.session_id) else {
        warn!(run_id = %run.id, session_id = %run.session_id, "Run 找不到对应的用户消息，已取消");
        store.cancel_run(&run.id, time::now_millis());
        return;
    };
    let assistant_message_id = match id::next_id() {
        Ok(id) => id,
        Err(error) => {
            warn!(run_id = %run.id, %error, "生成助手消息 ID 失败，已取消该 Run");
            store.cancel_run(&run.id, time::now_millis());
            return;
        }
    };
    stream::produce(store, run, prompt, assistant_message_id, producer).await;
}
