//! 内置 OpenAI 兼容 Mock LLM Server（P2-018 / D-P2-021）。
//!
//! 手写最小 HTTP/1.1（`tokio::net::TcpListener`，不引入 Web 框架），让 `llm.*` / `chat.*` /
//! `run.*` 用例在不付费、可精确构造异常的前提下驱动真实 Provider 调用。
//!
//! 端点：
//!
//! - `POST …/v1/chat/completions`（JSON 与 SSE 两种响应）；
//! - `GET …/v1/models`；
//! - `GET /mock/last-request`：最近一次 chat 请求体，含完整 `messages` 数组；
//! - `GET /mock/stats`：请求数、场景分布、流式收尾计数。
//!
//! 场景见 [`scenario::Scenario`]，节奏参数用查询串 `chunk_ms` / `stall_ms` 控制。

mod handler;
mod http;
pub(crate) mod observe;
pub(crate) mod probe;
mod record;
mod reply;
mod scenario;

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::task::{JoinHandle, JoinSet};

use record::MockState;

pub(crate) use reply::MODEL;

/// 运行中的 Mock LLM Server：`Drop` 时停止监听并中断所有连接任务。
pub(crate) struct MockServer {
    base_url: String,
    accept: JoinHandle<()>,
}

impl MockServer {
    /// 绑定 `127.0.0.1` 的随机端口并开始接受请求。
    pub(crate) async fn start() -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("Mock Server 绑定端口失败")?;
        let address = listener.local_addr().context("读取 Mock Server 地址失败")?;
        let accept = tokio::spawn(accept_loop(listener, Arc::new(MockState::new())));
        Ok(Self {
            base_url: format!("http://{address}"),
            accept,
        })
    }

    /// Mock Server 的根地址，例如 `http://127.0.0.1:53124`。
    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 拼出 Mock Server 上的绝对地址；`path` 带不带前导 `/` 都可以。
    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        // 中断监听任务；它持有的 `JoinSet` 随之被 drop，连接任务一并中断。
        self.accept.abort();
    }
}

/// 接受连接：一条连接一个请求，连接的收尾由 [`handler`] 负责。
async fn accept_loop(listener: TcpListener, state: Arc<MockState>) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            // 回收已结束的连接任务，避免 `JoinSet` 无限增长。
            _ = connections.join_next(), if !connections.is_empty() => {}
            accepted = listener.accept() => {
                let Ok((socket, _peer)) = accepted else { break };
                let state = state.clone();
                connections.spawn(async move { handler::handle(socket, state).await });
            }
        }
    }
}
