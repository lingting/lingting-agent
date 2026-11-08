//! 工作线程：把客户端的异步请求与事件流搬离界面线程。
//!
//! 界面线程只做渲染与按键，所有 HTTP 调用与 SSE 消费都在这里跑：
//!
//! - 线程内建一个多线程 Tokio 运行时，客户端实例 [`Client`] 由所有任务共享（`Arc`）；
//! - 每条请求都是一个独立任务，因此「正在收增量」时仍能立刻处理「中断」请求；
//! - 下行用 `tokio::sync::mpsc`（工作线程异步 `recv`），上行用 `std::sync::mpsc`
//!   （界面线程 `try_recv` 不阻塞）。

use crate::event::{Command, Update};
use lib_bin_cli::client::{Client, ClientError};
use lib_bin_cli::sdk::{
    ApiClient, MessageListParams, MessageSendPO, RunCancelPO, RunEventKind, RunStreamParams, SessionCreatePO,
};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::{self, JoinHandle};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// 会话标题取任务文本时的最大字符数。
const TITLE_CHARS: usize = 30;

/// 工作线程句柄与两侧通道。
pub struct Worker {
    /// 下行：界面 → 工作线程。
    commands: UnboundedSender<Command>,
    /// 上行：工作线程 → 界面。
    updates: Receiver<Update>,
    /// 线程句柄，退出时 join。
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// 拉起工作线程。
    pub fn spawn(base_url: String) -> std::io::Result<Self> {
        let (commands, command_rx) = unbounded_channel();
        let (update_tx, updates) = std::sync::mpsc::channel();
        let handle = thread::Builder::new()
            .name("agent-tui-worker".to_string())
            .spawn(move || run(base_url, command_rx, update_tx))?;
        Ok(Self {
            commands,
            updates,
            handle: Some(handle),
        })
    }

    /// 发一条请求；工作线程已退出说明退出现在开始，丢弃即可。
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// 取一条更新，不阻塞。
    pub fn try_recv(&self) -> Option<Update> {
        // 通道关闭与暂时没有更新对界面都是「这一拍没有事做」。
        self.updates.try_recv().ok()
    }

    /// 停止工作线程：关掉下行通道让接收端读到 `None`，运行中的任务随运行时一起丢弃。
    pub fn shutdown(mut self) {
        drop(self.commands);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// 工作线程主体：建运行时、建客户端，然后逐条处理请求。
fn run(base_url: String, mut commands: UnboundedReceiver<Command>, updates: Sender<Update>) {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            send(
                &updates,
                Update::Offline {
                    message: format!("创建异步运行时失败：{error}"),
                },
            );
            return;
        }
    };

    runtime.block_on(async move {
        let client = match Client::new(base_url) {
            Ok(client) => Arc::new(client),
            Err(error) => {
                send(
                    &updates,
                    Update::Offline {
                        message: error.to_string(),
                    },
                );
                return;
            }
        };
        while let Some(command) = commands.recv().await {
            dispatch(client.clone(), command, updates.clone());
        }
    });
}

/// 把一条请求派发成任务：请求之间互不阻塞，因此中断不必等长回复。
fn dispatch(client: Arc<Client>, command: Command, updates: Sender<Update>) {
    match command {
        Command::Refresh { session } => {
            tokio::spawn(async move {
                refresh(&client, session, &updates).await;
            });
        }
        Command::CreateSession => {
            tokio::spawn(async move {
                create_session(&client, &updates).await;
            });
        }
        Command::OpenSession { session_id } => {
            tokio::spawn(async move {
                open_session(&client, session_id, &updates).await;
            });
        }
        Command::Send { session_id, content } => {
            tokio::spawn(async move {
                send_message(&client, session_id, content, &updates).await;
            });
        }
        Command::Cancel { run_id } => {
            tokio::spawn(async move {
                cancel(&client, run_id, &updates).await;
            });
        }
        Command::Probe => {
            tokio::spawn(async move {
                probe(&client, &updates).await;
            });
        }
    }
}

/// 空闲期巡检：失败才报 `Offline`，成功不推更新（否则会顺手清掉提示行）。
async fn probe(client: &Client, updates: &Sender<Update>) {
    if let Err(error) = client.probe_health().await.map_err(ClientError::from_call) {
        send(
            updates,
            Update::Offline {
                message: error.to_string(),
            },
        );
    }
}

/// 探活 + 会话列表，并按需重新拉取指定会话的消息。
async fn refresh(client: &Client, session: Option<String>, updates: &Sender<Update>) {
    match client.probe_health().await.map_err(ClientError::from_call) {
        Ok(health) => send(
            updates,
            Update::Online {
                version: health.version,
            },
        ),
        Err(error) => {
            send(
                updates,
                Update::Offline {
                    message: error.to_string(),
                },
            );
            return;
        }
    }
    match client.session_list().await.map_err(ClientError::from_call) {
        Ok(sessions) => send(updates, Update::Sessions(sessions)),
        Err(error) => {
            send(
                updates,
                Update::Notice {
                    text: error.to_string(),
                },
            );
            return;
        }
    }
    if let Some(session_id) = session {
        open_session(client, session_id, updates).await;
    }
}

/// 新建会话。
async fn create_session(client: &Client, updates: &Sender<Update>) {
    let params = SessionCreatePO {
        title: None,
        model: None,
    };
    match client.session_create(params).await.map_err(ClientError::from_call) {
        Ok(session) => send(updates, Update::SessionCreated(session)),
        Err(error) => send(
            updates,
            Update::Notice {
                text: error.to_string(),
            },
        ),
    }
}

/// 拉取会话历史消息。
async fn open_session(client: &Client, session_id: String, updates: &Sender<Update>) {
    let params = MessageListParams {
        session_id: session_id.clone(),
    };
    match client.message_list(params).await.map_err(ClientError::from_call) {
        Ok(messages) => send(updates, Update::Messages { session_id, messages }),
        Err(error) => send(
            updates,
            Update::Notice {
                text: error.to_string(),
            },
        ),
    }
}

/// 发送消息并消费它创建的 Run 的事件流。
async fn send_message(client: &Client, session: Option<String>, content: String, updates: &Sender<Update>) {
    let session_id = match session {
        Some(session_id) => session_id,
        None => {
            let params = SessionCreatePO {
                title: Some(title_of(&content)),
                model: None,
            };
            match client.session_create(params).await.map_err(ClientError::from_call) {
                Ok(session) => {
                    let session_id = session.id.clone();
                    send(updates, Update::SessionCreated(session));
                    session_id
                }
                Err(error) => {
                    send(
                        updates,
                        Update::Notice {
                            text: error.to_string(),
                        },
                    );
                    return;
                }
            }
        }
    };

    let params = MessageSendPO {
        session_id: session_id.clone(),
        content,
    };
    let sent = match client.message_create(params).await.map_err(ClientError::from_call) {
        Ok(sent) => sent,
        Err(error) => {
            send(
                updates,
                Update::Notice {
                    text: error.to_string(),
                },
            );
            return;
        }
    };
    let run_id = sent.run.id.clone();
    send(
        updates,
        Update::Sent {
            session_id,
            message: sent.message,
            run_id: run_id.clone(),
        },
    );
    consume(client, run_id, updates).await;
}

/// 消费事件流，直到终态事件、EOF 或错误。
async fn consume(client: &Client, run_id: String, updates: &Sender<Update>) {
    let mut stream = match client.subscribe_run(RunStreamParams { run_id }).await {
        Ok(stream) => stream,
        Err(error) => {
            send(updates, stream_failure(error));
            return;
        }
    };

    loop {
        let event = match stream.recv().await {
            Ok(Some(event)) => event,
            // 取消后服务端就是这样收尾：没有终态事件，直接结束。
            Ok(None) => {
                send(updates, Update::StreamEnded);
                return;
            }
            Err(error) => {
                send(updates, stream_failure(error));
                return;
            }
        };
        match event.event {
            RunEventKind::RunStarted => send(updates, Update::RunStarted),
            RunEventKind::MessageDelta => {
                if let Some(delta) = event.delta {
                    send(updates, Update::Delta(delta));
                }
            }
            RunEventKind::RunCompleted => {
                send(updates, Update::Completed);
                return;
            }
            RunEventKind::RunFailed => {
                let reason = event.error.unwrap_or_else(|| "未提供失败原因".to_string());
                send(updates, Update::Failed { reason });
                return;
            }
            RunEventKind::ServerShutdown => {
                let reason = event.error.unwrap_or_else(|| "服务正在关闭".to_string());
                send(updates, Update::Interrupted { reason });
                return;
            }
            RunEventKind::Heartbeat => {}
        }
    }
}

/// 取消 Run。
async fn cancel(client: &Client, run_id: String, updates: &Sender<Update>) {
    match client
        .run_cancel(RunCancelPO { run_id })
        .await
        .map_err(ClientError::from_call)
    {
        Ok(_) => send(updates, Update::Cancelled),
        Err(error) => send(
            updates,
            Update::Notice {
                text: error.to_string(),
            },
        ),
    }
}

/// 事件流失败分类：服务端明确拒绝（非 2xx）归业务失败，其余归连接中断。
fn stream_failure(error: ClientError) -> Update {
    let reason = error.to_string();
    match error {
        ClientError::Status { .. } => Update::Failed { reason },
        _ => Update::Interrupted { reason },
    }
}

/// 会话标题取任务文本的首行，过长截断。
fn title_of(content: &str) -> String {
    let first = content.lines().next().unwrap_or(content).trim();
    if first.chars().count() > TITLE_CHARS {
        return first.chars().take(TITLE_CHARS).collect::<String>() + "…";
    }
    first.to_string()
}

/// 推一条更新；界面已退出时接收端关闭，丢弃即可。
fn send(updates: &Sender<Update>, update: Update) {
    let _ = updates.send(update);
}
