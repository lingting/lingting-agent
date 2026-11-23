//! 持久化用例：重启不丢数据、迁移可重复、残留状态被自愈、占用不恢复（P2-014）。
//!
//! 用例自管服务端（`CaseKind::SelfServed`）：同一 `AGENT_HOME` 与端口上先启动、停掉、再启动，
//! 才能断言「首次运行写入的数据在第二次启动后仍在」与「重启把残留 Run 自愈成终态」。
//! 断言一律通过生成的 SDK，不直接读数据库。

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use lib_bin_cli::client::{ClientError, EventStream};
use lib_bin_cli::sdk::{
    ApiClient, ClientEventKind, ClientStreamParams, MessageListParams, MessageRole, MessageSendPO, OccupancyGetParams,
    Run, RunGetParams, RunStatus, SessionCreatePO, SessionSummary,
};

use crate::case::{Case, CaseFuture, CaseKind, CaseOutcome};
use crate::util::context::TestContext;
use crate::util::flow::{self, occupy, register};
use crate::util::server::Server;
/// 等待 Run 抵终态的上限：Mock 生产方按固定间隔吐增量，留足余量。
const RUN_TIMEOUT: Duration = Duration::from_secs(30);
/// 启动自愈的原因文本：执行中的 Run 落 `FAILED`（与 `manager-agent` 的 `HEAL_RUNNING` 逐字一致）。
const HEAL_RUNNING: &str = "服务重启，执行中的 Run 已终止";

/// 持久化领域用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "persist.session",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: session_restore,
        },
        Case {
            name: "persist.migration",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: migration_repeat,
        },
        Case {
            name: "persist.queued_heal",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: queued_heal,
        },
        Case {
            name: "persist.occupancy_reset",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: occupancy_reset,
        },
        Case {
            name: "persist.running_heal",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: running_heal,
        },
    ]
}

/// 重启后会话与消息完整恢复，顺序与内容一致、消息数正确。
fn session_restore(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { session_restore_body(context).await })
}

/// 同一 data 目录连续启动两次：第二次不报错、数据保留、日志无错误。
fn migration_repeat(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { migration_repeat_body(context).await })
}

/// 未被占用的会话里 Run 停在 `QUEUED`，重启后被自愈成 `CANCELLED` 且带结束时间。
fn queued_heal(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { queued_heal_body(context).await })
}

/// 占用不随重启恢复；重新注册 + 重新占用后可继续发消息。
fn occupancy_reset(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { occupancy_reset_body(context).await })
}

/// 执行中的 Run 在服务被强杀后自愈成 `FAILED`，且终态不被后续重启覆盖（V-2、D-4）。
fn running_heal(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { running_heal_body(context).await })
}

/// 用例主体：`mock:slow` 让 Run 停在 `RUNNING`，强杀服务端后重启，核对自愈结果。
async fn running_heal_body(context: &TestContext) -> Result<CaseOutcome> {
    let port = context.port()?;
    let server = start(context).await?;

    let session = create_session(context, "运行中自愈").await?;
    let client_id = register(context).await?;
    occupy(context, &client_id, &session.id).await?;
    let mut stream = subscribe(context, &client_id).await?;
    let sent = send(context, &session.id, &format!("mock:slow 运行中 {}", context.token())).await?;
    // 慢速分块让 Run 维持 RUNNING：收到首段增量即说明执行已开始。
    flow::wait_first_delta(&mut stream, &sent.run.id).await?;
    let before = run_of(context, &sent.run.id).await?;
    ensure!(
        before.status == RunStatus::Running,
        "强杀前 Run 应处于 Running，实际 {:?}",
        before.status
    );
    ensure!(before.finished_at_ms.is_none(), "执行中的 Run 不应有结束时间");
    drop(stream);

    // `restart` 走 `start_kill`：不是优雅关闭，进程内的执行状态随进程一起丢失。
    let server = restarted(context, server, port).await?;
    let healed = run_of(context, &sent.run.id).await?;
    ensure!(
        healed.status == RunStatus::Failed,
        "重启后执行中的 Run 应为 Failed，实际 {:?}",
        healed.status
    );
    ensure!(
        healed.error.as_deref() == Some(HEAL_RUNNING),
        "自愈原因应为 {HEAL_RUNNING}，实际 {:?}",
        healed.error
    );
    let finished_at_ms = healed.finished_at_ms.context("自愈后的 Run 应有结束时间")?;
    ensure!(
        !has_assistant_message(context, &session.id).await?,
        "自愈不应补写助手消息"
    );

    // 第二次重启：已落终态的 Run 不被自愈覆盖（D-4）。
    let _server = restarted(context, server, port).await?;
    let again = run_of(context, &sent.run.id).await?;
    ensure!(
        again.status == RunStatus::Failed && again.error.as_deref() == Some(HEAL_RUNNING),
        "终态 Run 不应被再次自愈改写：{again:?}"
    );
    ensure!(
        again.finished_at_ms == Some(finished_at_ms),
        "终态 Run 的结束时间不应被改写：{:?} != {finished_at_ms}",
        again.finished_at_ms
    );

    // 异常不留 RUNNING，且服务端日志无 panic 行（L-5）。
    let log = std::fs::read_to_string(context.home().join("server.log")).unwrap_or_default();
    ensure!(!log.contains("panic"), "服务端日志出现 panic 行");
    let log_lines = log.lines().count();

    Ok(vec![
        format!("强杀前 Run 状态 {:?}，无结束时间", before.status),
        format!("重启后 Run 状态 {:?}，原因 {HEAL_RUNNING}", healed.status),
        "第二次重启后终态与结束时间保持不变，无助手消息".to_string(),
        format!("服务端日志 {log_lines} 行，无 panic"),
    ])
}

/// 用例主体：建会话 → 占用 → 发消息跑完 → 重启 → 核对会话与消息。
async fn session_restore_body(context: &TestContext) -> Result<CaseOutcome> {
    let port = context.port()?;
    let server = start(context).await?;

    let content = format!("持久化消息 {}", context.token());
    let session = create_session(context, "持久化会话").await?;
    let client_id = register(context).await?;
    occupy(context, &client_id, &session.id).await?;
    let mut stream = subscribe(context, &client_id).await?;
    let sent = send(context, &session.id, &content).await?;
    wait_completed(&mut stream, &sent.run.id).await?;
    let before = list_messages(context, &session.id).await?;
    ensure!(before.len() == 2, "首次运行应有 2 条消息，实际 {}", before.len());
    ensure!(before[0].role == MessageRole::User, "首条消息应为用户消息");
    ensure!(before[1].role == MessageRole::Assistant, "第二条消息应为助手消息");
    let reply = before[1].content.clone();
    ensure!(!reply.is_empty(), "助手消息内容为空");

    // 停掉服务端（进程退出即断连，占用与运行时会话都不保留），再按同一 data 目录重启。
    // 重启后的进程必须留到用例结束（`kill_on_drop`），否则后续请求会直接断连。
    let _server = restarted(context, server, port).await?;
    let after = list_messages(context, &session.id).await?;
    ensure!(
        after.len() == before.len(),
        "重启后消息数应一致：{} != {}",
        after.len(),
        before.len()
    );
    for (index, (before_message, after_message)) in before.iter().zip(after.iter()).enumerate() {
        ensure!(
            before_message.id == after_message.id,
            "第 {index} 条消息 ID 不一致：{} != {}",
            after_message.id,
            before_message.id
        );
        ensure!(
            before_message.content == after_message.content,
            "第 {index} 条消息内容不一致"
        );
        ensure!(before_message.role == after_message.role, "第 {index} 条消息角色不一致");
    }

    let summary = find_session(context, &session.id).await?;
    ensure!(
        summary.message_count == 2,
        "会话消息数应为 2，实际 {}",
        summary.message_count
    );
    ensure!(
        summary.title == session.title,
        "会话标题应保持一致：{} != {}",
        summary.title,
        session.title
    );

    Ok(vec![
        format!(
            "重启后消息顺序与内容一致（{} 条，首条用户消息 `{content}`）",
            after.len()
        ),
        format!("会话摘要 messageCount = {}", summary.message_count),
        "占用不随重启恢复：重新调用会话接口不需要先注册客户端".to_string(),
    ])
}

/// 用例主体：连续两次启动同一 data 目录。
async fn migration_repeat_body(context: &TestContext) -> Result<CaseOutcome> {
    let port = context.port()?;
    let server = start(context).await?;
    let session = create_session(context, "迁移会话").await?;
    let _server = restarted(context, server, port).await?;

    let summary = find_session(context, &session.id).await?;
    ensure!(summary.id == session.id, "第二次启动后会话应保留：{}", summary.id);

    let log = std::fs::read_to_string(context.home().join("server.log")).unwrap_or_default();
    let errors: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("ERROR") || line.contains("database is locked") || line.contains("SQLITE_BUSY"))
        .collect();
    ensure!(
        errors.is_empty(),
        "第二次启动不应出现错误日志（{}）：{:?}",
        errors.len(),
        errors
    );

    Ok(vec![
        "同一 data 目录连续启动两次：第二次成功且数据保留".to_string(),
        format!("第二次启动日志无错误（已扫描 {} 行日志）", log.lines().count()),
    ])
}

/// 用例主体：`QUEUED` 的 Run 在重启后被自愈成 `CANCELLED`。
async fn queued_heal_body(context: &TestContext) -> Result<CaseOutcome> {
    let port = context.port()?;
    let server = start(context).await?;

    let session = create_session(context, "残留会话").await?;
    // 不建立占用：Run 停在 QUEUED，Mock 生产方永远不会被触发。
    let sent = send(context, &session.id, &format!("残留消息 {}", context.token())).await?;
    ensure!(
        sent.run.status == RunStatus::Queued,
        "未占用时 Run 应为 Queued，实际 {:?}",
        sent.run.status
    );
    let before = run_of(context, &sent.run.id).await?;
    ensure!(before.finished_at_ms.is_none(), "未结束的 Run 不应有结束时间");
    ensure!(
        !has_assistant_message(context, &session.id).await?,
        "未执行的 Run 不应产生助手消息"
    );

    let _server = restarted(context, server, port).await?;
    let healed = run_of(context, &sent.run.id).await?;
    ensure!(
        healed.status == RunStatus::Cancelled,
        "重启后残留 Run 应为 Cancelled，实际 {:?}",
        healed.status
    );
    ensure!(healed.finished_at_ms.is_some(), "自愈后的 Run 应有结束时间：{healed:?}");
    ensure!(
        healed.error.as_deref().is_some_and(|error| !error.is_empty()),
        "自愈后的 Run 应带可读原因：{healed:?}"
    );
    ensure!(
        !has_assistant_message(context, &session.id).await?,
        "自愈不应补写助手消息"
    );

    Ok(vec![
        format!("重启前 Run 状态 {:?}，无结束时间", before.status),
        format!(
            "重启后 Run 状态 {:?}，原因 {}",
            healed.status,
            healed.error.clone().unwrap_or_default()
        ),
    ])
}

/// 用例主体：重启后占用不恢复，重新注册 + 重新占用可继续发消息。
async fn occupancy_reset_body(context: &TestContext) -> Result<CaseOutcome> {
    let port = context.port()?;
    let server = start(context).await?;

    let session = create_session(context, "占用恢复会话").await?;
    let client_id = register(context).await?;
    occupy(context, &client_id, &session.id).await?;

    let _server = restarted(context, server, port).await?;

    let missing = context
        .client()
        .occupancy_get(OccupancyGetParams {
            session_id: session.id.clone(),
        })
        .await
        .expect_err("重启后占用不应恢复");
    let error = ClientError::from_call(missing);
    ensure!(
        format!("{error}").contains("会话未被占用"),
        "重启后应返回固定 message，实际：{error}"
    );

    // 旧 clientId 不恢复：重新注册后重新占用同一会话，并继续发消息。
    let reconnected = register(context).await?;
    ensure!(reconnected != client_id, "重启后重新注册应得到新的 clientId");
    occupy(context, &reconnected, &session.id).await?;
    let mut stream = subscribe(context, &reconnected).await?;
    let sent = send(context, &session.id, &format!("恢复后消息 {}", context.token())).await?;
    wait_completed(&mut stream, &sent.run.id).await?;
    let messages = list_messages(context, &session.id).await?;
    ensure!(
        messages.len() == 2,
        "重新占用后应完成一轮对话（2 条消息），实际 {}",
        messages.len()
    );

    Ok(vec![
        "重启后 occupancy/get 返回 404 会话未被占用".to_string(),
        "重新注册得到新 clientId，重新占用后可继续发消息并跑完".to_string(),
    ])
}

/// 启动服务端（同一 `AGENT_HOME` 与端口）。
async fn start(context: &TestContext) -> Result<Server> {
    Server::start(context.port()?, context.home().to_path_buf())
        .await
        .with_context(|| format!("启动服务端失败：{}", context.home().display()))
}

/// 停掉当前服务端并在同一 `AGENT_HOME` 与端口上重启。
async fn restarted(context: &TestContext, server: Server, port: u16) -> Result<Server> {
    let server = server.restart(port).await.context("重启服务端失败")?;
    ensure!(
        server.base_url() == context.base_url(),
        "重启应复用同一端口：{} != {}",
        server.base_url(),
        context.base_url()
    );
    Ok(server)
}

/// 建会话。
async fn create_session(context: &TestContext, title: &str) -> Result<SessionSummary> {
    context
        .client()
        .session_create(SessionCreatePO {
            title: Some(format!("{title} {}", context.token())),
            model: None,
        })
        .await
        .map_err(ClientError::from_call)
        .map_err(anyhow::Error::from)
}

/// 建立客户端下行流。
async fn subscribe(context: &TestContext, client_id: &str) -> Result<EventStream> {
    context
        .client()
        .subscribe_client(ClientStreamParams {
            client_id: client_id.to_string(),
        })
        .await
        .map_err(anyhow::Error::from)
}

/// 发送消息。
async fn send(context: &TestContext, session_id: &str, content: &str) -> Result<lib_bin_cli::sdk::MessageSendVO> {
    context
        .client()
        .message_create(MessageSendPO {
            session_id: session_id.to_string(),
            content: content.to_string(),
        })
        .await
        .map_err(ClientError::from_call)
        .map_err(anyhow::Error::from)
}

/// 列出会话消息。
async fn list_messages(context: &TestContext, session_id: &str) -> Result<Vec<lib_bin_cli::sdk::Message>> {
    context
        .client()
        .message_list(MessageListParams {
            session_id: session_id.to_string(),
        })
        .await
        .map_err(ClientError::from_call)
        .map_err(anyhow::Error::from)
}

/// 查询 Run。
async fn run_of(context: &TestContext, run_id: &str) -> Result<Run> {
    context
        .client()
        .run_get(RunGetParams {
            run_id: run_id.to_string(),
        })
        .await
        .map_err(ClientError::from_call)
        .map_err(anyhow::Error::from)
}

/// 会话列表里找到指定会话。
async fn find_session(context: &TestContext, session_id: &str) -> Result<SessionSummary> {
    let sessions = context.client().session_list().await.map_err(ClientError::from_call)?;
    sessions
        .into_iter()
        .find(|session| session.id == session_id)
        .with_context(|| format!("会话列表中找不到 {session_id}"))
}

/// 会话里是否已有助手消息。
async fn has_assistant_message(context: &TestContext, session_id: &str) -> Result<bool> {
    Ok(list_messages(context, session_id)
        .await?
        .iter()
        .any(|message| message.role == MessageRole::Assistant))
}

/// 消费下行流直到指定 Run 完成。
async fn wait_completed(stream: &mut EventStream, run_id: &str) -> Result<()> {
    let deadline = Instant::now() + RUN_TIMEOUT;
    while Instant::now() < deadline {
        let Some(event) = stream.recv().await.map_err(anyhow::Error::from)? else {
            bail!("下行流已关闭，未收到 run_completed");
        };
        if event.event != ClientEventKind::SessionEvent {
            continue;
        }
        let Some(run_event) = event.session_event else {
            continue;
        };
        if run_event.run_id != run_id {
            continue;
        }
        match run_event.event {
            lib_bin_cli::sdk::RunEventKind::RunCompleted => return Ok(()),
            lib_bin_cli::sdk::RunEventKind::RunFailed => {
                bail!("Run 失败：{}", run_event.error.unwrap_or_default())
            }
            lib_bin_cli::sdk::RunEventKind::RunCancelled => {
                bail!("Run 被取消：{}", run_event.error.unwrap_or_default())
            }
            _ => {}
        }
    }
    bail!("等待 Run {run_id} 完成超时（{} 秒）", RUN_TIMEOUT.as_secs())
}
