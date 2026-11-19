//! 运行时状态注册表：客户端注册表 + 占用表 + 标记集合 + 下行通道。
//!
//! 并发规则见 `README.md` 第五节：所有读写都在同一把 `RwLock` 内完成、**不持锁跨 `await`**，
//! 需要 `await` 的动作（打开事件文件、向下行通道发送、总线通知）一律先取快照再放锁。
//! 本模块只做内存记账，不感知传输，也不依赖任何 Web 框架。

use lib_core::time::now_millis;
use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::sync::mpsc;
use types_core::runtime::{ClientCloseReason, ClientEvent, ClientEventKind, ClientUpdateKind, OccupancyVO};

use super::RuntimeError;
use super::client::{ClientChannel, ClientRecord, ClientState, PendingInquiry};

/// 进程内运行时状态：由 `lib-bin-web` 启动时构造并注入。
pub struct RuntimeState {
    /// 用 `Arc` 包裹，让下行流的 watcher 能脱离 `&self` 存活（`tokio::spawn` 要求 `'static`）。
    inner: Arc<RwLock<Registry>>,
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
}

impl RuntimeState {
    /// 创建空的运行时状态。
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Registry::default())),
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

    /// 心跳 / 问询应答记账，返回本次 pong 的时刻。
    ///
    /// 未注册（含已被反注册）的客户端一律返回 [`RuntimeError::ClientNotRegistered`]；
    /// 已标记但尚未被处理链消化的窗口期内仍然成功，且**不解除标记**（D-P1.1-001）。
    pub fn heartbeat(&self, client_id: &str, kind: ClientUpdateKind) -> Result<i64, RuntimeError> {
        let mut inner = write(&self.inner);
        let Some(record) = inner.clients.get_mut(client_id) else {
            return Err(RuntimeError::ClientNotRegistered);
        };
        let now = now_millis();
        record.heartbeat(kind, now);
        Ok(now)
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
        write(&self.inner).mark(client_id, reason)
    }

    /// 取走全部标记（take 语义）：返回一次即清空，处理链因此天然幂等。
    pub fn take_marks(&self) -> Vec<(String, ClientCloseReason)> {
        let marks = std::mem::take(&mut write(&self.inner).marks);
        marks.into_iter().collect()
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
            Registry::mark_stream_closed(&shared, &client_id, generation);
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

    /// 下行流断开：只有当前登记的通道仍是发起 watch 的那一条时才标记，
    /// 避免重连后的新流被旧 watcher 误标记；重复触发由 `mark` 吞掉。
    fn mark_stream_closed(inner: &RwLock<Registry>, client_id: &str, generation: u64) {
        let mut registry = write(inner);
        if registry.channels.get(client_id).map(|channel| channel.generation()) != Some(generation) {
            return;
        }
        registry.channels.remove(client_id);
        registry.mark(client_id, ClientCloseReason::StreamClosed);
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

/// 取读锁；锁中毒时沿用内部数据，避免单次 panic 让注册表永久失效。
fn read(inner: &RwLock<Registry>) -> RwLockReadGuard<'_, Registry> {
    inner.read().unwrap_or_else(|error| error.into_inner())
}

/// 取写锁，中毒处理同 [`read`]。
fn write(inner: &RwLock<Registry>) -> RwLockWriteGuard<'_, Registry> {
    inner.write().unwrap_or_else(|error| error.into_inner())
}
