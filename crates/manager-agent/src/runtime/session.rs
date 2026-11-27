//! 运行时会话：事件生产方 → 写者 → 事件文件 + 下行通道（RT-015）。
//!
//! 写序固定为「先落盘 flush，再转发」：文件是事实源，转发失败（慢客户端、已断流）只让
//! 该客户端被判 `StreamClosed`，不影响已写入的内容，也不回滚。文件写入放在
//! `spawn_blocking` 里，不占用异步 worker。
//!
//! 终止由 [`SessionStop`] 表达：写者先把排队的事件写完，再追加一条终态事件并 flush，
//! 之后该文件不再追加（RT-003）。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use agent_core::session::{RunEvent, RunEventKind, RunStatus};
use lib_core::time::now_millis;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, warn};
use types_core::runtime::OccupancyVO;

use service_core::session::{RunDriver, RunStop, SessionService};

use crate::runner::cancel::RunCancels;
use crate::runner::queue::RunExecutor;

use super::RuntimeState;
use super::journal::Journal;
use super::metrics;
use super::registry::Downlink;

/// 生产方 → 写者的事件通道容量：够放下一个 Run 的增量，生产方不必为背压提前阻塞。
const EVENT_CAPACITY: usize = 64;
/// 停止信号通道容量：一次会话只会停一次。
const CONTROL_CAPACITY: usize = 1;

/// 取消原因：会话占用被释放。
pub const REASON_OCCUPANCY_RELEASED: &str = "会话占用已释放";
/// 取消原因：客户端连接已断开。
pub const REASON_CLIENT_DISCONNECTED: &str = "客户端连接已断开";
/// 终止原因：服务正在关闭。
pub const REASON_SERVER_SHUTDOWN: &str = "服务正在关闭";

/// 会话终止原因：决定追加哪条终态事件与固定原因文本（RT-003）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStop {
    /// 占用被释放（`api/occupancy/delete`）。
    OccupancyReleased,
    /// 客户端断连（断连处理链）。
    ClientDisconnected,
    /// 服务关闭。
    ServerShutdown,
}

/// 运行时会话句柄：生产方写入端 + 写者任务 + Run 执行队列。
pub struct RuntimeSession {
    /// 业务会话 ID。
    session_id: String,
    /// 本次占用对应的运行时会话 ID（也是事件文件名）。
    runtime_session_id: String,
    /// 事件文件路径。
    path: PathBuf,
    /// 事件生产方的写入端。
    producer: mpsc::Sender<RunEvent>,
    /// 停止信号：会话结束时由持有者发送。
    control: mpsc::Sender<SessionStop>,
    /// 写者任务。
    task: JoinHandle<()>,
    /// 会话业务服务：停止时取消未结束的 Run（含执行队列的中止）。
    session: Arc<SessionService>,
    /// Run 执行队列：会话被占用期间串行执行该会话的 `QUEUED` Run（RT-018）。
    executor: RunExecutor,
}
impl RuntimeSession {
    /// 启动运行时会话：打开事件文件并起写者任务。
    ///
    /// 文件打开失败时直接返回错误，由调用方（占用服务）撤回占用登记，不留半建状态。
    pub(crate) fn start(
        occupancy: &OccupancyVO,
        root: &Path,
        downlink: Downlink,
        session: Arc<SessionService>,
        driver: Option<Arc<dyn RunDriver>>,
        cancels: Arc<RunCancels>,
    ) -> io::Result<Self> {
        let journal = Journal::open(root, &occupancy.session_id, &occupancy.runtime_session_id)?;
        let path = journal.path().to_path_buf();
        let (producer, events) = mpsc::channel(EVENT_CAPACITY);
        let (control, controls) = mpsc::channel(CONTROL_CAPACITY);
        let task = tokio::spawn(writer(Arc::new(Mutex::new(journal)), events, controls, downlink));
        // 执行队列随运行时会话启动：占用一建立就跑存量 QUEUED Run（D-P1.1-004）。
        let executor = RunExecutor::spawn(
            occupancy.session_id.clone(),
            producer.clone(),
            Arc::clone(&session),
            driver,
            cancels,
        );
        Ok(Self {
            session_id: occupancy.session_id.clone(),
            runtime_session_id: occupancy.runtime_session_id.clone(),
            path,
            producer,
            control,
            task,
            session,
            executor,
        })
    }

    /// 业务会话 ID。
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 本次占用对应的运行时会话 ID。
    pub fn runtime_session_id(&self) -> &str {
        &self.runtime_session_id
    }

    /// 事件文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 事件生产方的写入端：交给 Run 执行方（RT-018 接入）。
    pub fn producer(&self) -> mpsc::Sender<RunEvent> {
        self.producer.clone()
    }

    /// 有新 Run 排队：唤醒该会话的执行队列（未被占用时没有执行队列）。
    pub(crate) fn notify_run(&self) {
        self.executor.notify();
    }

    /// 会话收到一条新消息：唤醒执行队列，并让它先恢复该会话的 `PAUSED` Run（D-P6-009）。
    ///
    /// 与 [`Self::notify_run`] 的区别只在恢复：确认唤醒只回填 `QUEUED` Run，不触发恢复。
    pub(crate) fn notify_message(&self) {
        self.executor.notify_message();
    }

    /// 停止会话：先把未结束的 Run 落成终态，再让执行队列收取消信号，最后追加终态事件并 flush。
    ///
    /// 「未结束」不含 `PAUSED`（P6A-012）：中断态没有执行者、也没有待补的终态事件，停止不处理它，
    /// 否则一次关闭或断连就会把可恢复的中断态永久判死。
    ///
    /// 顺序与 P1.1 不同（P2-024）：数据库先落终态，取消原因才能作为权威文本被执行方取到，
    /// 由执行方发出 `run_cancelled` / `server_shutdown`；写者的兜底补写只在执行方没来得及
    /// 发终态事件时生效（D-P2-029 第 3 条）。
    ///
    /// 写者已经自行退出（生产方先收尾）时返回成功：文件已 flush，没有需要补写的终态事件。
    pub async fn stop(self, reason: SessionStop) -> io::Result<()> {
        let Self {
            session_id,
            producer,
            control,
            task,
            session,
            executor,
            ..
        } = self;
        // 四个阶段分别计时：会话锁持有时间必须与收尾时长解耦，指标是唯一证据（P2.1-003）。
        let started = Instant::now();
        // 一、数据库先落终态：条件更新影响 0 行说明 Run 已终态，属正常路径。
        let database_started = Instant::now();
        let cancelled = match session.cancel_session_runs(&session_id, reason.text()).await {
            Ok(cancelled) => cancelled,
            Err(error) => {
                warn!(session_id, %error, "停止运行时会话时取消未结束的 Run 失败");
                0
            }
        };
        let database_elapsed = database_started.elapsed();
        if cancelled > 0 {
            debug!(session_id, cancelled, "停止运行时会话，已取消未结束的 Run");
        }
        // 二、执行队列收取消信号并退出：在跑 Run 中断模型流后自己发终态事件。
        let executor_started = Instant::now();
        executor.stop(reason.run_stop()).await;
        let executor_elapsed = executor_started.elapsed();
        // 三、先丢弃生产方写入端：写者把队列里剩下的事件写完再收尾，顺序不会乱。
        drop(producer);
        let writer_started = Instant::now();
        let written: io::Result<()> = if control.send(reason).await.is_err() {
            let _ = task.await;
            Ok(())
        } else {
            drop(control);
            task.await
                .map_err(|error| io::Error::other(format!("写者任务异常退出：{error}")))
        };
        metrics::record_stop(
            started.elapsed(),
            database_elapsed,
            executor_elapsed,
            writer_started.elapsed(),
        );
        written
    }
}

impl SessionStop {
    /// 本次停止在数据库里的原因文本：写进 `runs.error`，也是执行方事件里的原因（D-P2-029）。
    pub fn text(self) -> &'static str {
        match self {
            Self::OccupancyReleased => REASON_OCCUPANCY_RELEASED,
            Self::ClientDisconnected => REASON_CLIENT_DISCONNECTED,
            Self::ServerShutdown => REASON_SERVER_SHUTDOWN,
        }
    }

    /// 本次停止对应的执行方停止类别：只有服务关闭落 `server_shutdown`。
    pub fn run_stop(self) -> RunStop {
        match self {
            Self::ServerShutdown => RunStop::Shutdown,
            Self::OccupancyReleased | Self::ClientDisconnected => RunStop::Cancelled,
        }
    }
}

/// 写者任务：写文件失败时停止会话并标记占用客户端 `JournalError`，清理交给断连处理链。
async fn writer(
    journal: Arc<Mutex<Journal>>,
    events: mpsc::Receiver<RunEvent>,
    controls: mpsc::Receiver<SessionStop>,
    downlink: Downlink,
) {
    if let Err(error) = write_all(&journal, events, controls, &downlink).await {
        warn!(%error, "事件文件写入失败，停止运行时会话");
        downlink.mark_journal_error();
    }
}

/// 写者主流程：事件按到达顺序写盘并转发，收到停止信号后补终态事件。
async fn write_all(
    journal: &Arc<Mutex<Journal>>,
    mut events: mpsc::Receiver<RunEvent>,
    mut controls: mpsc::Receiver<SessionStop>,
    downlink: &Downlink,
) -> io::Result<()> {
    let mut tail = None;
    let mut stop = None;
    // 生产方与停止端各自独立：一次 Run 的生产方结束不代表会话结束，会话活到占用被释放。
    let mut feeding = true;

    'write: loop {
        if feeding {
            tokio::select! {
                // 事件优先：停止信号不插队，已到达的事件先落盘。
                biased;
                received = events.recv() => match received {
                    Some(event) => {
                        tail = Some(write_event(journal, event, downlink).await?);
                        continue 'write;
                    }
                    // 这一轮 Run 的生产方收尾了（终态事件由它自己写），会话继续等停止信号。
                    None => {
                        feeding = false;
                        continue 'write;
                    }
                },
                reason = controls.recv() => {
                    stop = reason;
                    break 'write;
                }
            }
        }
        // 停止端也关闭（会话句柄被丢弃而没走 stop）：flush 后退出，不补终态事件。
        match controls.recv().await {
            Some(reason) => {
                stop = Some(reason);
                break 'write;
            }
            None => break 'write,
        }
    }

    // 停止后不再等新事件，但队列里已有的必须写完：文件顺序等于生产顺序。
    while let Ok(event) = events.try_recv() {
        tail = Some(write_event(journal, event, downlink).await?);
    }

    // 只在最后一个 Run 还没抵终态时补终态事件；没有事件时不必凭空造一条。
    if let Some(reason) = stop
        && let Some(tail) = tail
        && !tail.terminal
    {
        write_event(
            journal,
            terminal_event(&tail.run_id, &tail.session_id, reason),
            downlink,
        )
        .await?;
    }

    // 收尾 flush 放到阻塞线程里：与逐条写入同一把锁，顺序不变。
    let journal = Arc::clone(journal);
    tokio::task::spawn_blocking(move || lock(&journal).flush())
        .await
        .map_err(|error| io::Error::other(format!("事件文件 flush 任务失败：{error}")))?
}

/// 落盘一条事件（阻塞写入放阻塞线程池），成功后再转发。
async fn write_event(journal: &Arc<Mutex<Journal>>, event: RunEvent, downlink: &Downlink) -> io::Result<EventTail> {
    let journal = Arc::clone(journal);
    let written = tokio::task::spawn_blocking(move || lock(&journal).append(event))
        .await
        .map_err(|error| io::Error::other(format!("事件写入任务失败：{error}")))??;
    downlink.forward(&written);
    Ok(EventTail {
        run_id: written.run_id,
        session_id: written.session_id,
        terminal: matches!(
            written.event,
            RunEventKind::RunCompleted
                | RunEventKind::RunFailed
                | RunEventKind::RunCancelled
                | RunEventKind::ServerShutdown
        ),
    })
}

/// 最后一个 Run 的归属与是否已收尾。
struct EventTail {
    run_id: String,
    session_id: String,
    terminal: bool,
}

/// 会话被外部停止时的终态事件：取消落 `run_cancelled`，关闭落 `server_shutdown`。
fn terminal_event(run_id: &str, session_id: &str, reason: SessionStop) -> RunEvent {
    let (event, error) = match reason {
        SessionStop::OccupancyReleased => (RunEventKind::RunCancelled, REASON_OCCUPANCY_RELEASED),
        SessionStop::ClientDisconnected => (RunEventKind::RunCancelled, REASON_CLIENT_DISCONNECTED),
        SessionStop::ServerShutdown => (RunEventKind::ServerShutdown, REASON_SERVER_SHUTDOWN),
    };
    RunEvent {
        event,
        // 序号由写者覆写，这里只是占位。
        seq: 0,
        run_id: run_id.to_string(),
        session_id: session_id.to_string(),
        created_at_ms: now_millis(),
        delta: None,
        message_id: None,
        status: Some(RunStatus::Cancelled),
        error: Some(error.to_string()),
        input_tokens: None,
        output_tokens: None,
        tool_call_id: None,
        tool_name: None,
        tool_arguments: None,
        tool_status: None,
        tool_duration_ms: None,
        tool_round: None,
    }
}

/// 取文件锁；锁中毒时沿用内部数据（写者只有一个，中毒只可能来自 panic）。
fn lock(journal: &Arc<Mutex<Journal>>) -> std::sync::MutexGuard<'_, Journal> {
    journal.lock().unwrap_or_else(|error| error.into_inner())
}

impl RuntimeState {
    /// 启动并登记一条运行时会话；已有同会话的会话时先停止它（不应发生，防御性处理）。
    pub(crate) async fn start_session(&self, occupancy: &OccupancyVO) -> Result<(), io::Error> {
        let downlink = self.downlink(&occupancy.client_id);
        let driver = self.driver();
        let runtime_session = RuntimeSession::start(
            occupancy,
            self.events_root()?,
            downlink,
            Arc::clone(self.session()),
            driver,
            Arc::clone(self.cancels()),
        )?;
        if let Some(previous) = self.take_session(&occupancy.session_id) {
            warn!(session_id = %occupancy.session_id, "同一会话存在未停止的运行时会话，先停止旧会话");
            if let Err(error) = previous.stop(SessionStop::OccupancyReleased).await {
                warn!(%error, "停止旧运行时会话失败");
            }
        }
        self.insert_session(runtime_session);
        // 每个运行时会话建立后触发一次轮转：统计与删除在定时任务的独立任务里做，不阻塞事件写入。
        super::timer::wake_rotation();
        Ok(())
    }
}
