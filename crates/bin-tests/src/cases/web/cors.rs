//! `web.cors`：浏览器来源的预检与实际请求（D-027）。

use crate::case::CaseFuture;
use crate::context::TestContext;
use anyhow::ensure;
use framework_http::{HttpMethod, HttpRequest};
use lib_bin_cli::sdk::PROBE_HEALTH;

/// 白名单内的浏览器来源：开发环境 WebUI 端口（D-022）。
const ALLOWED_ORIGIN: &str = "http://127.0.0.1:20101";

/// 白名单外的来源。
const DENIED_ORIGIN: &str = "http://example.com";

/// 断言预检与实际请求的 CORS 头，并确认非白名单来源拿不到任何 `access-control-*`。
pub(super) fn cors(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let target = super::url(context, PROBE_HEALTH.path);

        let preflight = super::send(HttpMethod::Options, preflight_request(&target, ALLOWED_ORIGIN)).await?;
        ensure!(
            preflight.status == 204,
            "白名单预检应返回 204，实际 {}",
            preflight.status
        );
        ensure!(
            preflight
                .header("access-control-allow-origin")
                .is_some_and(|value| value == ALLOWED_ORIGIN),
            "预检未回显允许来源，实际 {:?}",
            preflight.header("access-control-allow-origin")
        );
        ensure!(
            preflight
                .header("access-control-allow-methods")
                .is_some_and(|value| value.contains("POST")),
            "预检未声明允许方法，实际 {:?}",
            preflight.header("access-control-allow-methods")
        );
        ensure!(
            preflight
                .header("access-control-allow-headers")
                .is_some_and(|value| value.contains("content-type")),
            "预检未声明允许请求头，实际 {:?}",
            preflight.header("access-control-allow-headers")
        );
        ensure!(
            preflight.header("access-control-max-age").is_some(),
            "预检缺少缓存时长（access-control-max-age）"
        );

        let actual = super::send(
            HttpMethod::Get,
            HttpRequest::new(target.clone()).with("origin", ALLOWED_ORIGIN),
        )
        .await?;
        ensure!(actual.status == 200, "白名单实际请求应返回 200，实际 {}", actual.status);
        ensure!(
            actual
                .header("access-control-allow-origin")
                .is_some_and(|value| value == ALLOWED_ORIGIN),
            "实际请求未回显允许来源，实际 {:?}",
            actual.header("access-control-allow-origin")
        );
        ensure!(
            actual
                .header("access-control-expose-headers")
                .is_some_and(|value| value.contains("x-trace-id")),
            "实际请求未暴露 x-trace-id，实际 {:?}",
            actual.header("access-control-expose-headers")
        );

        let denied_preflight = super::send(HttpMethod::Options, preflight_request(&target, DENIED_ORIGIN)).await?;
        ensure!(
            denied_preflight.header("access-control-allow-origin").is_none(),
            "非白名单预检不应输出允许来源，实际 {:?}",
            denied_preflight.header("access-control-allow-origin")
        );

        let denied = super::send(HttpMethod::Get, HttpRequest::new(target).with("origin", DENIED_ORIGIN)).await?;
        ensure!(
            denied.header("access-control-allow-origin").is_none(),
            "非白名单请求不应输出允许来源，实际 {:?}",
            denied.header("access-control-allow-origin")
        );

        Ok(vec![
            format!("白名单预检 204，允许来源回显 {ALLOWED_ORIGIN}"),
            "实际请求带 allow-origin 与 expose-headers: x-trace-id".to_string(),
            "非白名单来源的预检与实际请求均无 access-control-*".to_string(),
        ])
    })
}

/// 构造一次预检请求。
fn preflight_request(url: &str, origin: &str) -> HttpRequest {
    HttpRequest::new(url)
        .with("origin", origin)
        .with("access-control-request-method", "GET")
        .with("access-control-request-headers", "content-type")
}
