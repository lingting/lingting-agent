//! 断连处理链用例：来源收敛、串行入口、反注册与拒绝语义、会话停止与补偿（RT-011 ~ RT-013）。
//!
//! 处理链跑在后台任务里，断言一律“轮询等待 + 终态检查”：用例不假设它何时被唤醒。
//! `runtime.disconnect` 用进程级注入的运行时状态（处理链只安装一次），其余用例各自建实例。

use std::path::{Path, PathBuf};

use agent_core::session::{RunEvent, RunEventKind, SessionCreatePO as ContractSessionCreatePO};
use anyhow::{Context, Result, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{
    ApiClient, ApiError, CLIENT_DELETE, CLIENT_POST, ClientCreatePO, ClientDeletePO, ClientPostKind, ClientPostPO,
    OCCUPANCY_CREATE, OCCUPANCY_DELETE, OccupancyCreatePO, OccupancyDeletePO, SessionCreatePO,
};
use lib_core::time;
use manager_agent::runtime::{ClientRecord, RuntimeState, handler, journal};
use tokio::sync::mpsc;
use types_core::runtime::{ClientCloseReason, OccupancyCreatePO as ContractOccupancyCreatePO, OccupancyVO};

use crate::case::{Case, CaseFuture, CaseKind, CaseOutcome};
use crate::util::context::TestContext;
use crate::util::support::{
    assembly_events_root, installed, runtime_state, session_service, temp_directory, wait_until,
};

/// 断连处理链用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "runtime.disconnect",
            kind: CaseKind::Shared,
            prep: &[],
            run: disconnect,
        },
        Case {
            name: "runtime.handler",
            kind: CaseKind::Shared,
            prep: &[],
            run: serial_entry,
        },
        Case {
            name: "runtime.unregister",
            kind: CaseKind::Shared,
            prep: &[],
            run: unregister,
        },
    ]
}

/// 六条来源收敛到同一入口，清理顺序固定，事件文件以终态事件收尾（RT-011、RT-013）。
fn disconnect(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { disconnect_body().await })
}

/// 用例主体：独立成 async fn，避免整块 future 在 `dyn Future + Send` 上做高阶生命周期推导。
async fn disconnect_body() -> Result<CaseOutcome> {
    let state = global_state().await?;
    let root = assembly_events_root().await?.to_path_buf();

    // 每条来源各占一个客户端：原因不同，处理结果必须一致。
    let sources = [
        (ClientCloseReason::HeartbeatTimeout, "心跳超时"),
        (ClientCloseReason::InquiryTimeout, "问询无应答"),
        (ClientCloseReason::ClientClosed, "客户端主动断开"),
        (ClientCloseReason::StreamClosed, "下行流断开"),
        (ClientCloseReason::JournalError, "事件文件写失败"),
    ];
    let mut fixtures = Vec::new();
    for (index, (reason, label)) in sources.iter().enumerate() {
        let fixture = Fixture::new(state, &root, &format!("disconnect-{index}"), label).await?;
        // 先产出一条事件：终态事件必须接在它后面，证明链是「先收尾、再关闭」。
        let producer = fixture.producer(state)?;
        producer.send(event(&fixture, RunEventKind::RunStarted)).await?;
        drop(producer);
        ensure!(
            wait_until(|| read_events(&fixture.path()).len() == 1).await,
            "{label}：起手事件应已落盘"
        );
        ensure!(state.mark(&fixture.client_id, *reason), "{label}：首次标记应写入");
        fixtures.push((fixture, label));
    }

    let cleaned = wait_until(|| {
        fixtures
            .iter()
            .all(|(fixture, _)| state.client(&fixture.client_id).is_none())
    })
    .await;
    ensure!(cleaned, "断连处理链未在超时内完成清理");

    for (fixture, label) in &fixtures {
        ensure!(
            state.occupancy_of_client(&fixture.client_id).is_none(),
            "{label}：占用记录应已撤回"
        );
        ensure!(
            state.occupancy_of_session(&fixture.session_id).is_none(),
            "{label}：会话侧索引应已清空"
        );
        // 收尾在后台任务里（P2.1-006）：清理链返回后终态事件才落盘，先等它出现。
        ensure!(
            wait_until(|| read_events(&fixture.path()).len() == 2).await,
            "{label}：终态事件应在超时内落盘"
        );
        let events = read_events(&fixture.path());
        ensure!(events.len() == 2, "{label}：应有一条终态事件，实际 {} 条", events.len());
        let terminal = &events[1];
        ensure!(
            terminal.event == RunEventKind::RunCancelled,
            "{label}：终态事件应为 run_cancelled，实际 {:?}",
            terminal.event
        );
        ensure!(
            terminal.error.as_deref() == Some("客户端连接已断开"),
            "{label}：取消原因应为固定文本，实际 {:?}",
            terminal.error
        );
        ensure!(terminal.seq == 1, "{label}：终态事件的 seq 应接在最后一条之后");
    }

    // 单点失败：占用记录还在但运行时会话已不在，其余清理照做。
    let ghost = "disconnect-ghost";
    state.register(ClientRecord::connected(ghost));
    state.occupy(OccupancyVO {
        session_id: "disconnect-ghost-session".to_string(),
        client_id: ghost.to_string(),
        runtime_session_id: "disconnect-ghost-runtime".to_string(),
        acquired_at_ms: time::now_millis(),
    });
    ensure!(
        state.mark(ghost, ClientCloseReason::StreamClosed),
        "补偿用例的标记应写入"
    );
    ensure!(
        wait_until(|| state.client(ghost).is_none()).await,
        "单步失败不应中断其余清理"
    );
    ensure!(state.occupancy_of_client(ghost).is_none(), "单步失败后占用登记仍应撤回");

    Ok(vec![
        format!("来源：{} 种原因各自清理，结果一致", sources.len()),
        "反注册 → 停止运行时会话 → 追加终态事件 → 撤回占用登记".to_string(),
        "终态事件：run_cancelled / 客户端连接已断开".to_string(),
        "占用记录在而运行时会话不在时，其余清理照做".to_string(),
    ])
}

/// 串行入口与幂等：并发进入只有一个处理链在跑，标记取走后不再处理（RT-011）。
fn serial_entry(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let root = temp_directory("handler")?;
        let state = runtime_state(&root).await?;
        for index in 0..2 {
            state.register(ClientRecord::connected(format!("handler-{index}")));
        }
        ensure!(
            state.mark("handler-0", ClientCloseReason::ClientClosed),
            "首次标记应写入"
        );
        ensure!(
            state.mark("handler-1", ClientCloseReason::HeartbeatTimeout),
            "首次标记应写入"
        );

        // 两条通知、两次并发调用：合计只处理一遍（重复处理会让计数变成 4）。
        let (first, second) = tokio::join!(handler::process(&state), handler::process(&state));
        ensure!(first + second == 2, "两个标记应各处理一次，实际 {} 次", first + second);
        ensure!(handler::process(&state).await == 0, "标记取走后不应再处理");
        ensure!(
            state.client("handler-0").is_none() && state.client("handler-1").is_none(),
            "两个客户端都应被反注册"
        );
        ensure!(
            !state.mark("handler-missing", ClientCloseReason::ClientClosed),
            "未注册的客户端不应被标记"
        );

        std::fs::remove_dir_all(&root).ok();
        Ok(vec![
            "并发进入只跑一条处理链，合计处理一次".to_string(),
            "标记取走后重复调用不再处理".to_string(),
        ])
    })
}

/// 反注册后的拒绝语义：旧 `clientId` 的上行请求一律 400 + 固定 message（RT-012）。
fn unregister(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let created = context
            .client()
            .client_create(ClientCreatePO {})
            .await
            .map_err(ClientError::from_call)?;
        let session = context
            .client()
            .session_create(SessionCreatePO {
                title: Some(context.token()),
                model: None,
            })
            .await
            .map_err(ClientError::from_call)?;

        // 先占用一个会话，确认断开前的正常路径。
        let occupied = context
            .client()
            .occupancy_create(OccupancyCreatePO {
                client_id: created.client_id.clone(),
                session_id: session.id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;

        // 主动断开：处理链在服务端后台执行，轮询到旧 id 被拒为止。
        context
            .client()
            .client_delete(ClientDeletePO {
                client_id: created.client_id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        let mut removed = false;
        for _ in 0..100 {
            if context
                .client()
                .client_post(ClientPostPO {
                    client_id: created.client_id.clone(),
                    kind: ClientPostKind::Ping,
                    tool_call_id: None,
                    decision: None,
                })
                .await
                .is_err()
            {
                removed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        ensure!(removed, "断开后旧 clientId 的心跳应被拒绝");

        // 三条固定 message：update / delete / occupancy 都按「未注册」拒绝。
        let message = post_error(
            context,
            CLIENT_POST.path,
            serde_json::json!({ "clientId": created.client_id.clone(), "kind": "PING" }),
        )
        .await?;
        ensure!(
            message == "客户端未注册，请重新连接",
            "已反注册的心跳应返回固定 message，实际：{message}"
        );
        let message = post_error(
            context,
            OCCUPANCY_CREATE.path,
            serde_json::json!({ "clientId": created.client_id.clone(), "sessionId": session.id.clone() }),
        )
        .await?;
        ensure!(
            message == "客户端未注册，请重新连接",
            "已反注册的占用请求应返回固定 message，实际：{message}"
        );
        let message = post_error(
            context,
            OCCUPANCY_DELETE.path,
            serde_json::json!({ "clientId": created.client_id.clone() }),
        )
        .await?;
        ensure!(
            message == "客户端未注册，请重新连接",
            "已反注册的释放请求应返回固定 message，实际：{message}"
        );
        let message = post_error(
            context,
            CLIENT_DELETE.path,
            serde_json::json!({ "clientId": created.client_id.clone() }),
        )
        .await?;
        ensure!(
            message == "客户端未注册，请重新连接",
            "已反注册的断开请求应返回固定 message，实际：{message}"
        );

        // 重新注册后可以再次占用同一会话，且是新的运行时会话。
        let reconnected = context
            .client()
            .client_create(ClientCreatePO {})
            .await
            .map_err(ClientError::from_call)?;
        let retaken = context
            .client()
            .occupancy_create(OccupancyCreatePO {
                client_id: reconnected.client_id.clone(),
                session_id: session.id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(
            retaken.runtime_session_id != occupied.runtime_session_id,
            "重新占用应是新的运行时会话：{retaken:?} == {occupied:?}"
        );
        context
            .client()
            .occupancy_delete(OccupancyDeletePO {
                client_id: reconnected.client_id,
            })
            .await
            .map_err(ClientError::from_call)?;

        Ok(vec![
            format!("断开：clientId={}", created.client_id),
            "旧 clientId 的 update / occupancy 请求一律 400 固定 message".to_string(),
            "重新注册后可再次占用同一会话，运行时会话是新的".to_string(),
        ])
    })
}

/// 进程级运行时状态：处理链只安装一次，`runtime.disconnect` 用它验证真实装配。
///
/// 与其它进程内用例共用同一份装配（`crate::util::support::installed`）：注入只能发生一次。
async fn global_state() -> Result<&'static RuntimeState> {
    Ok(installed().await?.runtime())
}

/// 一条「客户端 + 会话 + 占用」的固定搭配。
struct Fixture {
    client_id: String,
    session_id: String,
    runtime_session_id: String,
    root: PathBuf,
}

impl Fixture {
    /// 注册客户端、建会话并建立占用（事件文件随之创建）。
    async fn new(state: &RuntimeState, root: &Path, client_id: &str, title: &str) -> Result<Self> {
        state.register(ClientRecord::connected(client_id));
        let service = session_service().await?;
        let session = service
            .create_session(ContractSessionCreatePO {
                title: Some(format!("{title}-{}", time::now_millis())),
                model: None,
            })
            .await
            .context("创建会话失败")?;
        let occupancy = state
            .acquire(ContractOccupancyCreatePO {
                client_id: client_id.to_string(),
                session_id: session.id.clone(),
            })
            .await
            .map_err(anyhow::Error::from)?;
        Ok(Self {
            client_id: client_id.to_string(),
            session_id: session.id,
            runtime_session_id: occupancy.runtime_session_id,
            root: root.to_path_buf(),
        })
    }

    /// 事件生产方的写入端。
    fn producer(&self, state: &RuntimeState) -> Result<mpsc::Sender<RunEvent>> {
        state.producer(&self.session_id).context("占用建立后应有运行时会话")
    }

    /// 本次运行时会话的事件文件路径。
    fn path(&self) -> PathBuf {
        journal::session_directory(&self.root, &self.session_id).join(format!("{}.jsonl", self.runtime_session_id))
    }
}

/// 构造一条事件；`seq` 由写者覆写，这里只填必需字段。
fn event(fixture: &Fixture, kind: RunEventKind) -> RunEvent {
    RunEvent {
        event: kind,
        seq: 0,
        run_id: format!("run-{}", fixture.runtime_session_id),
        session_id: fixture.session_id.clone(),
        created_at_ms: time::now_millis(),
        delta: None,
        message_id: None,
        status: None,
        error: None,
        input_tokens: None,
        output_tokens: None,
        tool_call_id: None,
        tool_name: None,
        tool_arguments: None,
        tool_status: None,
        tool_duration_ms: None,
        tool_round: None,
    }
}

/// 读出文件里已经写完整的事件行。
fn read_events(path: &Path) -> Vec<RunEvent> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<RunEvent>(line).ok())
        .collect()
}

/// 发送一个预期失败的 POST 请求，返回错误 message。
async fn post_error(context: &TestContext, path: &str, body: serde_json::Value) -> Result<String> {
    let response = framework_http::HttpClient::new()
        .post(
            framework_http::HttpRequest::new(format!("{}/{}", context.base_url().trim_end_matches('/'), path))
                .with_content_type("application/json")
                .with_body(body.to_string()),
        )
        .await
        .context("拒绝语义的请求失败")?;
    ensure!(
        response.status == 400,
        "已反注册的请求应返回 400，实际 {}",
        response.status
    );
    let error: ApiError = serde_json::from_slice(&response.body).context("错误响应不是有效的 ApiError")?;
    Ok(error.message)
}
