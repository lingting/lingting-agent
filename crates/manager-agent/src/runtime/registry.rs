//! 运行时状态注册表：客户端注册表 + 占用表 + 标记集合 + 下行通道 + 运行时会话。
//!
//! 并发规则见 `README.md` 第五节：所有读写都在同一把 `RwLock` 内完成、**不持锁跨 `await`**，
//! 需要 `await` 的动作（打开事件文件、向下行通道发送、总线通知）一律先取快照再放锁。
//! 本模块只做内存记账，不感知传输，也不依赖任何 Web 框架。

use agent_core::session::RunEvent;
use lib_core::time::now_millis;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Instant;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, mpsc};
use types_core::runtime::{ClientCloseReason, ClientEvent, ClientEventKind, OccupancyVO};

use super::RuntimeError;
use super::client::{ClientChannel, ClientRecord, ClientState, PendingInquiry};
use super::journal::{self, JournalConfig};
use super::metrics;
use super::session::{RuntimeSession, SessionStop};
use crate::runner::cancel::RunCancels;
use service_core::session::{RunDriver, RunStop, SessionService};
use tracing::{debug, warn};

/// 后台收尾的上限：执行队列自带 5s 上限，这里再给写库取消与写者 flush 留出余量。
const FINALIZE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// 进程内运行时状态：由装配层在启动时构造并持有。
pub struct RuntimeState {
    /// 会话业务服务：占用校验、执行队列与停止时的 Run 取消都经它访问存储。
    session: Arc<SessionService>,
    /// 用 `Arc` 包裹，让下行流的 watcher 能脱离 `&self` 存活（`tokio::spawn` 要求 `'static`）。
    inner: Arc<RwLock<Registry>>,
    /// 事件文件根目录：显式注入（用例的临时目录）或首次使用时按应用目录推导并缓存。
    events_root: OnceLock<PathBuf>,
    /// 事件文件轮转配置：启动时注入一次，运行期不变（不做热更新）。
    journal: JournalConfig,
    /// Run 执行端口：由启动方注入一次（D-P2-028 第 3 条），未注入时 Run 直接落 `FAILED`。
    driver: OnceLock<Arc<dyn RunDriver>>,
    /// 在跑 Run 的取消信号表：`api/run/cancel` 与会话停止都经它触发（D-P2-010）。
    cancels: Arc<RunCancels>,
    /// 后台收尾登记（P2.1-006）：同一会话的新占用先等上一次收尾完成，关闭流程等全部收尾完成。
    finalizing: Arc<Finalizer>,
}

/// 注册表的全部内存数据，读写都走 `RuntimeState::inner` 这一把锁。
#[derive(Default)]
struct Registry {
    /// 客户端记录，按客户端 ID 索引。
    clients: HashMap<String, ClientRecord>,
    /// 占用关系，按客户端 ID 索引（一对一约束由占用服务保证）。
    occupancy_by_client: HashMap<String, OccupancyVO>,
    /// 占用关系，按会话 ID 索引。
    occupancy_by_session: HashMap<String, OccupancyVO>,
    /// 已判定断连但尚未被处理链消费的客户端，按客户端 ID 索引并保留第一次的原因。
    marks: HashMap<String, ClientCloseReason>,
    /// 各客户端当前的下行通道句柄；反注册时一并丢弃。
    channels: HashMap<String, Arc<ClientChannel>>,
    /// 下行通道代号发号器：每开一条流换一个代号，旧 watcher 因此不会误标记新流。
    stream_generation: u64,
    /// 运行中的运行时会话，按会话 ID 索引（一个会话同时最多一个）。
    sessions: HashMap<String, RuntimeSession>,
    /// 会话级锁：占用登记与运行时会话的建立 / 停止共用同一粒度（`README.md` 第五节规则 4）。
    session_locks: HashMap<String, Arc<AsyncMutex<()>>>,
}

impl RuntimeState {
    /// 创建运行时状态；事件文件默认落在应用数据目录下，需要隔离时用 [`Self::with_events_root`]。
    pub fn new(session: Arc<SessionService>) -> Self {
        Self {
            session,
            inner: Arc::new(RwLock::new(Registry::default())),
            events_root: OnceLock::new(),
            journal: JournalConfig::default(),
            driver: OnceLock::new(),
            cancels: Arc::new(RunCancels::default()),
            finalizing: Arc::new(Finalizer::default()),
        }
    }

    /// 换用指定的事件文件根目录（用例注入临时目录，避免写到真实应用目录）。
    pub fn with_events_root(self, root: impl Into<PathBuf>) -> Self {
        self.events_root.set(root.into()).ok();
        self
    }

    /// 换用指定的事件文件轮转配置（装配层按 `config.toml` 注入）。
    pub fn with_journal_config(mut self, config: JournalConfig) -> Self {
        self.journal = config;
        self
    }

    /// 会话业务服务：占用校验、执行队列与 Run 取消都经它访问存储。
    pub(crate) fn session(&self) -> &Arc<SessionService> {
        &self.session
    }

    /// 事件文件轮转配置：轮转任务与用例读它。
    pub fn journal_config(&self) -> JournalConfig {
        self.journal
    }

    /// 注入 Run 执行端口：只允许注入一次，返回本次是否真正注入（D-P2-028 第 3 条）。
    ///
    /// 注入点固定在启动装配（`lib-bin-web`）；未注入的进程（`api_probe_server`、部分进程内用例）
    /// 里 Run 会直接落 `FAILED`「执行器未装配」。
    pub fn install_driver(&self, driver: Arc<dyn RunDriver>) -> bool {
        self.driver.set(driver).is_ok()
    }

    /// Run 执行端口；未注入时为空。
    pub(crate) fn driver(&self) -> Option<Arc<dyn RunDriver>> {
        self.driver.get().cloned()
    }

    /// 在跑 Run 的取消信号表：会话停止时批量触发，`api/run/cancel` 时按 Run 触发。
    pub(crate) fn cancels(&self) -> &Arc<RunCancels> {
        &self.cancels
    }

    /// 触发某个 Run 的取消信号；返回是否真的触发了（未登记说明不在执行中）。
    ///
    /// 数据库里的条件更新由调用方先完成：这里只负责让执行方中断模型流（D-P2-010 第 2 条）。
    pub fn cancel_run(&self, run_id: &str) -> bool {
        self.cancels.cancel(run_id, RunStop::Cancelled)
    }

    /// 事件文件根目录：首次使用时按应用目录推导并缓存。
    pub(crate) fn events_root(&self) -> io::Result<&Path> {
        if let Some(root) = self.events_root.get() {
            return Ok(root.as_path());
        }
        let root = journal::default_root()?;
        Ok(self.events_root.get_or_init(|| root).as_path())
    }

    /// 会话级锁：同一会话的建立、停止与占用登记串行化。
    ///
    /// 先走读锁快路径（命中直接 clone），未命中再降级到写锁插入（P2.1-007）：
    /// 已有会话的取锁不再与所有读操作争同一把写锁。
    pub(crate) fn session_lock(&self, session_id: &str) -> Arc<AsyncMutex<()>> {
        if let Some(lock) = read(&self.inner).session_locks.get(session_id) {
            return Arc::clone(lock);
        }
        let mut inner = write(&self.inner);
        let lock = Arc::clone(inner.session_locks.entry(session_id.to_string()).or_default());
        metrics::record_session_locks(inner.session_locks.len());
        lock
    }

    /// 回收无人在用的会话锁：会话停止与占用释放后调用，返回是否真的移除了条目（P2.1-007）。
    ///
    /// 只有表自己还持有时才移除；仍有其它持有者（正在建立 / 停止）就留给下一次调用。
    /// 回收失败不是错误，也不允许阻塞调用路径。
    pub(crate) fn reclaim_session_lock(&self, session_id: &str) -> bool {
        let mut inner = write(&self.inner);
        if !inner
            .session_locks
            .get(session_id)
            .is_some_and(|lock| Arc::strong_count(lock) == 1)
        {
            return false;
        }
        inner.session_locks.remove(session_id);
        metrics::record_session_locks(inner.session_locks.len());
        debug!(session_id, "回收无人持有的会话锁");
        true
    }

    /// 取同一会话的锁并计时（P2.1-006）：临界区只做状态切换，收尾一律在锁外。
    ///
    /// 持有时间由守卫的 `Drop` 记进指标：它与收尾时长是否解耦由这个数字证明。
    pub(crate) async fn lock_session(&self, session_id: &str) -> SessionLock {
        let lock = self.session_lock(session_id);
        #[cfg(feature = "metrics")]
        let waiting = Instant::now();
        let guard = lock.lock_owned().await;
        #[cfg(feature = "metrics")]
        metrics::record_session_lock_wait(waiting.elapsed());
        SessionLock {
            started: Instant::now(),
            _guard: guard,
        }
    }

    /// 等同一会话上一次的后台收尾完成：新占用不得与旧运行时会话并行（P2.1-006）。
    ///
    /// 通常没有待收尾的会话（返回立即完成），只有「刚释放就重新占用」这条路径会真等。
    pub(crate) async fn await_previous_stop(&self, session_id: &str) {
        if let Some(gate) = self.finalizing.gate(session_id) {
            gate.lock_owned().await;
        }
    }

    /// 该会话是否还有未完成的后台收尾。
    pub(crate) fn has_pending_stop(&self, session_id: &str) -> bool {
        self.finalizing.pending(session_id)
    }

    /// 登记一次后台收尾，返回放行信号：任务结束时先摘掉条目、再放行等待方。
    pub(crate) fn register_finalization(&self, session_id: &str) -> (Arc<AsyncMutex<()>>, OwnedMutexGuard<()>) {
        self.finalizing.register(session_id)
    }

    /// 等全部后台收尾完成：只有关闭流程需要。
    pub(crate) async fn drain_finalizations(&self) {
        for gate in self.finalizing.drain() {
            gate.lock_owned().await;
        }
    }

    /// 把运行时会话从注册表摘下并交给后台收尾，返回是否真的存在会话（P2.1-006）。
    ///
    /// 调用方必须持有该会话的会话锁：摘下会话是状态切换，收尾（写库取消、执行队列等待、
    /// 写者 flush）由后台任务完成，因此占用与释放不会被收尾时长拖住。
    pub(crate) fn stop_session_in_background(&self, session_id: &str, reason: SessionStop) -> bool {
        let Some(session) = self.take_session(session_id) else {
            return false;
        };
        let (gate, held) = self.register_finalization(session_id);
        let finalizing = Arc::clone(&self.finalizing);
        let owned = session_id.to_string();
        tokio::spawn(async move {
            match tokio::time::timeout(FINALIZE_TIMEOUT, session.stop(reason)).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => warn!(session_id = %owned, %error, "运行时会话收尾失败"),
                Err(_) => warn!(
                    session_id = %owned,
                    timeout_secs = FINALIZE_TIMEOUT.as_secs(),
                    "运行时会话收尾超时，放弃等待"
                ),
            }
            // 先摘登记、再放行等待方：等待方拿到闸门时该会话已经收尾完成。
            finalizing.finish(&owned);
            drop(held);
            drop(gate);
        });
        true
    }

    /// 登记一个运行时会话。
    pub(crate) fn insert_session(&self, session: RuntimeSession) {
        let session_id = session.session_id().to_string();
        write(&self.inner).sessions.insert(session_id, session);
    }

    /// 取出运行时会话：停止动作必须在锁外执行，因此先摘下再收尾。
    pub(crate) fn take_session(&self, session_id: &str) -> Option<RuntimeSession> {
        write(&self.inner).sessions.remove(session_id)
    }

    /// 摘下全部运行时会话：关闭流程逐条停止并 flush（停止动作在锁外执行）。
    pub(crate) fn take_sessions(&self) -> Vec<RuntimeSession> {
        std::mem::take(&mut write(&self.inner).sessions).into_values().collect()
    }

    /// 受保护的会话目录：正在占用（即正在写入）的会话，轮转只计不删。
    pub fn occupied_sessions(&self) -> HashSet<String> {
        read(&self.inner).occupancy_by_session.keys().cloned().collect()
    }

    /// 某个会话的事件生产方写入端：Run 执行方（RT-018）与用例据此投递事件。
    ///
    /// 没有运行时会话时为空——未被占用的会话不产出事件。
    pub fn producer(&self, session_id: &str) -> Option<mpsc::Sender<RunEvent>> {
        read(&self.inner).sessions.get(session_id).map(RuntimeSession::producer)
    }

    /// 会话收到一条新消息：唤醒执行队列，并让它先恢复该会话的 `PAUSED` Run（D-P6-009）。
    ///
    /// 返回该会话当前是否被占用：没有被占用的会话没有执行队列，恢复与执行都不会发生。
    pub fn notify_message(&self, session_id: &str) -> bool {
        let inner = read(&self.inner);
        let Some(session) = inner.sessions.get(session_id) else {
            return false;
        };
        session.notify_message();
        true
    }

    /// 唤醒某会话的 Run 执行队列，返回该会话当前是否被占用。
    ///
    /// 没有被占用的会话没有执行队列：新建的 Run 停在 `QUEUED`，由调用方记录原因（D-P1.1-004）。
    pub fn notify_run(&self, session_id: &str) -> bool {
        let inner = read(&self.inner);
        let Some(session) = inner.sessions.get(session_id) else {
            return false;
        };
        session.notify_run();
        true
    }

    /// 某个客户端下行通道的写者句柄：写者用它转发事件并回报写失败。
    pub(crate) fn downlink(&self, client_id: &str) -> Downlink {
        Downlink {
            inner: Arc::clone(&self.inner),
            client_id: client_id.to_string(),
        }
    }

    /// 写入一条客户端记录，返回被替换的旧记录。
    pub fn register(&self, record: ClientRecord) -> Option<ClientRecord> {
        write(&self.inner).clients.insert(record.client_id.clone(), record)
    }

    /// 查询客户端记录；不存在或已反注册时为空。
    pub fn client(&self, client_id: &str) -> Option<ClientRecord> {
        read(&self.inner).clients.get(client_id).cloned()
    }

    /// 全部客户端记录的快照：定时扫描先取快照再逐个判定，判定时不持锁调用其它方法。
    pub fn clients(&self) -> Vec<ClientRecord> {
        read(&self.inner).clients.values().cloned().collect()
    }

    /// 刷新心跳，返回本次 pong 的时刻（心跳与工具确认共用，D-P3-022）。
    ///
    /// 未注册（含已被反注册）的客户端一律返回 [`RuntimeError::ClientNotRegistered`]；
    /// 已标记但尚未被处理链消化的窗口期内仍然成功，且**不解除标记**（D-P1.1-001）。
    pub fn refresh_heartbeat(&self, client_id: &str) -> Result<i64, RuntimeError> {
        let mut inner = write(&self.inner);
        let Some(record) = inner.clients.get_mut(client_id) else {
            return Err(RuntimeError::ClientNotRegistered);
        };
        let now = now_millis();
        record.refresh(now);
        Ok(now)
    }

    /// 应答问询：只清待答状态、**不**刷新 `lastSeen`（D-P1.1-006），返回本次 pong 的时刻。
    ///
    /// 未注册的客户端与心跳同一口径，返回 [`RuntimeError::ClientNotRegistered`]。
    pub fn answer_inquiry(&self, client_id: &str) -> Result<i64, RuntimeError> {
        let mut inner = write(&self.inner);
        let Some(record) = inner.clients.get_mut(client_id) else {
            return Err(RuntimeError::ClientNotRegistered);
        };
        record.answer_inquiry();
        Ok(now_millis())
    }

    /// 到了问询时机就向下行通道投递一条 `inquiry`。
    ///
    /// 返回本次投递的问询；未到时机、已有未答问询、客户端不存在或通道不可用时为空。
    /// 判定与投递分离：定时任务按周期调用，`now_ms` 由调用方给，便于用缩短的时间轴验证。
    pub fn inquire(&self, client_id: &str, now_ms: i64) -> Option<PendingInquiry> {
        let mut inner = write(&self.inner);
        let Registry { clients, channels, .. } = &mut *inner;
        let record = clients.get_mut(client_id)?;
        if record.state != ClientState::Connected || !record.inquiry_due(now_ms) {
            return None;
        }
        let inquiry = PendingInquiry::new(now_ms);
        let event = ClientEvent {
            event: ClientEventKind::Inquiry,
            seq: record.allocate_seq(),
            created_at_ms: now_ms,
            inquiry_at_ms: Some(inquiry.requested_at_ms),
            inquiry_deadline_ms: Some(inquiry.deadline_ms),
            session_event: None,
            close_reason: None,
        };
        // 投递失败（未建流 / 队列满 / 对端已断）不记待答状态：没送达就不该等应答。
        if !channels.get(client_id)?.send(event) {
            return None;
        }
        record.inquiry = Some(inquiry);
        Some(inquiry)
    }

    /// 移除客户端记录（反注册），返回被移除的记录。
    pub fn remove(&self, client_id: &str) -> Option<ClientRecord> {
        let mut inner = write(&self.inner);
        inner.marks.remove(client_id);
        // 下行通道随记录一起丢弃：接收端随即看到关闭，其 watcher 因代号已不存在而不再标记。
        inner.channels.remove(client_id);
        inner.clients.remove(client_id)
    }

    /// 标记客户端断连：同一客户端重复标记只保留第一次的原因。
    ///
    /// 返回本次是否真的写入；已标记过、或客户端不在注册表里都返回 `false`。
    /// 标记是信号位，只有处理链的 `take_marks` 能清除它。
    pub fn mark(&self, client_id: &str, reason: ClientCloseReason) -> bool {
        let inserted = write(&self.inner).mark(client_id, reason);
        // 通知在处理链里被合并也无所谓：链内用 take 语义读标记，重复通知只处理一次。
        if inserted {
            super::handler::notify();
        }
        inserted
    }

    /// 服务关闭时的标记：**覆盖**已有原因，保证关闭导致的会话终止语义一律是 `ServerShutdown`。
    ///
    /// 关闭流程对全部客户端调用；返回本次是否真的写入（客户端已不在注册表里返回 `false`）。
    pub fn mark_shutdown(&self, client_id: &str) -> bool {
        let marked = write(&self.inner).mark_shutdown(client_id);
        if marked {
            super::handler::notify();
        }
        marked
    }

    /// 取走全部标记（take 语义）：返回一次即清空，处理链因此天然幂等。
    pub fn take_marks(&self) -> Vec<(String, ClientCloseReason)> {
        let marks = std::mem::take(&mut write(&self.inner).marks);
        marks.into_iter().collect()
    }

    /// 客户端主动断开（`api/client/delete`）：只打标记并通知处理链。
    ///
    /// 未注册（含已被反注册）的客户端返回 [`RuntimeError::ClientNotRegistered`]；
    /// 已标记但尚未被处理链消化的客户端返回成功（重复断开是幂等的）。
    pub fn disconnect(&self, client_id: &str) -> Result<(), RuntimeError> {
        if self.client(client_id).is_none() {
            return Err(RuntimeError::ClientNotRegistered);
        }
        self.mark(client_id, ClientCloseReason::ClientClosed);
        Ok(())
    }

    /// 向该客户端投递一帧 `client_closed`，返回是否送达。
    ///
    /// 只用于反注册前的最后一次通知：流已断或队列已满都返回 `false`，属预期路径。
    pub fn close_notice(&self, client_id: &str, reason: ClientCloseReason) -> bool {
        let mut inner = write(&self.inner);
        let Registry { clients, channels, .. } = &mut *inner;
        let Some(record) = clients.get_mut(client_id) else {
            return false;
        };
        let event = ClientEvent {
            event: ClientEventKind::ClientClosed,
            seq: record.allocate_seq(),
            created_at_ms: now_millis(),
            session_event: None,
            inquiry_at_ms: None,
            inquiry_deadline_ms: None,
            close_reason: Some(reason),
        };
        channels.get(client_id).is_some_and(|channel| channel.send(event))
    }

    /// 为已注册的客户端打开一条下行通道，返回交给传输层的接收端（D-P1.1-001）。
    ///
    /// 接收端全部 drop（对端断流、写失败）后，watcher 只给该客户端打一次 `StreamClosed` 标记，
    /// 清理交给断连处理链（RT-011）；重开下行流换新代号后，旧 watcher 不再标记新流。
    pub fn open_stream(&self, client_id: &str) -> Result<mpsc::Receiver<ClientEvent>, RuntimeError> {
        let mut inner = write(&self.inner);
        if !inner.clients.contains_key(client_id) {
            return Err(RuntimeError::ClientNotRegistered);
        }
        let generation = inner.allocate_generation();
        let (channel, receiver) = ClientChannel::open(generation);
        let channel = Arc::new(channel);
        inner.channels.insert(client_id.to_string(), Arc::clone(&channel));
        drop(inner);

        let shared = Arc::clone(&self.inner);
        let client_id = client_id.to_string();
        tokio::spawn(async move {
            channel.closed().await;
            if Registry::mark_stream_closed(&shared, &client_id, generation) {
                super::handler::notify();
            }
        });
        Ok(receiver)
    }

    /// 写入一条占用关系，返回该客户端原有的占用。
    ///
    /// 一对一与会话独占的冲突判定由占用服务在调用前完成（RT-014）；这里只保证双向索引一致：
    /// 该客户端已有占用时先清掉旧条目（含旧会话索引）再写入，返回值为旧占用。
    pub fn occupy(&self, occupancy: OccupancyVO) -> Option<OccupancyVO> {
        let mut inner = write(&self.inner);
        let previous = inner.occupancy_by_client.remove(&occupancy.client_id);
        if let Some(previous) = &previous {
            inner.occupancy_by_session.remove(&previous.session_id);
        }
        inner
            .occupancy_by_session
            .insert(occupancy.session_id.clone(), occupancy.clone());
        inner.occupancy_by_client.insert(occupancy.client_id.clone(), occupancy);
        previous
    }

    /// 按客户端 ID 查询占用。
    pub fn occupancy_of_client(&self, client_id: &str) -> Option<OccupancyVO> {
        read(&self.inner).occupancy_by_client.get(client_id).cloned()
    }

    /// 按会话 ID 查询占用。
    pub fn occupancy_of_session(&self, session_id: &str) -> Option<OccupancyVO> {
        read(&self.inner).occupancy_by_session.get(session_id).cloned()
    }

    /// 释放该客户端的占用，返回被释放的占用；没有占用时为空。
    pub fn release(&self, client_id: &str) -> Option<OccupancyVO> {
        let mut inner = write(&self.inner);
        let occupancy = inner.occupancy_by_client.remove(client_id)?;
        inner.occupancy_by_session.remove(&occupancy.session_id);
        Some(occupancy)
    }
}

impl Registry {
    /// 发一个新的下行通道代号。
    fn allocate_generation(&mut self) -> u64 {
        self.stream_generation += 1;
        self.stream_generation
    }

    /// 标记客户端断连；写入口只有 [`RuntimeState::mark`] 与本模块的流断开处理。
    fn mark(&mut self, client_id: &str, reason: ClientCloseReason) -> bool {
        let Some(record) = self.clients.get_mut(client_id) else {
            return false;
        };
        record.state = ClientState::Marked;
        if self.marks.contains_key(client_id) {
            return false;
        }
        self.marks.insert(client_id.to_string(), reason);
        true
    }

    /// 服务关闭：无条件写 `ServerShutdown` 标记，覆盖已有原因（关闭是最后一次标记机会）。
    fn mark_shutdown(&mut self, client_id: &str) -> bool {
        let Some(record) = self.clients.get_mut(client_id) else {
            return false;
        };
        record.state = ClientState::Marked;
        self.marks
            .insert(client_id.to_string(), ClientCloseReason::ServerShutdown);
        true
    }

    /// 下行流断开：只有当前登记的通道仍是发起 watch 的那一条时才标记，
    /// 避免重连后的新流被旧 watcher 误标记；重复触发由 `mark` 吞掉。
    fn mark_stream_closed(inner: &RwLock<Registry>, client_id: &str, generation: u64) -> bool {
        let mut registry = write(inner);
        if registry.channels.get(client_id).map(|channel| channel.generation()) != Some(generation) {
            return false;
        }
        registry.channels.remove(client_id);
        registry.mark(client_id, ClientCloseReason::StreamClosed)
    }
    /// 转发一条 `RunEvent` 到该客户端的下行通道，返回本次是否需要标记断连。
    ///
    /// 没有下行流（客户端还没建流）时只落盘、不标记；通道已断或队列满（慢客户端）
    /// 一律视为该流已废，就地标记 `StreamClosed`，事件本身不回滚（D-P1.1-001）。
    fn forward(&mut self, client_id: &str, event: &RunEvent) -> bool {
        let Some(record) = self.clients.get_mut(client_id) else {
            return false;
        };
        // 已判定的断连不再转发；事件照常落盘。
        if record.state != ClientState::Connected {
            return false;
        }
        let client_event = ClientEvent {
            event: ClientEventKind::SessionEvent,
            seq: record.allocate_seq(),
            created_at_ms: now_millis(),
            session_event: Some(event.clone()),
            inquiry_at_ms: None,
            inquiry_deadline_ms: None,
            close_reason: None,
        };
        let Some(channel) = self.channels.get(client_id) else {
            return false;
        };
        if channel.send(client_event) {
            return false;
        }
        // 通道已断或队列已满：丢弃这条流，清理交给断连处理链。
        metrics::record_downlink_dropped();
        self.channels.remove(client_id);
        self.mark(client_id, ClientCloseReason::StreamClosed)
    }
}

/// 写者持有的注册表句柄：转发事件、回报事件文件写失败。
///
/// 只持有注册表的锁与目标客户端，不持有 `RuntimeState` 本身，写者任务因此无需 `'static` 引用。
#[derive(Clone)]
pub(crate) struct Downlink {
    /// 与 `RuntimeState` 共享的注册表。
    inner: Arc<RwLock<Registry>>,
    /// 占用该会话的客户端。
    client_id: String,
}

impl Downlink {
    /// 转发一条事件；需要标记断连时同时发出通知。
    pub(crate) fn forward(&self, event: &RunEvent) {
        #[cfg(feature = "metrics")]
        let started = Instant::now();
        let marked = write(&self.inner).forward(&self.client_id, event);
        #[cfg(feature = "metrics")]
        metrics::record_downlink_forward(started.elapsed());
        if marked {
            super::handler::notify();
        }
    }

    /// 事件文件写失败：把占用客户端标记为 `JournalError`，清理交给断连处理链（RT-003）。
    pub(crate) fn mark_journal_error(&self) {
        if write(&self.inner).mark(&self.client_id, ClientCloseReason::JournalError) {
            super::handler::notify();
        }
    }
}
/// 取读锁；锁中毒时沿用内部数据，避免单次 panic 让注册表永久失效。
fn read(inner: &RwLock<Registry>) -> RwLockReadGuard<'_, Registry> {
    #[cfg(feature = "metrics")]
    let waiting = Instant::now();
    let guard = inner.read().unwrap_or_else(|error| error.into_inner());
    #[cfg(feature = "metrics")]
    metrics::record_registry_read(waiting.elapsed());
    guard
}

/// 会话锁的持有守卫：离开作用域时把持有时间记进指标（P2.1-006）。
///
/// 会话锁内只允许状态切换；这个数字与收尾耗时是否解耦，就是「收尾已移出临界区」的证据。
pub(crate) struct SessionLock {
    /// 进入临界区的时刻。
    started: Instant,
    /// 会话锁的所有权守卫：先记时长，字段随后释放（`Drop` 结束时才真正解锁）。
    _guard: OwnedMutexGuard<()>,
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        metrics::record_session_lock_hold(self.started.elapsed());
    }
}

/// 后台收尾登记表（P2.1-006）：每个会话一把「在途」闸门，任务结束即摘除。
///
/// 用闸门而不是信号量：等待方 `lock_owned` 就能等到收尾结束，不需要额外的通知原语；
/// 条目由后台任务结束时摘掉，表不随历史会话单调增长。
#[derive(Default)]
struct Finalizer {
    /// 会话 ID → 在途闸门（已被后台任务持有）。
    pending: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl Finalizer {
    /// 登记一次收尾并占住闸门：返回的守卫必须交给后台任务持有，任务结束才放行等待方。
    fn register(&self, session_id: &str) -> (Arc<AsyncMutex<()>>, OwnedMutexGuard<()>) {
        let gate = Arc::new(AsyncMutex::new(()));
        // 新建的闸门必然空闲；这里先占住，登记之后的等待方才会真的排队。
        let held = Arc::clone(&gate).try_lock_owned().expect("新建的收尾闸门必然空闲");
        lock(&self.pending).insert(session_id.to_string(), Arc::clone(&gate));
        (gate, held)
    }

    /// 该会话的在途闸门；没有收尾在进行时为空。
    fn gate(&self, session_id: &str) -> Option<Arc<AsyncMutex<()>>> {
        lock(&self.pending).get(session_id).cloned()
    }

    /// 该会话是否还有未完成的收尾。
    fn pending(&self, session_id: &str) -> bool {
        lock(&self.pending).contains_key(session_id)
    }

    /// 摘掉登记：后台任务在放行等待方之前调用。
    fn finish(&self, session_id: &str) {
        lock(&self.pending).remove(session_id);
    }

    /// 取走全部在途闸门：关闭流程据此等所有收尾完成。
    fn drain(&self) -> Vec<Arc<AsyncMutex<()>>> {
        std::mem::take(&mut *lock(&self.pending)).into_values().collect()
    }
}

/// 取登记表的锁；中毒时沿用内部数据（登记表里只有闸门句柄，中毒不影响正确性）。
fn lock(
    table: &Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
) -> std::sync::MutexGuard<'_, HashMap<String, Arc<AsyncMutex<()>>>> {
    table.lock().unwrap_or_else(|error| error.into_inner())
}

/// 取写锁，中毒处理同 [`read`]。
fn write(inner: &RwLock<Registry>) -> RwLockWriteGuard<'_, Registry> {
    #[cfg(feature = "metrics")]
    let waiting = Instant::now();
    let guard = inner.write().unwrap_or_else(|error| error.into_inner());
    #[cfg(feature = "metrics")]
    metrics::record_registry_write(waiting.elapsed());
    guard
}
