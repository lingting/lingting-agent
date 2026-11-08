//! `agent run`：创建会话 → 发送消息 → 订阅事件流 → 打印结果。
//!
//! Mock 的助手回复只在事件流跑到结束后才落库（D-026），所以三种输出形态都要消费完整个
//! 事件流：`--stream` 立即打印增量，默认打印最终正文，`--json` 打印结构化结果。

use crate::client::{Client, ClientError};
use crate::command::RunArgs;
use crate::config;
use crate::result::CommandError;
use crate::runtime;
use crate::sdk::{ApiClient, MessageSendPO, RunEventKind, RunStatus, RunStreamParams, SessionCreatePO};
use anyhow::{Result, anyhow};
use lib_core::logger;
use std::io::Write;

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

/// 一次任务的完整流程。
async fn execute_async(args: &RunArgs, client: &Client) -> Result<()> {
    let output = output_of(args);

    let session = client
        .session_create(SessionCreatePO {
            title: Some(args.task.clone()),
            model: None,
        })
        .await
        .map_err(ClientError::from_call)?;
    let sent = client
        .message_create(MessageSendPO {
            session_id: session.id.clone(),
            content: args.task.clone(),
        })
        .await
        .map_err(ClientError::from_call)?;

    let mut stream = client
        .subscribe_run(RunStreamParams {
            run_id: sent.run.id.clone(),
        })
        .await?;

    let mut content = String::new();
    let mut status = RunStatus::Queued;
    while let Some(event) = stream.recv().await? {
        if let Some(current) = event.status {
            status = current;
        }
        match event.event {
            RunEventKind::MessageDelta => {
                let delta = event.delta.unwrap_or_default();
                if output == Output::Stream {
                    print!("{delta}");
                    std::io::stdout().flush()?;
                }
                content.push_str(&delta);
            }
            RunEventKind::RunCompleted => break,
            RunEventKind::RunFailed => {
                let reason = event.error.unwrap_or_else(|| "未提供失败原因".to_string());
                return Err(anyhow!("Run 执行失败：{reason}"));
            }
            RunEventKind::ServerShutdown => {
                let reason = event.error.unwrap_or_else(|| "服务正在关闭".to_string());
                return Err(anyhow!(
                    "事件流被服务端终止：{reason}（已收到 {} 个字符）",
                    content.chars().count()
                ));
            }
            RunEventKind::RunStarted | RunEventKind::Heartbeat => {}
        }
    }

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
