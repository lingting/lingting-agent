//! 客户端记录：注册表里的一条客户端实体与它的连接状态（D-P1.1-001）。
//!
//! 记录只保存状态与时间戳；下行通道句柄、心跳记账与问询状态在 RT-007 ~ RT-009 落地。

use lib_core::time::now_millis;

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
        }
    }
}
