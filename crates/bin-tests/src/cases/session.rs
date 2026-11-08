//! 会话与 Run 用例：创建会话、发送消息、消费 SSE 事件流并核对最终状态。

use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::{bail, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{
    ApiClient, MessageListParams, MessageRole, MessageSendPO, RunEventKind, RunGetParams, RunStatus, RunStreamParams,
    SessionCreatePO,
};

/// 会话领域用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![Case {
        name: "session.message",
        kind: CaseKind::Shared,
        run: message,
    }]
}

fn message(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let client = context.client();
        let content = format!("回归测试消息 {}", context.token());

        let session = client
            .session_create(SessionCreatePO {
                title: Some(format!("测试会话 {}", context.token())),
                model: None,
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(!session.id.is_empty(), "会话 ID 为空");

        let sent = client
            .message_create(MessageSendPO {
                session_id: session.id.clone(),
                content: content.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(sent.message.content == content, "用户消息未保存");
        ensure!(sent.run.status == RunStatus::Queued, "Run 初始状态应为 Queued");

        let queued = client
            .run_get(RunGetParams {
                run_id: sent.run.id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(queued.status == RunStatus::Queued, "未订阅前 Run 应保持 Queued");

        let mut stream = client
            .subscribe_run(RunStreamParams {
                run_id: sent.run.id.clone(),
            })
            .await?;
        let mut reply = String::new();
        let mut last_seq = -1;
        let mut completed = false;

        while let Some(event) = stream.recv().await? {
            ensure!(event.run_id == sent.run.id, "事件 Run ID 不一致");
            ensure!(event.session_id == session.id, "事件会话 ID 不一致");
            ensure!(event.seq > last_seq, "事件序号未递增：{}", event.seq);
            last_seq = event.seq;

            match event.event {
                RunEventKind::RunStarted => {
                    ensure!(event.status == Some(RunStatus::Running), "run_started 状态应为 Running");
                }
                RunEventKind::MessageDelta => reply.push_str(event.delta.as_deref().unwrap_or_default()),
                RunEventKind::RunCompleted => {
                    ensure!(
                        event.status == Some(RunStatus::Completed),
                        "run_completed 状态应为 Completed"
                    );
                    completed = true;
                    break;
                }
                RunEventKind::RunFailed => bail!("Run 失败：{}", event.error.unwrap_or_default()),
                RunEventKind::Heartbeat | RunEventKind::ServerShutdown => {}
            }
        }

        ensure!(completed, "事件流未收到 run_completed");
        ensure!(reply.contains(&content), "回复未回显用户消息：{reply}");

        let finished = client
            .run_get(RunGetParams {
                run_id: sent.run.id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(finished.status == RunStatus::Completed, "Run 终态应为 Completed");
        ensure!(
            finished.output_tokens.is_some_and(|tokens| tokens > 0),
            "完成后的输出 token 数应大于 0"
        );

        let messages = client
            .message_list(MessageListParams {
                session_id: session.id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(messages.len() == 2, "会话应有 2 条消息，实际 {}", messages.len());
        ensure!(messages[0].role == MessageRole::User, "第一条消息应为用户消息");
        ensure!(messages[0].content == content, "第一条消息内容不一致");
        ensure!(messages[1].role == MessageRole::Assistant, "第二条消息应为助手消息");
        ensure!(!messages[1].content.is_empty(), "助手消息内容为空");
        Ok(())
    })
}
