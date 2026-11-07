//! 会话内存存储：P1 的 Mock 数据访问层，P2 由 Repository + SQLite 替换。
//!
//! 会话隔离靠“消息按会话 ID 分桶”保证：任何读取都必须带会话 ID，
//! 不存在跨会话查到消息的入口。

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use types_core::session::{Message, Run, RunStatus, SessionSummary};

/// 会话内存存储。
#[derive(Default)]
pub(crate) struct Store {
    inner: Mutex<Inner>,
}

/// 存储内部状态，全部读写都在同一把锁内完成。
#[derive(Default)]
struct Inner {
    /// 会话摘要，按会话 ID 索引。
    sessions: HashMap<String, SessionSummary>,
    /// 会话消息，按会话 ID 分桶，桶内按写入顺序排列。
    messages: HashMap<String, Vec<Message>>,
    /// Run，按 Run ID 索引。
    runs: HashMap<String, Run>,
}

impl Store {
    /// 创建空存储。
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 取锁；互斥锁中毒时沿用内部数据，避免单次 panic 让 Mock 存储永久失效。
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// 写入会话摘要。
    pub(crate) fn insert_session(&self, session: SessionSummary) {
        self.lock().sessions.insert(session.id.clone(), session);
    }

    /// 按 ID 取会话摘要。
    pub(crate) fn session(&self, session_id: &str) -> Option<SessionSummary> {
        self.lock().sessions.get(session_id).cloned()
    }

    /// 取全部会话摘要，不排序（排序属于业务规则，由 Service 决定）。
    pub(crate) fn sessions(&self) -> Vec<SessionSummary> {
        self.lock().sessions.values().cloned().collect()
    }

    /// 取指定会话的消息；会话不存在时返回 `None`。
    pub(crate) fn messages(&self, session_id: &str) -> Option<Vec<Message>> {
        let inner = self.lock();
        inner
            .sessions
            .contains_key(session_id)
            .then(|| inner.messages.get(session_id).cloned().unwrap_or_default())
    }

    /// 写入一条消息与它的 Run，并在同一次加锁内更新所属会话的统计信息。
    pub(crate) fn insert_exchange(&self, message: Message, run: Run, now_millis: i64) {
        let mut inner = self.lock();
        if let Some(session) = inner.sessions.get_mut(&message.session_id) {
            session.message_count += 1;
            session.updated_at_ms = now_millis;
        }
        inner
            .messages
            .entry(message.session_id.clone())
            .or_default()
            .push(message);
        inner.runs.insert(run.id.clone(), run);
    }

    /// 按 ID 取 Run。
    pub(crate) fn run(&self, run_id: &str) -> Option<Run> {
        self.lock().runs.get(run_id).cloned()
    }

    /// 修改指定 Run；Run 不存在时返回 `None`。
    pub(crate) fn update_run(&self, run_id: &str, update: impl FnOnce(&mut Run)) -> Option<Run> {
        let mut inner = self.lock();
        let run = inner.runs.get_mut(run_id)?;
        update(run);
        Some(run.clone())
    }

    /// 把 `QUEUED` 的 Run 推进到 `RUNNING`；状态不满足时返回 `None`，保证同一 Run 只有一个生产方。
    pub(crate) fn start_run(&self, run_id: &str, now_millis: i64) -> Option<Run> {
        let mut inner = self.lock();
        let run = inner.runs.get_mut(run_id)?;
        if !matches!(run.status, RunStatus::Queued) {
            return None;
        }
        run.status = RunStatus::Running;
        run.updated_at_ms = now_millis;
        Some(run.clone())
    }

    /// 完成 Run：写入助手消息、更新会话统计并把 Run 落成 `COMPLETED`（同一次加锁内完成）。
    pub(crate) fn complete_run(
        &self,
        run_id: &str,
        message: Message,
        input_tokens: i64,
        output_tokens: i64,
        now_millis: i64,
    ) {
        let mut inner = self.lock();
        if let Some(session) = inner.sessions.get_mut(&message.session_id) {
            session.message_count += 1;
            session.updated_at_ms = now_millis;
        }
        inner
            .messages
            .entry(message.session_id.clone())
            .or_default()
            .push(message);
        if let Some(run) = inner.runs.get_mut(run_id) {
            run.status = RunStatus::Completed;
            run.updated_at_ms = now_millis;
            run.finished_at_ms = Some(now_millis);
            run.input_tokens = Some(input_tokens);
            run.output_tokens = Some(output_tokens);
        }
    }
}
