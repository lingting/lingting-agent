//! 会话与 Run 用例：创建会话、占用、发送消息、消费下行流并核对最终状态。
//!
//! Run 触发点已改为「会话被占用 + 消息创建」（D-P1.1-004），因此这里统一走
//! `client/create` → `client/stream` → `occupancy/create` → `message/create` 的客户端编排。

use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;
use anyhow::{Context, bail, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{
    ApiClient, ClientEventKind, ClientStreamParams, MessageListParams, MessageRole, MessageSendPO, RunEventKind,
    RunGetParams, RunStatus, SessionCreatePO,
};
use std::time::Duration;

use crate::util::flow::{journal_path, occupy, read_journal, register, release, run_event_of};

/// 会话领域用例。
pub(crate) fn cases() -> Vec<Case> {
    let mut cases = vec![
        Case {
            name: "session.message",
            kind: CaseKind::Shared,
            prep: &[],
            run: message,
        },
        Case {
            name: "session.trigger",
            kind: CaseKind::Shared,
            prep: &[],
            run: trigger,
        },
        Case {
            name: "session.error",
            kind: CaseKind::Shared,
            prep: &[],
            run: error,
        },
        Case {
            name: "session.isolation",
            kind: CaseKind::Shared,
            prep: &[],
            run: isolation,
        },
    ];
    // 终止事件依赖关闭探针，而关闭探针只在 debug 构建注册。
    #[cfg(debug_assertions)]
    cases.push(Case {
        name: "session.stream_terminal",
        kind: CaseKind::Isolated,
        prep: &[],
        run: stream_terminal,
    });
    cases
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

        // 注册 → 建流 → 占用：占用成功后新建的 Run 才会被执行。
        let client_id = register(context).await?;
        let occupied = occupy(context, &client_id, &session.id).await?;
        let mut stream = client
            .subscribe_client(ClientStreamParams {
                client_id: client_id.clone(),
            })
            .await?;

        let sent = client
            .message_create(MessageSendPO {
                session_id: session.id.clone(),
                content: content.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(
            sent.run.status == RunStatus::Queued,
            "消息创建返回的 Run 初始状态应为 Queued"
        );

        let mut reply = String::new();
        let mut last_seq = -1;
        let mut event_count = 0;
        let mut completed = false;

        while let Some(event) = stream.recv().await? {
            let Some(event) = run_event_of(&event, &sent.run.id) else {
                continue;
            };
            ensure!(event.session_id == session.id, "事件会话 ID 不一致");
            ensure!(event.seq > last_seq, "事件序号未递增：{}", event.seq);
            last_seq = event.seq;
            event_count += 1;

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
                RunEventKind::Heartbeat | RunEventKind::ServerShutdown | RunEventKind::RunCancelled => {}
            }
        }

        ensure!(completed, "下行流未收到 run_completed");
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

        // 事件文件是事实源：本次占用一份文件，seq 连续，并以终态事件收尾。
        let journal = journal_path(context, &session.id, &occupied.runtime_session_id);
        let written = read_journal(&journal)?;
        ensure!(
            written.iter().map(|event| event.seq).collect::<Vec<_>>() == (0..written.len() as i64).collect::<Vec<_>>(),
            "事件文件内 seq 应从 0 连续递增"
        );
        ensure!(
            written.last().map(|event| event.event == RunEventKind::RunCompleted) == Some(true),
            "事件文件应以 run_completed 收尾"
        );

        release(context, &client_id).await?;

        Ok(vec![
            format!("会话：id={}，title={}", session.id, session.title),
            format!("用户消息：id={}，内容={}", sent.message.id, content),
            format!(
                "Run：id={}，初始状态={:?}，输出 token={}",
                sent.run.id,
                sent.run.status,
                finished.output_tokens.unwrap_or_default()
            ),
            format!("下行流：{event_count} 条事件，最后序号 {last_seq}"),
            format!("助手回复：{} 字符", reply.chars().count()),
            format!("事件文件：{} 条事件，以 run_completed 收尾", written.len()),
            format!("Run 终态：{:?}", finished.status),
            format!("消息列表：{} 条（用户 + 助手）", messages.len()),
        ])
    })
}

/// Run 触发点：未被占用的会话里 Run 停在 `QUEUED`；建立占用后存量 Run 立即执行（D-P1.1-004）。
fn trigger(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let client = context.client();
        let content = format!("触发点用例 {}", context.token());
        let session = client
            .session_create(SessionCreatePO {
                title: Some(format!("触发点 {}", context.token())),
                model: None,
            })
            .await
            .map_err(ClientError::from_call)?;
        let client_id = register(context).await?;

        // 一、会话未被占用：Run 停在 QUEUED，不自执行，也不产出事件文件。
        let sent = client
            .message_create(MessageSendPO {
                session_id: session.id.clone(),
                content: content.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        let session_events = context.home().join("data").join("events").join(&session.id);
        // 没有被占用的会话没有执行者：给它一点时间，确认 Run 不会自己跑起来。
        tokio::time::sleep(Duration::from_millis(600)).await;
        let queued = client
            .run_get(RunGetParams {
                run_id: sent.run.id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(
            queued.status == RunStatus::Queued,
            "未被占用的会话里 Run 应停在 Queued，实际 {:?}",
            queued.status
        );
        ensure!(
            !session_events.exists(),
            "未被占用的会话不应产生事件文件：{}",
            session_events.display()
        );

        // 二、建立占用：存量 QUEUED Run 立即被执行到终态。
        let occupied = occupy(context, &client_id, &session.id).await?;
        ensure!(
            wait_run_done(client, &sent.run.id).await?,
            "占用建立后 Run 应在超时内执行完成"
        );
        let journal = read_journal(&journal_path(context, &session.id, &occupied.runtime_session_id))?;
        ensure!(
            journal.last().map(|event| event.event == RunEventKind::RunCompleted) == Some(true),
            "执行完成后事件文件应以 run_completed 收尾"
        );

        release(context, &client_id).await?;

        Ok(vec![
            format!("会话：{}", session.id),
            "未占用：Run 停在 QUEUED，未产出事件文件".to_string(),
            format!("占用后：Run 执行完成，事件 {} 条", journal.len()),
        ])
    })
}

/// 业务错误：对象不存在统一 404、参数不满足规则统一 400，响应体仍是可读的 `{code, message}`（R-1）。
fn error(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let client = context.client();
        let missing_session = format!("missing-{}", context.token());
        let missing_run = format!("missing-{}", context.token());
        let mut notes = Vec::new();

        match client
            .message_list(MessageListParams {
                session_id: missing_session.clone(),
            })
            .await
            .map_err(ClientError::from_call)
        {
            Err(ClientError::Status { code, message }) => {
                ensure!(code == 404, "未知会话应返回 404，实际 {code}");
                ensure!(
                    message.contains("会话不存在") && message.contains(&missing_session),
                    "未知会话的错误消息应指出会话 ID：{message}"
                );
                notes.push(format!("未知会话：{code} {message}"));
            }
            other => ensure!(false, "未知会话应返回 404，实际 {other:?}"),
        }

        match client
            .run_get(RunGetParams {
                run_id: missing_run.clone(),
            })
            .await
            .map_err(ClientError::from_call)
        {
            Err(ClientError::Status { code, message }) => {
                ensure!(code == 404, "未知 Run 应返回 404，实际 {code}");
                ensure!(
                    message.contains("Run 不存在") && message.contains(&missing_run),
                    "未知 Run 的错误消息应指出 Run ID：{message}"
                );
                notes.push(format!("未知 Run：{code} {message}"));
            }
            other => ensure!(false, "未知 Run 应返回 404，实际 {other:?}"),
        }

        let session = client
            .session_create(SessionCreatePO {
                title: Some(format!("错误用例 {}", context.token())),
                model: None,
            })
            .await
            .map_err(ClientError::from_call)?;
        match client
            .message_create(MessageSendPO {
                session_id: session.id.clone(),
                content: "   ".to_string(),
            })
            .await
            .map_err(ClientError::from_call)
        {
            Err(ClientError::Status { code, message }) => {
                ensure!(code == 400, "空白消息应返回 400，实际 {code}");
                ensure!(message == "消息内容不能为空", "空白消息的错误消息可读性不符：{message}");
                notes.push(format!("空白消息：{code} {message}"));
            }
            other => ensure!(false, "空白消息应返回 400，实际 {other:?}"),
        }

        // 未知模型（D-P2-015）：清单来自 `[provider.models]`，不在其中就是 400。
        let unknown = format!("no-such-model-{}", context.token());
        match client
            .session_create(SessionCreatePO {
                title: None,
                model: Some(unknown.clone()),
            })
            .await
            .map_err(ClientError::from_call)
        {
            Err(ClientError::Status { code, message }) => {
                ensure!(code == 400, "未知模型应返回 400，实际 {code}");
                ensure!(
                    message == format!("模型不可用：{unknown}"),
                    "未知模型的错误消息不符：{message}"
                );
                notes.push(format!("未知模型：{code} {message}"));
            }
            other => ensure!(false, "未知模型应返回 400，实际 {other:?}"),
        }

        Ok(notes)
    })
}

/// 会话隔离：两个会话的消息互不可见，Run 也只属于各自的会话（R-2）。
fn isolation(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let client = context.client();
        let mut sessions = Vec::new();
        for index in 0..2 {
            let session = client
                .session_create(SessionCreatePO {
                    title: Some(format!("隔离用例 {}-{index}", context.token())),
                    model: None,
                })
                .await
                .map_err(ClientError::from_call)?;
            let content = format!("隔离消息 {}-{index}", context.token());
            client
                .message_create(MessageSendPO {
                    session_id: session.id.clone(),
                    content: content.clone(),
                })
                .await
                .map_err(ClientError::from_call)?;
            sessions.push((session, content));
        }

        let first = &sessions[0];
        let second = &sessions[1];
        let first_messages = client
            .message_list(MessageListParams {
                session_id: first.0.id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        let second_messages = client
            .message_list(MessageListParams {
                session_id: second.0.id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;

        ensure!(
            first_messages.len() == 1,
            "第一个会话应只有 1 条消息，实际 {}",
            first_messages.len()
        );
        ensure!(
            second_messages.len() == 1,
            "第二个会话应只有 1 条消息，实际 {}",
            second_messages.len()
        );
        ensure!(
            first_messages[0].content == first.1,
            "第一个会话读到别人的消息：{}",
            first_messages[0].content
        );
        ensure!(
            second_messages[0].content == second.1,
            "第二个会话读到别人的消息：{}",
            second_messages[0].content
        );
        ensure!(
            first_messages[0].session_id == first.0.id && second_messages[0].session_id == second.0.id,
            "消息的会话归属被串改"
        );

        // 会话统计只算自己的消息，且不存在跨会话读取入口。
        let summaries = client.session_list().await.map_err(ClientError::from_call)?;
        for (session, _) in &sessions {
            let summary = summaries
                .iter()
                .find(|item| item.id == session.id)
                .map(|item| item.message_count)
                .unwrap_or_default();
            ensure!(summary == 1, "会话 {} 的消息计数应为 1，实际 {summary}", session.id);
        }

        Ok(vec![
            format!("会话一：{} → {} 条消息", first.0.id, first_messages.len()),
            format!("会话二：{} → {} 条消息", second.0.id, second_messages.len()),
            "两会话消息互不可见，会话统计各为 1".to_string(),
        ])
    })
}

/// 终止事件：下行流以 `server_shutdown` 收尾而不是挂死，Run 落成已取消（R-3 的失败分支）。
#[cfg(debug_assertions)]
fn stream_terminal(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let client = context.client();
        let content = format!("终止事件 {}", context.token());
        let session = client
            .session_create(SessionCreatePO {
                title: Some(format!("终止用例 {}", context.token())),
                model: None,
            })
            .await
            .map_err(ClientError::from_call)?;
        let client_id = register(context).await?;
        let occupied = occupy(context, &client_id, &session.id).await?;

        let mut stream = client
            .subscribe_client(ClientStreamParams {
                client_id: client_id.clone(),
            })
            .await?;
        let sent = client
            .message_create(MessageSendPO {
                session_id: session.id.clone(),
                content: content.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;

        // 收到 run_started 与至少一段增量后再触发关闭，确保终止事件中断的是进行中的流。
        let mut last_seq = -1;
        let mut deltas = 0;
        let mut started = false;
        while deltas == 0 || !started {
            let Some(event) = stream.recv().await? else {
                bail!("下行流在增量之前就结束了");
            };
            let Some(event) = run_event_of(&event, &sent.run.id) else {
                continue;
            };
            ensure!(event.seq > last_seq, "事件序号未递增：{}", event.seq);
            last_seq = event.seq;
            match event.event {
                RunEventKind::RunStarted => started = true,
                RunEventKind::MessageDelta => deltas += 1,
                _ => {}
            }
        }
        client.probe_shutdown().await.map_err(ClientError::from_call)?;

        // 继续消费：服务端主动关流时先发一帧 client_closed（原因是服务关闭），然后结束响应体。
        let mut close_reason = None;
        loop {
            let Some(event) = stream.recv().await? else {
                break;
            };
            match event.event {
                ClientEventKind::SessionEvent => {
                    if let Some(run_event) = run_event_of(&event, &sent.run.id) {
                        ensure!(run_event.seq > last_seq, "事件序号未递增：{}", run_event.seq);
                        last_seq = run_event.seq;
                        ensure!(
                            run_event.event != RunEventKind::RunCompleted,
                            "关闭中的流不应再收到 run_completed"
                        );
                    }
                }
                ClientEventKind::ClientClosed => {
                    close_reason = event.close_reason;
                    break;
                }
                ClientEventKind::Inquiry => {}
            }
        }
        ensure!(
            close_reason == Some(lib_bin_cli::sdk::ClientCloseReason::ServerShutdown),
            "关闭时应下发 client_closed(ServerShutdown)，实际 {close_reason:?}"
        );
        ensure!(stream.recv().await?.is_none(), "client_closed 之后下行流应立即结束");

        // 事件文件是事实源：Run 落成已取消，文件以 server_shutdown 收尾（RT-003）。
        let path = journal_path(context, &session.id, &occupied.runtime_session_id);
        ensure!(
            crate::util::support::wait_until(|| std::fs::read_to_string(&path)
                .ok()
                .is_some_and(|text| text.contains("\"server_shutdown\"")))
            .await,
            "事件文件应以 server_shutdown 收尾：{}",
            path.display()
        );
        let written = read_journal(&path)?;
        let terminal = written.last().context("事件文件应有终态事件")?;
        ensure!(
            terminal.event == RunEventKind::ServerShutdown,
            "最后一条事件应为 server_shutdown，实际 {:?}",
            terminal.event
        );
        ensure!(
            terminal.status == Some(RunStatus::Cancelled),
            "server_shutdown 应带已取消状态，实际 {:?}",
            terminal.status
        );
        ensure!(
            terminal.error.as_deref().is_some_and(|error| !error.is_empty()),
            "server_shutdown 应带可读原因"
        );

        Ok(vec![
            format!("关闭前已收到 {deltas} 段增量，最后序号 {last_seq}"),
            format!("下行流以 client_closed({close_reason:?}) 收尾后立即结束"),
            format!(
                "事件文件以 server_shutdown 收尾，状态 {:?}，原因 {}",
                terminal.status,
                terminal.error.clone().unwrap_or_default()
            ),
            // 关闭期间不再查询 Run：服务端已进入 503 拒绝窗口，Run 的终态以事件文件的终态事件为准。
        ])
    })
}

/// 轮询直到 Run 抵终态，返回是否正常完成。
async fn wait_run_done(client: &lib_bin_cli::client::Client, run_id: &str) -> anyhow::Result<bool> {
    Ok(crate::util::flow::wait_run(client, run_id).await?.status == RunStatus::Completed)
}
