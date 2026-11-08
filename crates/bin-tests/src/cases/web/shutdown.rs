//! `web.shutdown`：关闭期间的新请求必须拿到可读的 503，而不是连接被静默丢弃（D-028）。

use crate::case::CaseFuture;
use crate::context::TestContext;
use anyhow::{bail, ensure};
use framework_http::{HttpMethod, HttpRequest};
use lib_bin_cli::sdk::{PROBE_HEALTH, PROBE_SHUTDOWN};
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// 观测 503 的上限：关闭流程本身有 5 秒兜底，观测窗口不超过它。
const REJECT_TIMEOUT: Duration = Duration::from_secs(5);

/// 观测间隔：拒绝窗口只有 200ms，轮询要足够密。
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// 触发关闭后持续请求，直到拿到 503 并校验错误体可读。
pub(super) fn shutdown(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let trigger = super::url(context, PROBE_SHUTDOWN.path);
        super::send(HttpMethod::Post, HttpRequest::new(trigger)).await?;

        let health = super::url(context, PROBE_HEALTH.path);
        let deadline = Instant::now() + REJECT_TIMEOUT;
        let mut last_status = None;

        loop {
            match super::send(HttpMethod::Get, HttpRequest::new(health.clone())).await {
                Ok(response) if response.status == 503 => {
                    ensure!(
                        response.mime_type().as_deref() == Some("application/json"),
                        "关闭期间的错误体应为 JSON，实际 {:?}",
                        response.content_type()
                    );
                    let error = super::api_error(&response)?;
                    ensure!(error.code == 503, "关闭期间的错误码应为 503，实际 {}", error.code);
                    ensure!(
                        error.message == "服务正在关闭，请稍后重试",
                        "关闭期间的错误消息不可读：{}",
                        error.message
                    );
                    return Ok(vec![
                        "关闭探针返回成功".to_string(),
                        format!("关闭期间请求 → {}：{}", error.code, error.message),
                    ]);
                }
                Ok(response) => last_status = Some(response.status),
                Err(error) => bail!(
                    "关闭期间未观测到 503（最后一次状态 {:?}），连接已被直接丢弃：{error:#}",
                    last_status
                ),
            }
            if Instant::now() >= deadline {
                bail!(
                    "关闭后 {} 毫秒内未观测到 503（最后一次状态 {:?}）",
                    REJECT_TIMEOUT.as_millis(),
                    last_status
                );
            }
            sleep(POLL_INTERVAL).await;
        }
    })
}
