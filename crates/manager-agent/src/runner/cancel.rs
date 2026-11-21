//! 在跑 Run 的取消信号登记表（D-P2-010）：每 Run 一个 [`RunCancel`]，Run 结束即移除。
//!
//! 登记表放装配层而不是执行方（`agent-runtime`），因为触发取消的三条路径都在这里：
//! `api/run/cancel`（`session::cancel_run`）、占用释放与客户端断连（运行时会话停止）。
//! 执行方只拿到信号本身，取消原因仍以数据库为权威来源。

use std::collections::HashMap;
use std::sync::Mutex;

use service_core::session::{RunCancel, RunStop};

/// 一个在跑 Run 的登记项。
struct Entry {
    /// 所属会话：会话停止时按它批量取消。
    session_id: String,
    /// 取消信号：执行方持有 clone。
    cancel: RunCancel,
}

/// 在跑 Run 的取消信号表；一个进程一份，随运行时状态创建。
#[derive(Default)]
pub(crate) struct RunCancels {
    /// 按 Run ID 索引；锁只覆盖内存读写，不跨 `await` 持有。
    inner: Mutex<HashMap<String, Entry>>,
}

impl RunCancels {
    /// 登记一个 Run 并返回它的取消信号；重复登记返回已有信号。
    pub(crate) fn register(&self, session_id: &str, run_id: &str) -> RunCancel {
        self.with(|inner| {
            inner
                .entry(run_id.to_string())
                .or_insert_with(|| Entry {
                    session_id: session_id.to_string(),
                    cancel: RunCancel::new(),
                })
                .cancel
                .clone()
        })
    }

    /// Run 结束（任意终态）：移除登记，信号随最后一个持有者丢弃。
    pub(crate) fn finish(&self, run_id: &str) {
        self.with(|inner| inner.remove(run_id));
    }

    /// 取消指定 Run：返回是否真的触发了信号（未登记说明不在执行中）。
    pub(crate) fn cancel(&self, run_id: &str, stop: RunStop) -> bool {
        self.with(|inner| {
            let Some(entry) = inner.get(run_id) else {
                return false;
            };
            entry.cancel.cancel(stop);
            true
        })
    }

    /// 取消某个会话的全部在跑 Run，返回触发数量（同会话串行，正常只有 0 或 1 个）。
    pub(crate) fn cancel_session(&self, session_id: &str, stop: RunStop) -> usize {
        self.with(|inner| {
            inner
                .values()
                .filter(|entry| entry.session_id == session_id)
                .inspect(|entry| entry.cancel.cancel(stop))
                .count()
        })
    }

    /// 锁中毒时按「数据仍可用」处理：登记丢失只会让取消退化为「由数据库终态兜底」。
    fn with<T>(&self, action: impl FnOnce(&mut HashMap<String, Entry>) -> T) -> T {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        action(&mut inner)
    }
}
