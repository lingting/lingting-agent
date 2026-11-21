//! Mock 侧观测记录：请求数、场景分布、流式收尾方式与最近一次 chat 请求体。
//!
//! 用例读 `/mock/stats` 与 `/mock/last-request` 判断「Mock 看到了什么」，
//! 例如取消后连接是否真正关闭（N-2）、历史消息是否按序进入请求（X-1）。

use lib_core::time;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Mutex;

use super::scenario::Scenario;

/// 流式响应的收尾方式。
#[derive(Clone, Copy)]
pub(crate) enum StreamOutcome {
    /// 写完所有分块与 `[DONE]`。
    Completed,
    /// 客户端中途关闭连接。
    Aborted,
}

/// Mock 的观测记录；锁只覆盖内存写入，不跨 `await` 持有。
pub(crate) struct MockState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    requests: u64,
    chat_requests: u64,
    chat_json: u64,
    chat_sse: u64,
    scenarios: BTreeMap<&'static str, u64>,
    streams_completed: u64,
    streams_aborted: u64,
    last_request: Option<Value>,
    last_request_at_ms: i64,
}

impl MockState {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }

    /// 所有路径的请求都记一次。
    pub(crate) fn record_request(&self) {
        self.with(|inner| inner.requests += 1);
    }

    /// 记录一次 chat 请求：请求体、是否流式、命中的场景。
    pub(crate) fn record_chat(&self, body: &Value, scenario: Scenario, streaming: bool) {
        self.with(|inner| {
            inner.chat_requests += 1;
            if streaming {
                inner.chat_sse += 1;
            } else {
                inner.chat_json += 1;
            }
            *inner.scenarios.entry(scenario.name()).or_default() += 1;
            inner.last_request = Some(body.clone());
            inner.last_request_at_ms = time::now_millis();
        });
    }

    /// 记录一次流式响应的收尾方式（由 Mock 主动断开 / 静默的场景不计入）。
    pub(crate) fn record_stream(&self, outcome: StreamOutcome) {
        self.with(|inner| match outcome {
            StreamOutcome::Completed => inner.streams_completed += 1,
            StreamOutcome::Aborted => inner.streams_aborted += 1,
        });
    }

    /// 最近一次 chat 请求体；`/mock/last-request` 直接返回它。
    pub(crate) fn last_request(&self) -> Option<Value> {
        self.with(|inner| inner.last_request.clone())
    }

    /// `/mock/stats` 的响应体。
    pub(crate) fn snapshot(&self) -> Value {
        self.with(|inner| {
            json!({
                "requests": inner.requests,
                "chatRequests": inner.chat_requests,
                "chatJson": inner.chat_json,
                "chatSse": inner.chat_sse,
                "scenarios": inner.scenarios,
                "streamsCompleted": inner.streams_completed,
                "streamsAborted": inner.streams_aborted,
                "lastRequestAtMs": inner.last_request_at_ms,
            })
        })
    }

    /// 锁中毒时按「数据仍可用」处理：记录丢失只会让断言失败，不该让服务停摆。
    fn with<T>(&self, action: impl FnOnce(&mut Inner) -> T) -> T {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        action(&mut inner)
    }
}
