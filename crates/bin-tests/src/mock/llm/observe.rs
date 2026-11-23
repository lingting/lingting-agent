//! Mock 的观测侧 helper：`/mock/stats`、`/mock/last-request` 的读取与等待。
//!
//! 同一观测语义只有这一处实现（P2.1-010）：`util::flow` 与各用例都调用这里，
//! 不再各自写 HTTP 读取与轮询。

use anyhow::{Context, Result, ensure};
use framework_http::{HttpClient, HttpRequest};
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::time::sleep;

use super::MockServer;

/// 等 Mock 记录到「至少一次」连接中断的上限：Mock 在下一帧才探测到关闭。
const ABORT_TIMEOUT: Duration = Duration::from_secs(10);
/// 轮询 `/mock/stats` 的间隔。
const ABORT_INTERVAL: Duration = Duration::from_millis(100);
/// 等连接中断计数超过某个已知值的上限。
const ABORT_STEP_TIMEOUT: Duration = Duration::from_secs(5);
/// 等连接中断计数增长的轮询间隔：FIN 到达 Mock 只需要一点时间。
const ABORT_STEP_INTERVAL: Duration = Duration::from_millis(20);

/// GET 一个 JSON 端点（`/v1/models`、`/mock/*`）。
pub(crate) async fn get_json(url: &str) -> Result<Value> {
    let client = HttpClient::new();
    let response = client
        .get(HttpRequest::new(url))
        .await
        .with_context(|| format!("GET {url} 失败"))?;
    ensure!(response.status == 200, "GET {url} 应返回 200，实际 {}", response.status);
    let body = response.string();
    serde_json::from_str(&body).with_context(|| format!("GET {url} 的响应不是 JSON：{body}"))
}

/// `/mock/stats` 的响应体。
pub(crate) async fn stats(server: &MockServer) -> Result<Value> {
    get_json(&server.url("mock/stats")).await
}

/// 当前的客户端关闭计数（`streamsAborted`）。
pub(crate) async fn aborted(server: &MockServer) -> Result<u64> {
    let stats = stats(server).await?;
    Ok(stats["streamsAborted"].as_u64().unwrap_or_default())
}

/// 最近一次 chat 请求体。
pub(crate) async fn last_request(server: &MockServer) -> Result<Value> {
    get_json(&server.url("mock/last-request")).await
}

/// 最近一次 chat 请求的 `messages` 数组。
pub(crate) async fn last_request_messages(server: &MockServer) -> Result<Vec<Value>> {
    last_request(server)
        .await?
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .context("last-request 应返回完整的 messages 数组")
}

/// 等 Mock 记录到一次连接中断（`streamsAborted` 至少为 1）；超时返回可读错误。
///
/// Mock 只在写下一帧探测时才可能发现对端关闭，因此执行方 drop 模型流之后需要轮询一小会儿：
/// 这是 O-1 的实测口径（P2-018：drop 流式响应确实关闭 TCP 连接）。
pub(crate) async fn wait_aborted(server: &MockServer) -> Result<u64> {
    let aborted = poll_aborted(server, 0, ABORT_TIMEOUT, ABORT_INTERVAL).await?;
    ensure!(aborted >= 1, "Mock 在 {} 秒内未记录到连接中断", ABORT_TIMEOUT.as_secs());
    Ok(aborted)
}

/// 等 `streamsAborted` 超过 `before`；超时返回当前值，由调用方给出断言。
pub(crate) async fn wait_aborted_over(server: &MockServer, before: u64) -> Result<u64> {
    poll_aborted(server, before, ABORT_STEP_TIMEOUT, ABORT_STEP_INTERVAL).await
}

/// 轮询 `/mock/stats` 直到 `streamsAborted` 超过 `before`，到点返回最后一次读到的值。
async fn poll_aborted(server: &MockServer, before: u64, timeout: Duration, interval: Duration) -> Result<u64> {
    let deadline = Instant::now() + timeout;
    loop {
        let aborted = aborted(server).await?;
        if aborted > before || Instant::now() >= deadline {
            return Ok(aborted);
        }
        sleep(interval).await;
    }
}
