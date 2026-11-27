//! Run 触发点：会话被占用时执行该会话的 `QUEUED` Run（RT-018、D-P1.1-004）。
//!
//! **先恢复后执行**：会话收到一条新消息时，先拾取该会话全部 `PAUSED` Run（`updated_at_ms` 升序），
//! 再执行 `QUEUED` Run；只占用不发消息不触发恢复（P6A-011 / D-P6-009）。
//!
//! **一条规则**：Run 只有在所属会话当前被占用时才会被执行；执行者始终是该会话的运行时会话。
//! 因此执行队列随运行时会话创建而启动、随其停止而结束，事件一律写进运行时会话的生产方，
//! 由写者先落盘再转发（RT-015）。
//!
//! 串行保证：同一会话同一时刻最多一个 `RUNNING` Run，队列实现是一条 `mpsc` 串行任务；
//! `QUEUED → RUNNING` 的推进是数据库里的条件更新（`WHERE status='QUEUED'`），不并发推进多个 Run。
//!
//! 执行内容由端口 [`RunDriver`] 提供（P2-023）：队列只负责取 Run、取提示词、登记取消信号，
//! 终态写入与终态事件都在执行方完成；未装配端口时 Run 直接落 `FAILED`（D-P2-028 第 3 条）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use agent_core::session::{ResumePoint, Run, RunEvent, RunEventKind, RunStatus};
use lib_core::time::now_millis;
use service_core::session::{RunDriver, RunInput, RunOutcome, RunStop, SessionService};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::runner::cancel::RunCancels;
use crate::runner::resume;

/// 唤醒通道容量：一次唤醒足以让执行任务重扫队列，多余的通知可以丢掉。
const WAKE_CAPACITY: usize = 1;
/// 停止信号通道容量：一条执行队列只会停一次。
const STOP_CAPACITY: usize = 1;
/// 停止时等待在跑 Run 收尾的上限：执行方要落库并写终态事件，给它一个界限，超时即中止任务。
const STOP_TIMEOUT_MS: u64 = 5_000;
/// 未装配执行端口时的失败原因（对外可见）。
const REASON_NOT_ASSEMBLED: &str = "执行器未装配";
/// 取消原因：Run 找不到对应的用户消息（数据不完整，不发起执行）。
const REASON_PROMPT_MISSING: &str = "找不到对应的用户消息";

/// 会话执行队列句柄：由运行时会话持有，会话停止即结束。
pub(crate) struct RunExecutor {
    /// 所属会话：停止时按它批量取消在跑 Run。
    session_id: String,
    /// 唤醒端：有新 Run 排队时让执行任务重扫队列。
    wake: mpsc::Sender<()>,
    /// 恢复待处理：会话收到新消息时置位，执行任务据此先拾取该会话的 `PAUSED` Run（D-P6-009）。
    resume_pending: Arc<AtomicBool>,
    /// 停止端：让执行任务在跑完当前 Run 后退出。
    stop: mpsc::Sender<()>,
    /// 在跑 Run 的取消信号表：与运行时状态共享同一份。
    cancels: Arc<RunCancels>,
    /// 执行任务。
    task: JoinHandle<()>,
}

impl RunExecutor {
    /// 启动某个会话的执行队列：事件写进运行时会话的生产方。
    ///
    /// 启动后立即尝试执行存量 `QUEUED` Run（`QUEUED` 按 `created_at_ms` 升序），
    /// 因此 `occupancy/create` 之后不需要额外的唤醒。
    /// 存量 `PAUSED` Run 不在这里拾取：恢复只由「会话已被占用 ∧ 收到一条新消息」触发（D-P6-009）。
    pub(crate) fn spawn(
        session_id: String,
        producer: mpsc::Sender<RunEvent>,
        service: Arc<SessionService>,
        driver: Option<Arc<dyn RunDriver>>,
        cancels: Arc<RunCancels>,
    ) -> Self {
        let (wake, wakeups) = mpsc::channel(WAKE_CAPACITY);
        let (stop, stops) = mpsc::channel(STOP_CAPACITY);
        // 恢复待处理标记与唤醒通道分开：唤醒可能被丢弃（容量 1），标记不会。
        let resume_pending = Arc::new(AtomicBool::new(false));
        let task = tokio::spawn(drain(
            session_id.clone(),
            producer,
            wakeups,
            stops,
            Arc::clone(&resume_pending),
            service,
            driver,
            Arc::clone(&cancels),
        ));
        Self {
            session_id,
            wake,
            resume_pending,
            stop,
            cancels,
            task,
        }
    }

    /// 有新 Run 排队：队列空时唤醒执行任务；已有待处理通知时忽略（一次唤醒足够重扫）。
    pub(crate) fn notify(&self) {
        let _ = self.wake.try_send(());
    }

    /// 会话收到一条新消息：置「恢复待处理」并唤醒队列（D-P6-009 的唯一恢复触发点）。
    ///
    /// 标记与唤醒分开：唤醒通道容量为 1，连续两条消息的唤醒会被合并丢弃，标记不会。
    pub(crate) fn notify_message(&self) {
        self.resume_pending.store(true, Ordering::SeqCst);
        self.notify();
    }

    /// 停止执行队列：先给在跑 Run 收取消信号，再等它收尾退出。
    ///
    /// 停止方（运行时会话）已经把数据库里的终态写好；这里负责让执行方中断模型流并发出终态事件，
    /// 然后让队列退出。超时视为执行方无响应，中止任务兜底。
    pub(crate) async fn stop(self, stop: RunStop) {
        let cancelled = self.cancels.cancel_session(&self.session_id, stop);
        if cancelled > 0 {
            info!(session_id = %self.session_id, cancelled, "已取消在跑的 Run，等待执行队列收尾");
        }
        let _ = self.stop.try_send(());
        match tokio::time::timeout(std::time::Duration::from_millis(STOP_TIMEOUT_MS), self.task).await {
            Ok(_) => {}
            Err(_) => warn!(session_id = %self.session_id, "执行队列在超时内未收尾，已中止任务"),
        }
    }
}

/// 执行队列主循环：先按消息触发恢复该会话的 `PAUSED` Run，再逐个跑 `QUEUED` Run；
/// 都空了就等下一次唤醒或停止信号。
///
/// 顺序是这一子阶段的判据之一（D-P6-009）：同一批里被恢复的 Run 必须早于本次消息产生的
/// `QUEUED` Run，因此恢复段在 `QUEUED` 段之前，且两段都在同一个串行任务里。
#[allow(clippy::too_many_arguments)]
async fn drain(
    session_id: String,
    producer: mpsc::Sender<RunEvent>,
    mut wakeups: mpsc::Receiver<()>,
    mut stops: mpsc::Receiver<()>,
    resume_pending: Arc<AtomicBool>,
    service: Arc<SessionService>,
    driver: Option<Arc<dyn RunDriver>>,
    cancels: Arc<RunCancels>,
) {
    loop {
        // 一、会话收到新消息：先恢复该会话全部 `PAUSED` Run（`updated_at_ms` 升序），再执行 `QUEUED`。
        //     `queued_run` 取最早的一条，因此本次消息产生的新 Run 一定排在恢复段之后（D-P6-009）。
        if resume_pending.swap(false, Ordering::SeqCst)
            && resume::paused(&service, driver.as_ref(), &cancels, &session_id, &mut stops, &producer).await
        {
            return;
        }
        // 二、串行执行该会话的 `QUEUED` Run。
        loop {
            let run = match service.next_queued_run(&session_id).await {
                Ok(Some(run)) => run,
                Ok(None) => break,
                Err(error) => {
                    warn!(session_id, %error, "取出待执行 Run 失败，等待下一次唤醒");
                    break;
                }
            };
            run_one(&service, driver.as_ref(), &cancels, run, None, &producer).await;
            // 生产方关闭说明运行时会话已经在收尾，再跑下一个也没有意义。
            if producer.is_closed() {
                return;
            }
            // 停止信号到达：剩余 Run 已由停止方落成终态，这里不再取下一个。
            if stops.try_recv().is_ok() {
                return;
            }
        }
        // 队列空：等下一次唤醒或停止；唤醒端被丢弃（会话句柄释放）时任务随之退出。
        tokio::select! {
            received = wakeups.recv() => {
                if received.is_none() {
                    return;
                }
            }
            _ = stops.recv() => return,
        }
    }
}

/// 执行一个已推进到 `RUNNING` 的 Run：取提示词，交给执行端口。
///
/// `resume` 由恢复段（[`resume::paused`]）判定后给出；`None` 表示按正常 `QUEUED → RUNNING` 执行。
/// 提示词一律取该 Run 自己那条用户消息，恢复不使用触发它的新消息正文（D-P6-009）。
pub(super) async fn run_one(
    service: &SessionService,
    driver: Option<&Arc<dyn RunDriver>>,
    cancels: &RunCancels,
    run: Run,
    resume: Option<ResumePoint>,
    producer: &mpsc::Sender<RunEvent>,
) {
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

    // 未装配执行端口：Run 直接落 FAILED 并留一条终态事件，原因可读（D-P2-028 第 3 条）。
    let Some(driver) = driver else {
        warn!(run_id = %run.id, "执行端口未装配，Run 直接落 FAILED");
        fail(service, &run, REASON_NOT_ASSEMBLED, producer).await;
        return;
    };

    let run_id = run.id.clone();
    let cancel = cancels.register(&run.session_id, &run_id);
    let outcome = driver
        .drive(RunInput {
            run,
            prompt,
            resume,
            events: producer.clone(),
            cancel,
        })
        .await;
    cancels.finish(&run_id);
    log_outcome(&run_id, outcome);
}

/// 执行端口的结局只影响日志：终态写入与终态事件都已由执行方完成。
fn log_outcome(run_id: &str, outcome: RunOutcome) {
    match outcome {
        RunOutcome::Completed => info!(run_id, "Run 执行完成"),
        RunOutcome::Failed { public_error } => info!(run_id, %public_error, "Run 执行失败"),
        RunOutcome::Cancelled { reason } => info!(run_id, %reason, "Run 已取消"),
        RunOutcome::Suspended { tool_call_id } => {
            info!(run_id, %tool_call_id, "Run 挂起等待工具确认，让出并发位");
        }
        RunOutcome::SessionGone => info!(run_id, "运行时会话已停止，Run 未提交终态"),
    }
}

/// 直接落 `FAILED`：写库后补一条 `run_failed` 事件（写库失败时只记日志）。
///
/// 两个调用点：未装配执行端口（D-P2-028 第 3 条）与恢复判定为不可恢复（D-P6-007）。
/// 两者都只写终态与固定文案，不写助手消息、不重跑任何调用。
pub(super) async fn fail(service: &SessionService, run: &Run, reason: &str, producer: &mpsc::Sender<RunEvent>) {
    match service.fail_run(&run.id, reason).await {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            warn!(run_id = %run.id, %error, "标记 Run 失败时写库失败");
            return;
        }
    }
    let event = RunEvent {
        event: RunEventKind::RunFailed,
        // 序号由写者覆写。
        seq: 0,
        run_id: run.id.clone(),
        session_id: run.session_id.clone(),
        created_at_ms: now_millis(),
        delta: None,
        message_id: None,
        status: Some(RunStatus::Failed),
        error: Some(reason.to_string()),
        input_tokens: None,
        output_tokens: None,
        tool_call_id: None,
        tool_name: None,
        tool_arguments: None,
        tool_status: None,
        tool_duration_ms: None,
        tool_round: None,
    };
    let _ = producer.send(event).await;
}

/// 执行前校验失败：把 Run 落成 `CANCELLED`，失败只记日志（Run 已终态时更新 0 行属正常）。
async fn cancel(service: &SessionService, run_id: &str, reason: &str) {
    if let Err(error) = service.cancel_run_quietly(run_id, reason).await {
        warn!(run_id, %error, "取消 Run 失败");
    }
}
