//! `web.stream`：旧事件流端点已移除，新的下行流端点失败时仍是可读的 API 错误。
//!
//! `api/run/stream` 属破坏性契约变更（D-P1.1-004）：它不再注册，框架按未知路由返回 404；
//! 服务端推送只剩 `api/client/stream`，建立之前的失败（未注册的 `clientId`）返回
//! 400 + 固定 message，而不是一条空流（D-P1.1-001）。

use crate::case::CaseFuture;
use crate::context::TestContext;
use anyhow::ensure;
use framework_http::{HttpMethod, HttpRequest};
use lib_bin_cli::sdk::CLIENT_STREAM;

/// 旧的 `api/run/stream` 必须 404；`api/client/stream` 未注册时给 400 + 固定 message。
pub(super) fn stream(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        // 旧端点：不认识这个路径，返回与「本就不存在的接口」一致的 404。
        let removed = super::send(
            HttpMethod::Get,
            HttpRequest::new(super::url(context, "api/run/stream")).with_accept("text/event-stream"),
        )
        .await?;
        ensure!(
            removed.status == 404,
            "已移除的 api/run/stream 应返回 404，实际 {}",
            removed.status
        );

        // 新下行流：未注册的 clientId 拿到的必须是可读错误体，而不是空事件流。
        let client_id = format!("missing-{}", context.token());
        let url = format!("{}?clientId={}", super::url(context, CLIENT_STREAM.path), client_id);
        let response = super::send(HttpMethod::Get, HttpRequest::new(url).with_accept("text/event-stream")).await?;

        ensure!(
            response.status == 400,
            "未注册的 clientId 建立下行流应返回 400，实际 {}",
            response.status
        );
        ensure!(
            response.mime_type().as_deref() == Some("application/json"),
            "建立失败的响应应是 API 错误体而不是事件流，实际 {:?}",
            response.content_type()
        );
        let error = super::api_error(&response)?;
        ensure!(error.code == 400, "错误码应为 400，实际 {}", error.code);
        ensure!(
            error.message == "客户端未注册，请重新连接",
            "固定 message 不符：{}",
            error.message
        );

        Ok(vec![
            "旧端点 api/run/stream 返回 404".to_string(),
            format!("新下行流未注册：{} {}", error.code, error.message),
        ])
    })
}
