//! 工作线程：把注册、心跳、下行流与占用搬离界面线程。
//!
//! 界面线程只做渲染与按键，所有 HTTP 调用与事件流消费都在这里跑：
//!
//! - 线程内建一个多线程 Tokio 运行时，客户端实例 [`Client`] 由所有任务共享（`Arc`）；
//! - 连接由 [`Link`] 持有：注册 → 建立下行流 → 占用会话，全程只有一个 `clientId`；
//! - 下行流是一条长连接（`api/client/stream`），`session_event` 帧里带整条 `RunEvent`，
//!   界面不再逐 Run 订阅（D-P1.1-004）；
//! - 心跳在独立任务里每 [`PING_INTERVAL`] 一次；收到 400 `客户端未注册，请重新连接` 说明
//!   服务端已反注册，整体重连（新 `clientId`）后按服务端消息整体重载（D-011、D-P1.1-005）；
//! - 每条请求都是一个独立任务，因此「正在收增量」时仍能立刻处理「中断」请求；
//! - 下行用 `tokio::sync::mpsc`（工作线程异步 `recv`），上行用 `std::sync::mpsc`
//!   （界面线程 `try_recv` 不阻塞）。

use crate::event::{Command, Update};
use lib_bin_cli::client::{Client, ClientError, EventStream};
use lib_bin_cli::sdk::{
    ApiClient, ClientCreatePO, ClientDecision, ClientDeletePO, ClientEvent, ClientEventKind, ClientPostKind,
    ClientPostPO, ClientStreamParams, MessageListParams, MessageSendPO, OccupancyCreatePO, OccupancyDeletePO,
    RunCancelPO, RunEvent, RunEventKind, RunGetParams, RunStatus, SessionCreatePO, SkillAction, SkillGetParams,
    SkillUpdatePO, ToolCallStatus,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinSet;
use tokio::time::{MissedTickBehavior, interval, timeout};

/// 会话标题取任务文本时的最大字符数。
const TITLE_CHARS: usize = 30;
/// 界面退出后给已发出的请求留的收尾窗口：够一条取消请求落地，又不会被长事件流拖住退出。
const SHUTDOWN_GRACE: Duration = Duration::from_secs(1);
/// 心跳间隔：与 `service-core` 的 `PING_INTERVAL` 保持一致（D-P1.1-006）。
const PING_INTERVAL: Duration = Duration::from_secs(15);
/// 服务端反注册后的固定 message（D-P1.1-005）：收到它必须整体重连。
const NOT_REGISTERED: &str = "客户端未注册，请重新连接";

/// 连接快照：界面排障与用例断言读它。
#[derive(Debug, Clone, Default)]
pub struct LinkSnapshot {
    /// 当前注册的客户端 ID；未连接时为空。
    pub client_id: Option<String>,
    /// 已成功收到 pong 的次数（注册响应里的首个 pong 也算一次）。
    pub pongs: u64,
}

/// 工作线程句柄与两侧通道。
pub struct Worker {
    /// 下行：界面 → 工作线程。
    commands: UnboundedSender<Command>,
    /// 上行：工作线程 → 界面。
    updates: Receiver<Update>,
    /// 连接快照：与工作线程共享。
    snapshot: Arc<Mutex<LinkSnapshot>>,
    /// 线程句柄，退出时 join。
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// 拉起工作线程。
    pub fn spawn(base_url: String) -> std::io::Result<Self> {
        let (commands, command_rx) = unbounded_channel();
        let (update_tx, updates) = std::sync::mpsc::channel();
        let snapshot = Arc::new(Mutex::new(LinkSnapshot::default()));
        let shared = Arc::clone(&snapshot);
        let handle = thread::Builder::new()
            .name("agent-tui-worker".to_string())
            .spawn(move || run(base_url, command_rx, update_tx, shared))?;
        Ok(Self {
            commands,
            updates,
            snapshot,
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

    /// 当前连接快照：用例据此拿到 `clientId` 与心跳次数。
    pub fn snapshot(&self) -> LinkSnapshot {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }

    /// 停止工作线程：关掉下行通道让接收端读到 `None`，已发出的请求在收尾窗口内尽力完成，
    /// 仍在运行的任务随运行时一起丢弃。
    pub fn shutdown(mut self) {
        // 先取快照再关通道：`commands` 是部分移动，之后就不能再借用整个 `self`。
        let snapshot = self.snapshot();
        drop(self.commands);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        tracing::debug!(
            client_id = snapshot.client_id.as_deref().unwrap_or("-"),
            pongs = snapshot.pongs,
            "TUI 连接已收尾"
        );
    }
}

/// 工作线程主体：建运行时、建连接，然后逐条处理请求。
fn run(
    base_url: String,
    mut commands: UnboundedReceiver<Command>,
    updates: Sender<Update>,
    snapshot: Arc<Mutex<LinkSnapshot>>,
) {
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
        let link = Link::new(client, updates.clone(), snapshot);
        // 启动即注册并建立下行流；失败只报未连接，界面可用 Ctrl+R 重做整套连接。
        if let Err(error) = link.connect().await {
            send(
                &updates,
                Update::Offline {
                    message: error.to_string(),
                },
            );
        }

        let mut tasks = JoinSet::new();
        while let Some(command) = commands.recv().await {
            dispatch(&link, command, &updates, &mut tasks);
        }
        // 界面关掉下行通道说明开始退出：先交还占用、再反注册，然后用剩余窗口等已发出的请求。
        let _ = timeout(SHUTDOWN_GRACE, link.close()).await;
        let _ = timeout(SHUTDOWN_GRACE, async { while tasks.join_next().await.is_some() {} }).await;
    });
}

/// 连接：客户端实例、连接期状态与连接快照。
struct Link {
    /// 生成的 SDK 客户端。
    client: Arc<Client>,
    /// 上行通道：把结果推给界面。
    updates: Sender<Update>,
    /// 连接快照：`clientId` 与 pong 次数。
    snapshot: Arc<Mutex<LinkSnapshot>>,
    /// 连接期状态，读写都在这一把异步锁内、不持锁跨 `await`。
    state: AsyncMutex<LinkState>,
}

/// 连接期状态。
#[derive(Default)]
struct LinkState {
    /// 连接代号：每次注册递增，旧的心跳与流任务据此自行退出，不会误操作新连接。
    generation: u64,
    /// 当前连接：注册成功后才写入，反注册或重连前清空。
    current: Option<Current>,
    /// 发送中的窗口：这段时间到达的下行事件先缓冲，等 `Sent` 推给界面后再按序放出，
    /// 否则助手占位会排在用户消息前面。
    buffering: Option<Vec<ClientEvent>>,
}

/// 当前连接：客户端 ID、占用的会话与代号。
struct Current {
    /// 服务端分配的客户端 ID。
    client_id: String,
    /// 当前占用的会话；未占用时为空。
    session_id: Option<String>,
    /// 与 [`LinkState::generation`] 一致。
    generation: u64,
}

impl Link {
    /// 建连接对象：此时还没有注册。
    fn new(client: Arc<Client>, updates: Sender<Update>, snapshot: Arc<Mutex<LinkSnapshot>>) -> Arc<Self> {
        Arc::new(Self {
            client,
            updates,
            snapshot,
            state: AsyncMutex::new(LinkState::default()),
        })
    }

    /// 注册并建立下行流，成功后启动该代号的心跳与消费任务。
    ///
    /// 返回装箱的 future：心跳任务会经「未注册」重连回到这里，装箱把这条环形调用链的类型擦除，
    /// 否则递归的 future 类型无法收敛。
    fn connect(self: &Arc<Self>) -> Pin<Box<dyn Future<Output = Result<(), ClientError>> + Send>> {
        let link = Arc::clone(self);
        Box::pin(async move {
            let registered = link
                .client
                .client_create(ClientCreatePO {})
                .await
                .map_err(ClientError::from_call)?;
            link.count_pong();
            let generation = {
                let mut state = link.state.lock().await;
                state.generation += 1;
                state.current = Some(Current {
                    client_id: registered.client_id.clone(),
                    session_id: None,
                    generation: state.generation,
                });
                state.generation
            };
            link.store_client_id(&registered.client_id);

            let stream = link
                .client
                .subscribe_client(ClientStreamParams {
                    client_id: registered.client_id,
                })
                .await?;
            tokio::spawn(consume(Arc::clone(&link), generation, stream));
            tokio::spawn(heartbeat(Arc::clone(&link), generation));
            Ok(())
        })
    }

    /// 当前连接代号；未连接时为空。
    async fn generation(&self) -> Option<u64> {
        self.state
            .lock()
            .await
            .current
            .as_ref()
            .map(|current| current.generation)
    }

    /// 当前注册的客户端 ID；未连接时为空。
    async fn client_id(&self) -> Option<String> {
        self.state
            .lock()
            .await
            .current
            .as_ref()
            .map(|current| current.client_id.clone())
    }

    /// 确认当前连接仍被服务端承认：没有连接或收到「客户端未注册」时返回 `false`。
    async fn ensure_registered(&self) -> bool {
        let Some(client_id) = self.client_id().await else {
            return false;
        };
        match self
            .client
            .client_post(ClientPostPO {
                client_id,
                kind: ClientPostKind::Ping,
                tool_call_id: None,
                decision: None,
            })
            .await
        {
            Ok(_) => {
                self.count_pong();
                true
            }
            // 连不上只说明服务不可达：连接本身还留着，等界面提示后再次重试。
            Err(error) => !is_not_registered(&ClientError::from_call(error)),
        }
    }

    /// 当前占用的会话。
    async fn occupied_session(&self) -> Option<String> {
        self.state
            .lock()
            .await
            .current
            .as_ref()
            .and_then(|current| current.session_id.clone())
    }

    /// 占用指定会话：换会话时先交还旧占用（一对一约束，不做隐式切换）。
    async fn occupy(self: &Arc<Self>, session_id: &str) -> Result<(), ClientError> {
        let (client_id, previous) = {
            let state = self.state.lock().await;
            let Some(current) = state.current.as_ref() else {
                return Err(ClientError::Status {
                    code: 400,
                    message: NOT_REGISTERED.to_string(),
                });
            };
            (current.client_id.clone(), current.session_id.clone())
        };
        if previous.as_deref() == Some(session_id) {
            return Ok(());
        }
        if previous.is_some() {
            self.client
                .occupancy_delete(OccupancyDeletePO {
                    client_id: client_id.clone(),
                })
                .await
                .map_err(ClientError::from_call)?;
        }
        self.client
            .occupancy_create(OccupancyCreatePO {
                client_id,
                session_id: session_id.to_string(),
            })
            .await
            .map_err(ClientError::from_call)?;
        let mut state = self.state.lock().await;
        if let Some(current) = state.current.as_mut() {
            current.session_id = Some(session_id.to_string());
        }
        Ok(())
    }

    /// 开始发送窗口：之后到达的下行事件先缓冲。
    async fn begin_send(&self) {
        let mut state = self.state.lock().await;
        state.buffering = Some(Vec::new());
    }

    /// 结束发送窗口：返回缓冲的下行事件，由调用方在 `Sent` 之后按序放出。
    async fn end_send(&self) -> Vec<ClientEvent> {
        let mut state = self.state.lock().await;
        state.buffering.take().unwrap_or_default()
    }

    /// 把一条下行事件派发给界面；在发送窗口内先入缓冲。
    async fn dispatch(self: &Arc<Self>, event: ClientEvent) {
        let mut state = self.state.lock().await;
        if let Some(buffer) = state.buffering.as_mut() {
            buffer.push(event);
            return;
        }
        drop(state);
        self.handle_event(event).await;
    }

    /// 处理一条下行事件：问询就地应答，其余按类型转成界面更新。
    async fn handle_event(self: &Arc<Self>, event: ClientEvent) {
        match event.event {
            ClientEventKind::SessionEvent => {
                if let Some(run_event) = event.session_event {
                    dispatch_run_event(&self.updates, run_event);
                }
            }
            ClientEventKind::ClientClosed => send(
                &self.updates,
                Update::Interrupted {
                    reason: "服务端关闭了下行流".to_string(),
                },
            ),
            // 问询应答不刷新 lastSeen（D-P1.1-006），只清掉待答状态。
            ClientEventKind::Inquiry => {
                let client_id = self
                    .state
                    .lock()
                    .await
                    .current
                    .as_ref()
                    .map(|current| current.client_id.clone());
                if let Some(client_id) = client_id {
                    let _ = self
                        .client
                        .client_post(ClientPostPO {
                            client_id,
                            kind: ClientPostKind::InquiryAck,
                            tool_call_id: None,
                            decision: None,
                        })
                        .await;
                }
            }
        }
    }

    /// 退出：先交还占用、再反注册（顺序不可交换，见 D-P1.1-001）。
    async fn close(&self) {
        let current = self.state.lock().await.current.take();
        let Some(current) = current else {
            return;
        };
        if current.session_id.is_some() {
            let _ = self
                .client
                .occupancy_delete(OccupancyDeletePO {
                    client_id: current.client_id.clone(),
                })
                .await;
        }
        let _ = self
            .client
            .client_delete(ClientDeletePO {
                client_id: current.client_id,
            })
            .await;
        self.store_client_id_opt(None);
    }

    /// 记一次 pong。
    fn count_pong(&self) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.pongs += 1;
        }
    }

    /// 记录当前客户端 ID。
    fn store_client_id(&self, client_id: &str) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.client_id = Some(client_id.to_string());
        }
    }

    /// 清空或替换当前客户端 ID。
    fn store_client_id_opt(&self, client_id: Option<String>) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.client_id = client_id;
        }
    }
}

/// 把一条请求派发成任务：请求之间互不阻塞，因此中断不必等长回复。
fn dispatch(link: &Arc<Link>, command: Command, updates: &Sender<Update>, tasks: &mut JoinSet<()>) {
    match command {
        Command::Refresh { session } => {
            let link = Arc::clone(link);
            tasks.spawn(async move {
                refresh_connection(&link, session).await;
            });
        }
        Command::CreateSession => {
            let link = Arc::clone(link);
            let updates = updates.clone();
            tasks.spawn(async move {
                create_session(&link, &updates).await;
            });
        }
        Command::OpenSession { session_id } => {
            let link = Arc::clone(link);
            let updates = updates.clone();
            tasks.spawn(async move {
                open_session(&link, session_id, &updates).await;
            });
        }
        Command::Send { session_id, content } => {
            let link = Arc::clone(link);
            let updates = updates.clone();
            tasks.spawn(async move {
                send_message(&link, session_id, content, &updates).await;
            });
        }
        Command::Cancel { run_id } => {
            let link = Arc::clone(link);
            let updates = updates.clone();
            tasks.spawn(async move {
                cancel(&link, run_id, &updates).await;
            });
        }
        Command::Confirm { tool_call_id, decision } => {
            let link = Arc::clone(link);
            let updates = updates.clone();
            tasks.spawn(async move {
                confirm(&link, tool_call_id, decision, &updates).await;
            });
        }
        Command::SkillList => {
            let link = Arc::clone(link);
            let updates = updates.clone();
            tasks.spawn(async move {
                skill_list(&link, &updates).await;
            });
        }
        Command::SkillUpdate { name, action } => {
            let link = Arc::clone(link);
            let updates = updates.clone();
            tasks.spawn(async move {
                skill_update(&link, name, action, &updates).await;
            });
        }
        Command::SkillDetail { name } => {
            let link = Arc::clone(link);
            let updates = updates.clone();
            tasks.spawn(async move {
                skill_detail(&link, name, &updates).await;
            });
        }
        Command::Probe => {
            let link = Arc::clone(link);
            let updates = updates.clone();
            tasks.spawn(async move {
                probe(&link, &updates).await;
            });
        }
    }
}

/// 界面的重连请求（`Ctrl+R`）：现有连接仍被服务端承认时只刷新数据，否则整体重连。
async fn refresh_connection(link: &Arc<Link>, session: Option<String>) {
    if link.ensure_registered().await {
        refresh(link, session.as_deref()).await;
        return;
    }
    reconnect(link, session).await;
}

/// 整体重连：换新 `clientId` 重做注册与下行流，再按服务端消息整体重载（不做增量拼接）。
async fn reconnect(link: &Arc<Link>, session: Option<String>) {
    let previous = session.or(link.occupied_session().await);
    {
        let mut state = link.state.lock().await;
        state.current = None;
    }
    match link.connect().await {
        Ok(()) => {}
        Err(error) => {
            send(
                &link.updates,
                Update::Offline {
                    message: error.to_string(),
                },
            );
            return;
        }
    }
    if let Some(session_id) = previous.as_deref()
        && let Err(error) = link.occupy(session_id).await
    {
        send(
            &link.updates,
            Update::Notice {
                text: error.to_string(),
            },
        );
        return;
    }
    refresh(link, previous.as_deref()).await;
}

/// 探活 + 会话列表，并按需重新拉取指定会话的消息。
async fn refresh(link: &Arc<Link>, session: Option<&str>) {
    match link.client.probe_health().await.map_err(ClientError::from_call) {
        Ok(health) => send(
            &link.updates,
            Update::Online {
                version: health.version,
            },
        ),
        Err(error) => {
            send(
                &link.updates,
                Update::Offline {
                    message: error.to_string(),
                },
            );
            return;
        }
    }
    match link.client.session_list().await.map_err(ClientError::from_call) {
        Ok(sessions) => send(&link.updates, Update::Sessions(sessions)),
        Err(error) => {
            send(
                &link.updates,
                Update::Notice {
                    text: error.to_string(),
                },
            );
            return;
        }
    }
    if let Some(session_id) = session {
        open_session(link, session_id.to_string(), &link.updates).await;
    }
}

/// 空闲期巡检：失败才报 `Offline`，成功不推更新（否则会顺手清掉提示行）。
async fn probe(link: &Arc<Link>, updates: &Sender<Update>) {
    if let Err(error) = link.client.probe_health().await.map_err(ClientError::from_call) {
        send(
            updates,
            Update::Offline {
                message: error.to_string(),
            },
        );
    }
}

/// 技能清单：面板每次打开都重新拉一次，禁用与 `INVALID` 项都在清单里。
async fn skill_list(link: &Arc<Link>, updates: &Sender<Update>) {
    match link.client.skill_list().await.map_err(ClientError::from_call) {
        Ok(skills) => send(updates, Update::Skills(skills)),
        Err(error) => send(
            updates,
            Update::Notice {
                text: error.to_string(),
            },
        ),
    }
}

/// 启停或重载：服务端返回动作完成后的整张清单，界面直接替换，不再二次请求。
async fn skill_update(link: &Arc<Link>, name: Option<String>, action: SkillAction, updates: &Sender<Update>) {
    let po = SkillUpdatePO { name, action };
    match link.client.skill_update(po).await.map_err(ClientError::from_call) {
        Ok(skills) => send(updates, Update::Skills(skills)),
        Err(error) => send(
            updates,
            Update::Notice {
                text: error.to_string(),
            },
        ),
    }
}

/// 技能详情：正文与引用清单；禁用与 `INVALID` 项的正文为空串，原因仍在 `error` 上。
async fn skill_detail(link: &Arc<Link>, name: String, updates: &Sender<Update>) {
    let params = SkillGetParams { name };
    match link.client.skill_get(params).await.map_err(ClientError::from_call) {
        Ok(detail) => send(updates, Update::SkillDetail(detail)),
        Err(error) => send(
            updates,
            Update::Notice {
                text: error.to_string(),
            },
        ),
    }
}

/// 新建会话。
async fn create_session(link: &Arc<Link>, updates: &Sender<Update>) {
    let params = SessionCreatePO {
        title: None,
        model: None,
    };
    match link.client.session_create(params).await.map_err(ClientError::from_call) {
        Ok(session) => send(updates, Update::SessionCreated(session)),
        Err(error) => send(
            updates,
            Update::Notice {
                text: error.to_string(),
            },
        ),
    }
}

/// 拉取会话历史消息：重连后的整体重载也走这条路。
///
/// 拉完消息再看一眼这条会话最后一个 Run 的状态（消息带 `runId`）：`PAUSED` 说明上一次执行被
/// 打断且可以继续，界面据此给出「已中断（发送消息可继续）」的提示（P6A-015）。查询失败不影响
/// 历史消息的展示——它只是一条提示，没读到就没有提示。
async fn open_session(link: &Arc<Link>, session_id: String, updates: &Sender<Update>) {
    let params = MessageListParams {
        session_id: session_id.clone(),
    };
    let messages = match link.client.message_list(params).await.map_err(ClientError::from_call) {
        Ok(messages) => messages,
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
    let last_run = messages.iter().rev().find_map(|message| message.run_id.clone());
    send(updates, Update::Messages { session_id, messages });
    if let Some(run_id) = last_run {
        paused_update(link, run_id, updates).await;
    }
}

/// 最后一个 Run 处于 `PAUSED` 时推一条提示；其余状态（执行中 / 已终态 / 查询失败）不推。
async fn paused_update(link: &Arc<Link>, run_id: String, updates: &Sender<Update>) {
    let params = RunGetParams { run_id: run_id.clone() };
    let Ok(run) = link.client.run_get(params).await.map_err(ClientError::from_call) else {
        return;
    };
    if run.status != RunStatus::Paused {
        return;
    }
    send(
        updates,
        Update::Paused {
            run_id,
            reason: run.interrupted_reason.unwrap_or_else(|| "执行被中断".to_string()),
        },
    );
}

/// 发送消息：先占用会话（Run 只有被占用时才会执行），再把用户消息与 Run 交给界面。
async fn send_message(link: &Arc<Link>, session: Option<String>, content: String, updates: &Sender<Update>) {
    let session_id = match session {
        Some(session_id) => session_id,
        None => {
            let params = SessionCreatePO {
                title: Some(title_of(&content)),
                model: None,
            };
            match link.client.session_create(params).await.map_err(ClientError::from_call) {
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

    if let Err(error) = link.occupy(&session_id).await {
        send(
            updates,
            Update::Notice {
                text: error.to_string(),
            },
        );
        return;
    }

    // 从这一刻起到达的下行事件先缓冲：服务端可能比本响应更快地推出 run_started。
    link.begin_send().await;
    let params = MessageSendPO {
        session_id: session_id.clone(),
        content,
    };
    let sent = match link.client.message_create(params).await.map_err(ClientError::from_call) {
        Ok(sent) => sent,
        Err(error) => {
            let _ = link.end_send().await;
            send(
                updates,
                Update::Notice {
                    text: error.to_string(),
                },
            );
            return;
        }
    };
    send(
        updates,
        Update::Sent {
            session_id,
            message: sent.message,
            run_id: sent.run.id,
        },
    );
    // `Sent` 已经入队，按到达顺序放出缓冲事件，保证用户消息排在助手占位之前。
    for event in link.end_send().await {
        link.handle_event(event).await;
    }
}

/// 取消 Run。
async fn cancel(link: &Arc<Link>, run_id: String, updates: &Sender<Update>) {
    match link
        .client
        .run_cancel(RunCancelPO { run_id })
        .await
        .map_err(ClientError::from_call)
    {
        Ok(_) => {
            send(updates, Update::Cancelled);
            // 取消后服务端不再为这个 Run 产出事件：这条流不会有终态事件，直接本地收尾。
            send(updates, Update::StreamEnded);
        }
        Err(error) => send(
            updates,
            Update::Notice {
                text: error.to_string(),
            },
        ),
    }
}

/// 应答一次工具确认：上行 `api/client/post { kind = TOOL_CONFIRM }`，成功后本次响应即 pong。
///
/// 失败只报提示：界面在提示到达时把确认行恢复为可应答（见 `App::apply` 的 `Notice` 分支），
/// 用户因此可以重试或改用 `Esc` 取消 Run。
async fn confirm(link: &Arc<Link>, tool_call_id: String, decision: ClientDecision, updates: &Sender<Update>) {
    let Some(client_id) = link.client_id().await else {
        send(
            updates,
            Update::Notice {
                text: NOT_REGISTERED.to_string(),
            },
        );
        return;
    };
    let po = ClientPostPO {
        client_id,
        kind: ClientPostKind::ToolConfirm,
        tool_call_id: Some(tool_call_id),
        decision: Some(decision),
    };
    match link.client.client_post(po).await {
        // 确认同时被当作一次心跳：pong 计数照记，避免界面上心跳停止前进。
        Ok(_) => link.count_pong(),
        Err(error) => send(
            updates,
            Update::Notice {
                text: ClientError::from_call(error).to_string(),
            },
        ),
    }
}

/// 心跳任务：每 [`PING_INTERVAL`] 一次；服务端反注册后整体重连，其他失败等下一拍再试。
async fn heartbeat(link: Arc<Link>, generation: u64) {
    let mut ticker = interval(PING_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        if link.generation().await != Some(generation) {
            return;
        }
        let Some(client_id) = link
            .state
            .lock()
            .await
            .current
            .as_ref()
            .map(|current| current.client_id.clone())
        else {
            return;
        };
        match link
            .client
            .client_post(ClientPostPO {
                client_id,
                kind: ClientPostKind::Ping,
                tool_call_id: None,
                decision: None,
            })
            .await
        {
            Ok(_) => link.count_pong(),
            Err(error) => {
                if is_not_registered(&ClientError::from_call(error)) {
                    reconnect_after_unregistered(&link, generation).await;
                    return;
                }
                // 网络抖动不打断正在进行的一轮回复：等下一拍再试。
            }
        }
    }
}

/// 下行流消费任务：`session_event` 里是整条 `RunEvent`，问询就地应答。
async fn consume(link: Arc<Link>, generation: u64, mut stream: EventStream) {
    let mut closed = false;
    loop {
        match stream.recv().await {
            Ok(Some(event)) => {
                if event.event == ClientEventKind::ClientClosed {
                    closed = true;
                }
                link.dispatch(event).await;
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    // 流结束：仍属于当前代号说明连接真的断了，交给界面提示手动重连。
    if link.generation().await != Some(generation) {
        return;
    }
    // 服务端已经发过 `client_closed`（带关闭原因）时不再重复提示。
    if !closed {
        send(
            &link.updates,
            Update::Interrupted {
                reason: "下行流已断开".to_string(),
            },
        );
    }
}

/// 服务端已反注册：同一代号只重连一次，重连后按服务端消息整体重载。
async fn reconnect_after_unregistered(link: &Arc<Link>, generation: u64) {
    if link.generation().await != Some(generation) {
        return;
    }
    let session = link.occupied_session().await;
    send(
        &link.updates,
        Update::Interrupted {
            reason: NOT_REGISTERED.to_string(),
        },
    );
    reconnect(link, session).await;
}

/// 一条 `RunEvent` 转成界面更新。
fn dispatch_run_event(updates: &Sender<Update>, run_event: RunEvent) {
    match run_event.event {
        // 一次发送可能带出两条 Run（被恢复的旧 Run + 本次的新 Run）：终态与开始都带上 `runId`，
        // 界面按它归类，不按到达顺序猜（P6A-015 第 3 条）。
        RunEventKind::RunStarted => send(
            updates,
            Update::RunStarted {
                run_id: run_event.run_id,
            },
        ),
        RunEventKind::MessageDelta => {
            if let Some(delta) = run_event.delta {
                send(updates, Update::Delta(delta));
            }
        }
        RunEventKind::RunCompleted => send(
            updates,
            Update::Completed {
                run_id: run_event.run_id,
            },
        ),
        RunEventKind::RunFailed => send(
            updates,
            Update::Failed {
                run_id: run_event.run_id,
                reason: run_event.error.unwrap_or_else(|| "未提供失败原因".to_string()),
            },
        ),
        RunEventKind::RunCancelled => send(updates, Update::StreamEnded),
        RunEventKind::ServerShutdown => send(
            updates,
            Update::Interrupted {
                reason: run_event.error.unwrap_or_else(|| "服务正在关闭".to_string()),
            },
        ),
        RunEventKind::Heartbeat => {}
        // 三个工具事件各有一条界面更新（P3-028）：开始 / 结束按 `toolCallId` 配对，
        // 确认请求单独成行并提供 `y` / `n`。缺字段属服务端异常，按空值展示而不是静默丢弃。
        RunEventKind::ToolCallStarted => send(
            updates,
            Update::ToolStarted {
                tool_call_id: run_event.tool_call_id.unwrap_or_default(),
                name: run_event.tool_name.unwrap_or_default(),
            },
        ),
        RunEventKind::ToolCallCompleted => send(
            updates,
            Update::ToolFinished {
                tool_call_id: run_event.tool_call_id.unwrap_or_default(),
                name: run_event.tool_name.unwrap_or_default(),
                // 没有状态说明服务端漏传：按失败展示，避免界面把未知结果当成功。
                status: run_event.tool_status.unwrap_or(ToolCallStatus::Error),
                duration_ms: run_event.tool_duration_ms.unwrap_or_default(),
                error: run_event.error,
            },
        ),
        RunEventKind::ToolCallConfirmRequired => send(
            updates,
            Update::ToolConfirmRequired {
                tool_call_id: run_event.tool_call_id.unwrap_or_default(),
                name: run_event.tool_name.unwrap_or_default(),
                arguments: run_event.tool_arguments.unwrap_or_default(),
            },
        ),
    }
}

/// 是否为「客户端未注册」：服务端反注册后所有上行请求都返回它（D-P1.1-005）。
fn is_not_registered(error: &ClientError) -> bool {
    matches!(error, ClientError::Status { code: 400, message } if message == NOT_REGISTERED)
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
