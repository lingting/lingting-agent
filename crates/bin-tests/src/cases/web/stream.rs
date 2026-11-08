//! `web.stream`：事件流端点的失败形态 —— 客户端拿到的必须是可读的 API 错误，而不是空流。

use crate::case::CaseFuture;
use crate::context::TestContext;
use anyhow::ensure;
use framework_http::{HttpMethod, HttpRequest};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiClient, MessageSendPO, RUN_STREAM, RunCancelPO, SessionCreatePO};

/// 订阅一个已取消的 Run：服务端必须拒绝并给出可读错误体（D-026）。
pub(super) fn stream(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let client = context.client();
        let session = client
            .session_create(SessionCreatePO {
                title: Some(format!("事件流用例 {}", context.token())),
                model: None,
            })
            .await
            .map_err(ClientError::from_call)?;
        let sent = client
            .message_create(MessageSendPO {
                session_id: session.id.clone(),
                content: format!("事件流用例 {}", context.token()),
            })
            .await
            .map_err(ClientError::from_call)?;
        let cancelled = client
            .run_cancel(RunCancelPO {
                run_id: sent.run.id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;

        let url = format!("{}?runId={}", super::url(context, RUN_STREAM.path), cancelled.id);
        let response = super::send(HttpMethod::Get, HttpRequest::new(url).with_accept("text/event-stream")).await?;

        ensure!(
            response.status == 400,
            "订阅已取消的 Run 应返回 400，实际 {}",
            response.status
        );
        ensure!(
            response.mime_type().as_deref() == Some("application/json"),
            "订阅失败应返回 API 错误体而不是事件流，实际 {:?}",
            response.content_type()
        );
        let error = super::api_error(&response)?;
        ensure!(error.code == 400, "订阅失败的错误码应为 400，实际 {}", error.code);
        ensure!(
            error.message.contains("不可订阅"),
            "订阅失败的错误消息不可读：{}",
            error.message
        );

        Ok(vec![
            format!("Run {} 已取消", cancelled.id),
            format!("订阅失败：{} {}", error.code, error.message),
        ])
    })
}
