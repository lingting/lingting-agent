//! 运行时状态注册表：客户端注册表 + 占用表 + 标记集合。
//!
//! 并发规则见 `README.md` 第五节：所有读写都在同一把 `RwLock` 内完成、**不持锁跨 `await`**，
//! 需要 `await` 的动作（打开事件文件、向下行通道发送、总线通知）一律先取快照再放锁。
//! 本模块只做内存记账，不感知传输，也不依赖任何 Web 框架。

use std::collections::HashMap;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use types_core::runtime::{ClientCloseReason, OccupancyVO};

use super::client::{ClientRecord, ClientState};

/// 进程内运行时状态：由 `lib-bin-web` 启动时构造并注入。
pub struct RuntimeState {
    inner: RwLock<Registry>,
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
}

impl RuntimeState {
    /// 创建空的运行时状态。
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Registry::default()),
        }
    }

    /// 写入一条客户端记录，返回被替换的旧记录。
    pub fn register(&self, record: ClientRecord) -> Option<ClientRecord> {
        self.write().clients.insert(record.client_id.clone(), record)
    }

    /// 查询客户端记录；不存在或已反注册时为空。
    pub fn client(&self, client_id: &str) -> Option<ClientRecord> {
        self.read().clients.get(client_id).cloned()
    }

    /// 移除客户端记录（反注册），返回被移除的记录。
    pub fn remove(&self, client_id: &str) -> Option<ClientRecord> {
        let mut inner = self.write();
        inner.marks.remove(client_id);
        inner.clients.remove(client_id)
    }

    /// 标记客户端断连：同一客户端重复标记只保留第一次的原因。
    ///
    /// 返回本次是否真的写入；已标记过、或客户端不在注册表里都返回 `false`。
    /// 标记是信号位，只有处理链的 `take_marks` 能清除它。
    pub fn mark(&self, client_id: &str, reason: ClientCloseReason) -> bool {
        let mut inner = self.write();
        let Some(record) = inner.clients.get_mut(client_id) else {
            return false;
        };
        record.state = ClientState::Marked;
        if inner.marks.contains_key(client_id) {
            return false;
        }
        inner.marks.insert(client_id.to_string(), reason);
        true
    }

    /// 取走全部标记（take 语义）：返回一次即清空，处理链因此天然幂等。
    pub fn take_marks(&self) -> Vec<(String, ClientCloseReason)> {
        let mut inner = self.write();
        let marks = std::mem::take(&mut inner.marks);
        marks.into_iter().collect()
    }

    /// 写入一条占用关系，返回该客户端原有的占用。
    ///
    /// 一对一与会话独占的冲突判定由占用服务在调用前完成（RT-014）；这里只保证双向索引一致：
    /// 该客户端已有占用时先清掉旧条目（含旧会话索引）再写入，返回值为旧占用。
    pub fn occupy(&self, occupancy: OccupancyVO) -> Option<OccupancyVO> {
        let mut inner = self.write();
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
        self.read().occupancy_by_client.get(client_id).cloned()
    }

    /// 按会话 ID 查询占用。
    pub fn occupancy_of_session(&self, session_id: &str) -> Option<OccupancyVO> {
        self.read().occupancy_by_session.get(session_id).cloned()
    }

    /// 释放该客户端的占用，返回被释放的占用；没有占用时为空。
    pub fn release(&self, client_id: &str) -> Option<OccupancyVO> {
        let mut inner = self.write();
        let occupancy = inner.occupancy_by_client.remove(client_id)?;
        inner.occupancy_by_session.remove(&occupancy.session_id);
        Some(occupancy)
    }

    /// 取读锁；锁中毒时沿用内部数据，避免单次 panic 让注册表永久失效。
    fn read(&self) -> RwLockReadGuard<'_, Registry> {
        self.inner.read().unwrap_or_else(|error| error.into_inner())
    }

    /// 取写锁，中毒处理同 [`Self::read`]。
    fn write(&self) -> RwLockWriteGuard<'_, Registry> {
        self.inner.write().unwrap_or_else(|error| error.into_inner())
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self::new()
    }
}
