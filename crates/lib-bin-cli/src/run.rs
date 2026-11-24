//! `agent run`：注册 → 占用 → 发送消息 → 消费下行流 → 打印结果。
//!
//! 事件推送只有 `api/client/stream` 一条通道（D-P1.1-004）：Run 由「会话被占用 + 消息创建」
//! 触发执行，客户端不再订阅 Run。Mock 的助手回复要在整个 Run 跑完后才落库，因此三种输出
//! 形态都要消费到终态事件：`--stream` 立即打印增量，默认打印最终正文，`--json` 打印结构化结果。
//!
//! 退出前先交还占用、再反注册：占用释放会停止运行时会话并落 `run_cancelled` 终态事件。

use crate::client::{Client, ClientError};
use crate::command::RunArgs;
use crate::config;
use crate::result::CommandError;
use crate::runtime;
use crate::sdk::{
    ApiClient, ClientCreatePO, ClientDeletePO, ClientEventKind, ClientStreamParams, ClientUpdateKind, ClientUpdatePO,
    MessageSendPO, OccupancyCreatePO, OccupancyDeletePO, RunEventKind, RunStatus, SessionCreatePO,
};
use anyhow::{Result, anyhow};
use lib_core::logger;
use std::io::Write;
use std::time::Duration;

/// 心跳间隔：与 `service-core` 的 `PING_INTERVAL` 保持一致（D-P1.1-006）。
const PING_INTERVAL: Duration = Duration::from_secs(15);

/// 输出形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Output {
    /// 结束后打印助手回复正文。
    Text,
    /// 结束后打印 JSON。
    Json,
    /// 收到增量立即打印。
    Stream,
}

/// 执行 `agent run`。
pub fn execute(args: &RunArgs) -> Result<(), CommandError> {
    let config = config::resolve_client(args.server.as_deref()).map_err(CommandError::argument)?;
    let guard = logger::init_cli(config.log_level).map_err(CommandError::runtime)?;
    let client = Client::new(config.base_url).map_err(CommandError::runtime)?;

    let outcome = runtime::block_on(execute_async(args, &client));
    guard.flush();
    outcome.map_err(CommandError::runtime)?.map_err(CommandError::runtime)
}

/// 一次任务的完整流程：注册 → 建流 → 占用 → 发消息 → 消费下行流 → 收尾。
async fn execute_async(args: &RunArgs, client: &Client) -> Result<()> {
    let output = output_of(args);

    // 注册：响应里的首个 pong 即注册成功信号。
    let registered = client
        .client_create(ClientCreatePO {})
        .await
        .map_err(ClientError::from_call)?;
    let client_id = registered.client_id;

    let session = client
        .session_create(SessionCreatePO {
            title: Some(args.task.clone()),
            model: None,
        })
        .await
        .map_err(ClientError::from_call)?;
    // 占用成功即启动该会话的运行时会话并打开事件文件，之后创建的 Run 才会被执行。
    client
        .occupancy_create(OccupancyCreatePO {
            client_id: client_id.clone(),
            session_id: session.id.clone(),
        })
        .await
        .map_err(ClientError::from_call)?;
    // 先建流再发消息：服务端不重放事件，晚建流会漏掉开头几条。
    let mut stream = client
        .subscribe_client(ClientStreamParams {
            client_id: client_id.clone(),
        })
        .await?;

    let sent = client
        .message_create(MessageSendPO {
            session_id: session.id.clone(),
            content: args.task.clone(),
        })
        .await
        .map_err(ClientError::from_call)?;

    let mut content = String::new();
    let mut status = RunStatus::Queued;
    let mut heartbeat = tokio::time::interval(PING_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    'consume: loop {
        tokio::select! {
            // 心跳优先：长回复期间也保持连接不被判超时（应答不刷新 lastSeen，见 D-P1.1-006）。
            _ = heartbeat.tick() => {
                let _ = client
                    .client_update(ClientUpdatePO {
                        client_id: client_id.clone(),
                        kind: ClientUpdateKind::Ping,
                    })
                    .await;
            }
            received = stream.recv() => {
                let Some(event) = received? else {
                    break 'consume;
                };
                match event.event {
                    ClientEventKind::SessionEvent => {
                        let Some(run_event) = event.session_event else {
                            continue 'consume;
                        };
                        // 一条客户端流只承载它占用会话的事件，这里仍按 Run 过滤，避免会话切换后串味。
                        if run_event.run_id != sent.run.id {
                            continue 'consume;
                        }
                        if let Some(current) = run_event.status {
                            status = current;
                        }
                        match run_event.event {
                            RunEventKind::MessageDelta => {
                                let delta = run_event.delta.unwrap_or_default();
                                if output == Output::Stream {
                                    print!("{delta}");
                                    std::io::stdout().flush()?;
                                }
                                content.push_str(&delta);
                            }
                            RunEventKind::RunCompleted => break 'consume,
                            RunEventKind::RunFailed => {
                                let reason = run_event.error.unwrap_or_else(|| "未提供失败原因".to_string());
                                return Err(anyhow!("Run 执行失败：{reason}"));
                            }
                            RunEventKind::ServerShutdown => {
                                let reason = run_event.error.unwrap_or_else(|| "服务正在关闭".to_string());
                                return Err(anyhow!(
                                    "事件流被服务端终止：{reason}（已收到 {} 个字符）",
                                    content.chars().count()
                                ));
                            }
                            RunEventKind::RunCancelled => break 'consume,
                            RunEventKind::RunStarted | RunEventKind::Heartbeat => {}
                            // 工具事件在 CLI 输出里暂不展示（展示口径见 P3-028 / P3-029）。
                            RunEventKind::ToolCallStarted
                            | RunEventKind::ToolCallCompleted
                            | RunEventKind::ToolCallConfirmRequired => {}
                        }
                    }
                    // 服务端要关掉这条流：之后的清理由服务端断连链完成，这里不再重连。
                    ClientEventKind::ClientClosed => {
                        let reason = event.close_reason.map(|reason| format!("{reason:?}")).unwrap_or_default();
                        return Err(anyhow!("服务端关闭了下行通道：{reason}"));
                    }
                    // 问询：应答一次即可，短任务不必等超时判定。
                    ClientEventKind::Inquiry => {
                        let _ = client
                            .client_update(ClientUpdatePO {
                                client_id: client_id.clone(),
                                kind: ClientUpdateKind::InquiryAck,
                            })
                            .await;
                    }
                }
            }
        }
    }

    // 收尾顺序固定：先交还占用，再反注册（D-P1.1-001 的客户端编排）。
    let _ = client
        .occupancy_delete(OccupancyDeletePO {
            client_id: client_id.clone(),
        })
        .await;
    let _ = client
        .client_delete(ClientDeletePO {
            client_id: client_id.clone(),
        })
        .await;

    match output {
        Output::Text => println!("{content}"),
        Output::Stream => println!(),
        Output::Json => {
            let payload = serde_json::json!({
                "sessionId": session.id,
                "runId": sent.run.id,
                "status": serde_json::to_value(status)?,
                "content": content,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
    }
    Ok(())
}

/// 由命令行开关决定输出形态；`--json` 与 `--stream` 由 clap 保证互斥。
fn output_of(args: &RunArgs) -> Output {
    if args.json {
        Output::Json
    } else if args.stream {
        Output::Stream
    } else {
        Output::Text
    }
}
