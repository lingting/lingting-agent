//! 客户端记录：注册表里的一条客户端实体与它的连接状态（D-P1.1-001）。
//!
//! 记录只保存状态与时间戳；下行通道的服务端句柄由 [`ClientChannel`] 承载，心跳记账见
//! [`ClientRecord::refresh`] / [`ClientRecord::answer_inquiry`]，问询的时机与应答窗口见
//! [`ClientRecord::inquiry_due`] 等判定方法。

use lib_core::time::now_millis;
use tokio::sync::mpsc;
use types_core::runtime::ClientEvent;

/// 问询触发：`lastSeen` 超过该时长没有前进就投递一条问询（D-P1.1-006）。
pub const INQUIRY_AFTER_MS: i64 = 30_000;

/// 问询应答窗口：投递后超过该时长仍未应答即判定断连（D-P1.1-006）。
pub const INQUIRY_WINDOW_MS: i64 = 10_000;

/// 心跳超时：`lastSeen` 超过该时长未刷新即判定断连（D-P1.1-006，3 个 15s 心跳周期）。
pub const CLIENT_TTL_MS: i64 = 45_000;

/// 客户端记录的连接状态，与 `README.md` 的状态机一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    /// 已注册，连接正常。
    Connected,
    /// 已判定断连，等待断连处理链消费标记。
    Marked,
}

/// 注册表里的一条客户端记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRecord {
    /// 客户端 ID，注册时由 `lib_core::id` 生成；每次注册都不同。
    pub client_id: String,
    /// 注册时刻（Unix 毫秒）。
    pub registered_at_ms: i64,
    /// 最近一次心跳时刻（Unix 毫秒）。
    pub last_seen_ms: i64,
    /// 当前连接状态。
    pub state: ClientState,
    /// 当前未答问询；`None` 表示没有待答状态。
    pub inquiry: Option<PendingInquiry>,
    /// 下行事件序号发号器，`ClientEvent.seq` 从这里取（与 `RunEvent.seq` 不同源）。
    next_seq: i64,
}

/// 一条已投递、尚未应答的问询（载荷与 `inquiry` 下行事件一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingInquiry {
    /// 投递时刻（Unix 毫秒）。
    pub requested_at_ms: i64,
    /// 应答截止时刻（Unix 毫秒）。
    pub deadline_ms: i64,
}

impl PendingInquiry {
    /// 以投递时刻为准算出应答截止时刻。
    pub fn new(requested_at_ms: i64) -> Self {
        Self {
            requested_at_ms,
            deadline_ms: requested_at_ms + INQUIRY_WINDOW_MS,
        }
    }
}

impl ClientRecord {
    /// 新建一条 `Connected` 状态的记录，注册与最后活跃时间都取当前时刻。
    pub fn connected(client_id: impl Into<String>) -> Self {
        let now = now_millis();
        Self {
            client_id: client_id.into(),
            registered_at_ms: now,
            last_seen_ms: now,
            state: ClientState::Connected,
            inquiry: None,
            next_seq: 0,
        }
    }

    /// 刷新最后活跃时刻：心跳与工具确认都算「客户端还在」（一次确认同时当心跳）。
    pub fn refresh(&mut self, at_ms: i64) {
        self.last_seen_ms = at_ms;
    }

    /// 应答问询：只清待答状态，**不**刷新 `lastSeen`，否则超时判定失效（D-P1.1-006）。
    pub fn answer_inquiry(&mut self) {
        self.inquiry = None;
    }

    /// 分配一个新的下行事件序号：按客户端从 0 单调递增，与 `RunEvent.seq` 不同源。
    pub fn allocate_seq(&mut self) -> i64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    /// 是否到了问询时机：`lastSeen` 已超过 [`INQUIRY_AFTER_MS`]，且当前没有未答问询。
    ///
    /// 未答问询存在时不重复投递，因此同一客户端同刻最多一条问询。
    pub fn inquiry_due(&self, now_ms: i64) -> bool {
        self.inquiry.is_none() && now_ms - self.last_seen_ms > INQUIRY_AFTER_MS
    }

    /// 未答问询是否已过应答窗口（定时任务据此标记 `InquiryTimeout`）。
    pub fn inquiry_expired(&self, now_ms: i64) -> bool {
        self.inquiry.is_some_and(|inquiry| now_ms >= inquiry.deadline_ms)
    }

    /// 心跳超时：`lastSeen` 超过 [`CLIENT_TTL_MS`] 未刷新。
    pub fn heartbeat_expired(&self, now_ms: i64) -> bool {
        now_ms - self.last_seen_ms > CLIENT_TTL_MS
    }
}

/// 每客户端下行通道的容量：满了即视为该流已废（转发用 `try_send`，不阻塞事件写入）。
const CHANNEL_CAPACITY: usize = 64;

/// 一条下行通道的服务端句柄（D-P1.1-001）：接收端交给传输层，发送端留在注册表。
pub(crate) struct ClientChannel {
    /// 通道代号：重开下行流会换新代号，旧 watcher 因此不会误标记新流。
    generation: u64,
    /// 下行事件的发送端；接收端全部 drop 即代表该流已断。
    sender: mpsc::Sender<ClientEvent>,
}

impl ClientChannel {
    /// 新建一条通道，返回服务端句柄与交给传输层的接收端。
    pub(crate) fn open(generation: u64) -> (Self, mpsc::Receiver<ClientEvent>) {
        let (sender, receiver) = mpsc::channel(CHANNEL_CAPACITY);
        (Self { generation, sender }, receiver)
    }

    /// 该通道的代号。
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// 等待接收端全部 drop：完成即代表下行流已断开。
    pub(crate) async fn closed(&self) {
        self.sender.closed().await;
    }

    /// 向下行通道投递一条事件；队列满或对端已断都返回 `false`，不阻塞事件写入。
    pub(crate) fn send(&self, event: ClientEvent) -> bool {
        self.sender.try_send(event).is_ok()
    }
}
