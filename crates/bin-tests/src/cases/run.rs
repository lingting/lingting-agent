//! 真实执行端到端用例（P2-026）：流式成功、三类失败、三条终止路径、并发与排队超时、事件一致性。
//!
//! 断言口径：每条用例都要能区分「Mock 行为」与「真实执行」——通过 `/mock/stats` 的连接计数、
//! 增量与完整回复的逐字相等、以及数据库终态与事件文件的对应关系来判断。
//!
//! 服务端形态按需求选择：只需关键字选场景的用例复用共享服务端；需要 `[agent]` 预算参数或
//! 需要独立 Mock 观测记录的用例自起服务端（`server::serve_once`）。

use std::future::Future;
use std::time::{Duration, Instant};

use agent_core::session as domain;
use anyhow::{Context, Result, anyhow, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{
    ApiClient, MessageListParams, MessageRole, MessageSendPO, RunCancelPO, RunEventKind, RunStatus,
};
use manager_agent::runtime::{ClientRecord, journal};
use tokio::time::sleep;
use types_core::runtime::OccupancyCreatePO;

use crate::case::{Case, CaseFuture, CaseKind};
use crate::mock::llm::{MockServer, observe};
use crate::util::context::TestContext;
use crate::util::flow::{self, Occupied};
use crate::util::server::{self, config};
use crate::util::support::{runtime_state, session_service, temp_directory};

/// 一次失败路径对外的脱敏文案（与 `agent-route` 的 `public_message()` 逐字一致）。
const AUTH_ERROR: &str = "调用模型失败：认证失败";
/// 上游 5xx 的脱敏文案。
const UPSTREAM_ERROR: &str = "调用模型失败：上游服务错误";
/// 空闲超时的脱敏文案。
const TIMEOUT_ERROR: &str = "调用模型失败：响应超时";
/// 排队超时的失败文案。
const QUEUE_TIMEOUT: &str = "执行排队超时";
/// 未注入执行端口时的失败文案（与 `manager-agent` 的 `REASON_NOT_ASSEMBLED` 逐字一致）。
const NOT_ASSEMBLED: &str = "执行器未装配";
/// 用户取消的原因文本（D-P2-029）。
const CANCEL_REASON: &str = "用户取消执行";
/// 断连的原因文本（沿用 P1.1 的固定文本）。
const DISCONNECT_REASON: &str = "客户端连接已断开";
/// 服务关闭的终止原因文本。
const SHUTDOWN_REASON: &str = "服务正在关闭";
/// 模型流空闲超时压到 1 秒：默认 60 秒会超过用例上限。
const TIMEOUT_EXTRA: &str = "[agent]\nllm_idle_timeout_ms = 1000\n";
/// 并发上限压到 1；排队超时留足余量，让第二个 Run 一定能等到许可。
const CONCURRENCY_EXTRA: &str = "[agent]\nmax_concurrent_runs = 1\nrun_queue_timeout_ms = 60000\n";
/// 并发上限压到 1 且排队超时压到 1 秒：用于触发「执行排队超时」。
const QUEUE_TIMEOUT_EXTRA: &str = "[agent]\nmax_concurrent_runs = 1\nrun_queue_timeout_ms = 1000\n";

/// 执行领域用例。
pub(crate) fn cases() -> Vec<Case> {
    let mut cases = vec![
        Case {
            name: "run.cancel",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: cancel,
        },
        Case {
            name: "run.concurrency",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: concurrency,
        },
        Case {
            name: "run.disconnect",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: disconnect,
        },
        Case {
            name: "run.failed_auth",
            kind: CaseKind::Shared,
            prep: &[],
            run: failed_auth,
        },
        Case {
            name: "run.failed_timeout",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: failed_timeout,
        },
        Case {
            name: "run.failed_upstream",
            kind: CaseKind::Shared,
            prep: &[],
            run: failed_upstream,
        },
        Case {
            name: "run.execute",
            kind: CaseKind::Shared,
            prep: &[],
            run: execute,
        },
        Case {
            name: "run.journal_consistency",
            kind: CaseKind::Shared,
            prep: &[],
            run: journal_consistency,
        },
        Case {
            name: "run.queue_timeout",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: queue_timeout,
        },
    ];
    // 关闭探针只在 debug 构建注册，关闭类用例靠它触发服务端优雅关闭。
    #[cfg(debug_assertions)]
    cases.push(Case {
        name: "run.shutdown",
        kind: CaseKind::SelfServed,
        prep: &[],
        run: shutdown,
    });
    cases
}

/// 用共享 Mock 与追加配置起一次服务端：只需要 `[agent]` 参数、不需要 Mock 观测记录的用例走这里。
async fn serve_with<T>(context: &TestContext, extra: &str, body: impl Future<Output = Result<T>>) -> Result<T> {
    let mock = server::shared_mock().await?;
    let text = config(mock.base_url(), extra);
    server::serve_once(context, &text, body).await
}

/// 认证失败：`run_failed` + 脱敏文案，无助手消息。
fn failed_auth(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { failed_body(context, "mock:auth", AUTH_ERROR).await })
}

/// 上游 5xx：与认证失败同一套断言。
fn failed_upstream(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { failed_body(context, "mock:upstream", UPSTREAM_ERROR).await })
}

/// 空闲超时：`agent.llm_idle_timeout_ms` 压到 1 秒，Mock 静默后由 Adapter 报超时。
fn failed_timeout(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        serve_with(
            context,
            TIMEOUT_EXTRA,
            failed_body(context, "mock:stall", TIMEOUT_ERROR),
        )
        .await
    })
}

/// 三类失败共用的断言：终态 `FAILED`、脱敏文案、无助手消息、事件文件以 `run_failed` 收尾。
async fn failed_body(context: &TestContext, content: &str, public_error: &str) -> Result<Vec<String>> {
    let scenario = content.trim_start_matches("mock:");
    let mut talk = Occupied::start(context, &format!("失败 {scenario} {}", context.token())).await?;
    let content = format!("{content} {}", context.token());
    let (sent, drained) = talk.say(context, &content).await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunFailed,
        "应以 run_failed 收尾，实际 {:?}",
        drained.terminal.event
    );
    ensure!(
        drained.terminal.status == Some(RunStatus::Failed),
        "run_failed 的状态应为 Failed，实际 {:?}",
        drained.terminal.status
    );
    ensure!(
        drained.terminal.error.as_deref() == Some(public_error),
        "错误文案应为脱敏固定文本 {public_error}，实际 {:?}",
        drained.terminal.error
    );

    let finished = flow::wait_run(context.client(), &sent.run.id).await?;
    ensure!(finished.status == RunStatus::Failed, "Run 终态应为 Failed");
    ensure!(
        finished.error.as_deref() == Some(public_error),
        "数据库里的 error 应为脱敏文案，实际 {:?}",
        finished.error
    );
    ensure!(finished.finished_at_ms.is_some(), "失败后应写 finishedAtMs");

    let listed = list_messages(context, &talk.session.id).await?;
    ensure!(
        listed.len() == 1,
        "失败的 Run 不应写助手消息，实际 {} 条消息",
        listed.len()
    );
    ensure!(listed[0].role == MessageRole::User, "唯一的一条应为用户消息");

    let journal = flow::wait_journal_terminal(context, &talk.session.id, &talk.occupancy.runtime_session_id).await?;
    ensure!(
        journal.last().map(|event| event.event) == Some(RunEventKind::RunFailed),
        "事件文件应以 run_failed 收尾"
    );
    ensure!(
        journal.iter().filter(|event| flow::is_terminal(event.event)).count() == 1,
        "一条 Run 只应有一个终态事件"
    );

    talk.release(context).await?;
    Ok(vec![
        format!("失败场景：{scenario}"),
        format!("run_failed：{public_error}"),
        format!("消息列表：{} 条（只有用户消息）", listed.len()),
        format!("事件文件：{} 条事件，以 run_failed 收尾", journal.len()),
    ])
}

/// 取消：数据库先落终态、执行方 drop 模型流并补 `run_cancelled`（P2-024 第 1 条）。
fn cancel(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let text = config(mock.base_url(), "");
        server::serve_once(context, &text, cancel_body(context, &mock)).await
    })
}

/// 取消用例主体：用 `mock:slow` 让流式持续足够久，确认首段增量之后再取消。
async fn cancel_body(context: &TestContext, mock: &MockServer) -> Result<Vec<String>> {
    let mut talk = Occupied::start(context, &format!("取消 {}", context.token())).await?;
    let content = format!("mock:slow 取消 {}", context.token());
    let sent = context
        .client()
        .message_create(MessageSendPO {
            session_id: talk.session.id.clone(),
            content,
        })
        .await
        .map_err(ClientError::from_call)?;

    let before = flow::wait_first_delta(&mut talk.stream, &sent.run.id).await?;
    ensure!(
        before.iter().any(|event| event.event == RunEventKind::RunStarted),
        "首段增量之前应已收到 run_started"
    );

    let cancelled = context
        .client()
        .run_cancel(RunCancelPO {
            run_id: sent.run.id.clone(),
        })
        .await
        .map_err(ClientError::from_call)?;
    ensure!(
        cancelled.status == RunStatus::Cancelled,
        "取消接口应直接返回 Cancelled，实际 {:?}",
        cancelled.status
    );
    ensure!(
        cancelled.error.as_deref() == Some(CANCEL_REASON),
        "取消原因应为 {CANCEL_REASON}，实际 {:?}",
        cancelled.error
    );

    let drained = flow::drain_run(&mut talk.stream, &sent.run.id).await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCancelled,
        "应以 run_cancelled 收尾，实际 {:?}",
        drained.terminal.event
    );
    ensure!(
        drained.terminal.error.as_deref() == Some(CANCEL_REASON),
        "run_cancelled 的原因应为 {CANCEL_REASON}，实际 {:?}",
        drained.terminal.error
    );

    let finished = flow::wait_run(context.client(), &sent.run.id).await?;
    ensure!(finished.status == RunStatus::Cancelled, "run/get 应为 Cancelled");
    ensure!(finished.finished_at_ms.is_some(), "取消后应写 finishedAtMs");
    let listed = list_messages(context, &talk.session.id).await?;
    ensure!(listed.len() == 1, "取消的 Run 不应写助手消息，实际 {} 条", listed.len());

    let journal = flow::wait_journal_terminal(context, &talk.session.id, &talk.occupancy.runtime_session_id).await?;
    ensure!(
        journal.last().map(|event| event.event) == Some(RunEventKind::RunCancelled),
        "事件文件应以 run_cancelled 收尾"
    );
    ensure!(
        journal.iter().filter(|event| flow::is_terminal(event.event)).count() == 1,
        "取消后不应再追加第二条终态事件"
    );

    // O-1 的实测结论：drop 模型流即关闭底层连接，Mock 侧记录到一次中断。
    let aborted = observe::wait_aborted(mock).await?;

    talk.release(context).await?;
    Ok(vec![
        format!("取消前已收到 {} 条事件", before.len()),
        format!("run/get：Cancelled，原因 {CANCEL_REASON}"),
        format!("Mock 侧连接中断计数：{aborted}"),
        format!("事件文件：{} 条事件，以 run_cancelled 收尾", journal.len()),
    ])
}

/// 断连：下行流断开 → 断连处理链停止运行时会话 → 在跑 Run 落终态（P2-024 第 2 条）。
fn disconnect(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let text = config(mock.base_url(), "");
        server::serve_once(context, &text, disconnect_body(context, &mock)).await
    })
}

/// 断连用例主体：收到首段增量后直接丢弃下行流，模拟客户端掉线。
async fn disconnect_body(context: &TestContext, mock: &MockServer) -> Result<Vec<String>> {
    let mut talk = Occupied::start(context, &format!("断连 {}", context.token())).await?;
    let content = format!("mock:slow 断连 {}", context.token());
    let sent = context
        .client()
        .message_create(MessageSendPO {
            session_id: talk.session.id.clone(),
            content,
        })
        .await
        .map_err(ClientError::from_call)?;
    flow::wait_first_delta(&mut talk.stream, &sent.run.id).await?;

    // 只丢弃事件流：服务端据此判定断连，清理交给断连处理链。
    let Occupied {
        session,
        occupancy,
        stream,
        ..
    } = talk;
    drop(stream);

    let finished = flow::wait_run(context.client(), &sent.run.id).await?;
    ensure!(
        finished.status == RunStatus::Cancelled,
        "断连后 Run 应落 Cancelled，实际 {:?}",
        finished.status
    );

    let journal = flow::wait_journal_terminal(context, &session.id, &occupancy.runtime_session_id).await?;
    let terminal = journal.last().context("事件文件应有终态事件")?;
    ensure!(
        terminal.event == RunEventKind::RunCancelled,
        "断连的终态事件应为 run_cancelled，实际 {:?}",
        terminal.event
    );
    ensure!(
        terminal.error.as_deref() == Some(DISCONNECT_REASON),
        "断连原因应为 {DISCONNECT_REASON}，实际 {:?}",
        terminal.error
    );
    ensure!(
        journal.iter().filter(|event| flow::is_terminal(event.event)).count() == 1,
        "断连不应重复追加终态事件"
    );

    let aborted = observe::wait_aborted(mock).await?;

    Ok(vec![
        format!("会话：{}，下行流已断开", session.id),
        "run/get：Cancelled".to_string(),
        format!(
            "事件文件：{} 条事件，以 run_cancelled / {DISCONNECT_REASON} 收尾",
            journal.len()
        ),
        format!("Mock 侧连接中断计数：{aborted}"),
    ])
}

/// 服务关闭：执行方发 `server_shutdown`，随后写者 flush 收尾（P2-024 第 3 条）。
#[cfg(debug_assertions)]
fn shutdown(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = server::shared_mock().await?;
        let text = config(mock.base_url(), "");
        server::serve_once(context, &text, shutdown_body(context)).await
    })
}

/// 关闭用例主体：收到首段增量后触发关闭探针，断言事件文件的终态是 `server_shutdown`。
#[cfg(debug_assertions)]
async fn shutdown_body(context: &TestContext) -> Result<Vec<String>> {
    let mut talk = Occupied::start(context, &format!("关闭 {}", context.token())).await?;
    let content = format!("mock:slow 关闭 {}", context.token());
    let sent = context
        .client()
        .message_create(MessageSendPO {
            session_id: talk.session.id.clone(),
            content,
        })
        .await
        .map_err(ClientError::from_call)?;
    flow::wait_first_delta(&mut talk.stream, &sent.run.id).await?;

    context
        .client()
        .probe_shutdown()
        .await
        .map_err(ClientError::from_call)?;

    let journal = flow::wait_journal_terminal(context, &talk.session.id, &talk.occupancy.runtime_session_id).await?;
    let terminal = journal.last().context("事件文件应有终态事件")?;
    ensure!(
        terminal.event == RunEventKind::ServerShutdown,
        "关闭的终态事件应为 server_shutdown，实际 {:?}",
        terminal.event
    );
    ensure!(
        terminal.status == Some(RunStatus::Cancelled),
        "server_shutdown 应带已取消状态，实际 {:?}",
        terminal.status
    );
    ensure!(
        terminal.error.as_deref() == Some(SHUTDOWN_REASON),
        "关闭原因应为 {SHUTDOWN_REASON}，实际 {:?}",
        terminal.error
    );
    ensure!(
        journal.iter().filter(|event| flow::is_terminal(event.event)).count() == 1,
        "关闭不应重复追加终态事件"
    );
    ensure!(
        journal.iter().all(|event| event.event != RunEventKind::RunCompleted),
        "关闭中的 Run 不应出现 run_completed"
    );

    Ok(vec![
        format!("会话：{}，首段增量后触发关闭", talk.session.id),
        format!("事件文件：{} 条事件，以 server_shutdown 收尾", journal.len()),
        format!("终止原因：{SHUTDOWN_REASON}"),
    ])
}

/// 全局并发上限：`max_concurrent_runs = 1` 时两个会话的 Run 不同时进入 `RUNNING`（P2-025）。
fn concurrency(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { serve_with(context, CONCURRENCY_EXTRA, concurrency_body(context)).await })
}

/// 并发用例主体：两个会话同时发慢速消息，采样两个 Run 的状态直到都抵终态。
async fn concurrency_body(context: &TestContext) -> Result<Vec<String>> {
    let mut first = Occupied::start(context, &format!("并发 A {}", context.token())).await?;
    let mut second = Occupied::start(context, &format!("并发 B {}", context.token())).await?;
    // 长内容 + 慢速分块：单个 Run 持续数秒，采样窗口足够大。
    let content = format!("mock:slow 并发采样 {}", "内容".repeat(30));
    let a = context
        .client()
        .message_create(MessageSendPO {
            session_id: first.session.id.clone(),
            content: content.clone(),
        })
        .await
        .map_err(ClientError::from_call)?;
    let b = context
        .client()
        .message_create(MessageSendPO {
            session_id: second.session.id.clone(),
            content,
        })
        .await
        .map_err(ClientError::from_call)?;

    let deadline = Instant::now() + flow::RUN_TIMEOUT;
    let mut max_running = 0;
    let mut samples = 0;
    loop {
        let a_status = run_status(context, &a.run.id).await?;
        let b_status = run_status(context, &b.run.id).await?;
        let running = [a_status, b_status]
            .iter()
            .filter(|status| **status == RunStatus::Running)
            .count();
        max_running = max_running.max(running);
        samples += 1;
        let done = is_terminal(a_status) && is_terminal(b_status);
        ensure!(
            done || Instant::now() < deadline,
            "两个 Run 在 {} 秒内未都抵终态",
            flow::RUN_TIMEOUT.as_secs()
        );
        if done {
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    ensure!(
        max_running == 1,
        "max_concurrent_runs = 1 时最多一个 Run 处于 RUNNING，实际观测到 {max_running}"
    );

    for run_id in [&a.run.id, &b.run.id] {
        let finished = flow::wait_run(context.client(), run_id).await?;
        ensure!(
            finished.status == RunStatus::Completed,
            "两个 Run 都应以 Completed 收尾，实际 {:?}",
            finished.status
        );
    }
    // 收完各自的事件流，顺带确认上行事件没有丢终态。
    flow::drain_run(&mut first.stream, &a.run.id).await?;
    flow::drain_run(&mut second.stream, &b.run.id).await?;
    first.release(context).await?;
    second.release(context).await?;

    // 并发写入不撞锁：写锁争用与 panic 都改由进程内计数给出（S-7、L-5），不读日志文本。
    let metrics = server::capture(context.base_url())
        .await
        .map_err(|reason| anyhow!("未采集服务端快照（{reason}），无法判定并发期是否撞锁"))?;
    ensure!(
        metrics.database.errors == 0,
        "并发运行期出现数据库写锁争用：database.errors = {}",
        metrics.database.errors
    );
    ensure!(
        metrics.audit.panics == 0,
        "并发运行期出现 panic：audit.panics = {}",
        metrics.audit.panics
    );

    Ok(vec![
        format!("采样 {samples} 次，同时 RUNNING 的最大值：{max_running}"),
        "两个 Run 都抵 Completed".to_string(),
        "配置：max_concurrent_runs = 1".to_string(),
        "服务端快照：database.errors = 0、audit.panics = 0".to_string(),
    ])
}

/// 排队超时：`run_queue_timeout_ms` 压到 1 秒，等不到全局许可的 Run 落 `FAILED`（P2-025）。
fn queue_timeout(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { serve_with(context, QUEUE_TIMEOUT_EXTRA, queue_timeout_body(context)).await })
}

/// 排队超时用例主体：A 占住唯一的并发位，B 在 1 秒后落「执行排队超时」。
async fn queue_timeout_body(context: &TestContext) -> Result<Vec<String>> {
    let holder = Occupied::start(context, &format!("占位 {}", context.token())).await?;
    let waiting = Occupied::start(context, &format!("排队 {}", context.token())).await?;
    let slow = format!("mock:slow 占位 {}", "内容".repeat(30));
    let a = context
        .client()
        .message_create(MessageSendPO {
            session_id: holder.session.id.clone(),
            content: slow,
        })
        .await
        .map_err(ClientError::from_call)?;
    // 等 A 真的进入 RUNNING，再让 B 排队，避免 B 抢到唯一的许可。
    let deadline = Instant::now() + flow::RUN_TIMEOUT;
    while run_status(context, &a.run.id).await? != RunStatus::Running {
        ensure!(Instant::now() < deadline, "占位 Run 在超时内未进入 RUNNING");
        sleep(Duration::from_millis(50)).await;
    }

    let b = context
        .client()
        .message_create(MessageSendPO {
            session_id: waiting.session.id.clone(),
            content: format!("mock:slow 排队 {}", "内容".repeat(30)),
        })
        .await
        .map_err(ClientError::from_call)?;
    let failed = flow::wait_run(context.client(), &b.run.id).await?;
    ensure!(
        failed.status == RunStatus::Failed,
        "排队超时的 Run 应落 Failed，实际 {:?}",
        failed.status
    );
    ensure!(
        failed.error.as_deref() == Some(QUEUE_TIMEOUT),
        "失败原因应为 {QUEUE_TIMEOUT}，实际 {:?}",
        failed.error
    );
    let journal =
        flow::wait_journal_terminal(context, &waiting.session.id, &waiting.occupancy.runtime_session_id).await?;
    ensure!(
        journal.last().map(|event| event.event) == Some(RunEventKind::RunFailed),
        "排队超时的 Run 事件文件应以 run_failed 收尾"
    );

    // 占位 Run 不受影响，仍然正常完成。
    let completed = flow::wait_run(context.client(), &a.run.id).await?;
    ensure!(
        completed.status == RunStatus::Completed,
        "占位 Run 应正常完成，实际 {:?}",
        completed.status
    );

    holder.release(context).await?;
    waiting.release(context).await?;
    Ok(vec![
        format!("占位 Run：{} 完成", a.run.id),
        format!("排队 Run：{} 落 Failed，原因 {QUEUE_TIMEOUT}", b.run.id),
        "配置：max_concurrent_runs = 1、run_queue_timeout_ms = 1000".to_string(),
    ])
}

/// 事件一致性：下行事件序列与事件文件序列一一对应（`event` 与 `seq` 都一致）。
fn journal_consistency(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { journal_consistency_body(context).await })
}

/// 事件一致性用例主体：一次普通回复，比对同一 Run 的两条来源。
async fn journal_consistency_body(context: &TestContext) -> Result<Vec<String>> {
    let mut talk = Occupied::start(context, &format!("一致性 {}", context.token())).await?;
    let (sent, drained) = talk.say(context, &format!("事件一致性 {}", context.token())).await?;
    let journal = flow::wait_journal_terminal(context, &talk.session.id, &talk.occupancy.runtime_session_id).await?;
    let written: Vec<&lib_bin_cli::sdk::RunEvent> =
        journal.iter().filter(|event| event.run_id == sent.run.id).collect();
    ensure!(
        written.len() == drained.events.len(),
        "下行事件 {} 条，事件文件 {} 条",
        drained.events.len(),
        written.len()
    );
    for (index, (live, stored)) in drained.events.iter().zip(written.iter()).enumerate() {
        ensure!(
            live.event == stored.event && live.seq == stored.seq,
            "第 {index} 条事件不一致：下行 {:?}#{}，文件 {:?}#{}",
            live.event,
            live.seq,
            stored.event,
            stored.seq
        );
    }
    let seqs: Vec<i64> = written.iter().map(|event| event.seq).collect();
    ensure!(
        seqs == (0..written.len() as i64).collect::<Vec<_>>(),
        "事件文件内的 seq 应从 0 连续递增：{seqs:?}"
    );

    talk.release(context).await?;
    Ok(vec![
        format!("同一 Run：下行与事件文件各 {} 条，逐条一致", written.len()),
        format!("seq：0..{}", written.len() as i64 - 1),
    ])
}

/// 读一次 Run 的状态。
async fn run_status(context: &TestContext, run_id: &str) -> Result<RunStatus> {
    let run = context
        .client()
        .run_get(lib_bin_cli::sdk::RunGetParams {
            run_id: run_id.to_string(),
        })
        .await
        .map_err(ClientError::from_call)?;
    Ok(run.status)
}

/// 是否已抵终态。
fn is_terminal(status: RunStatus) -> bool {
    matches!(status, RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled)
}

/// 域状态是否已抵终态（进程内用例直接用域类型，不经过 SDK）。
fn is_domain_terminal(status: domain::RunStatus) -> bool {
    status.is_terminal()
}

/// 读取会话消息列表。
async fn list_messages(context: &TestContext, session_id: &str) -> Result<Vec<lib_bin_cli::sdk::Message>> {
    let messages = context
        .client()
        .message_list(MessageListParams {
            session_id: session_id.to_string(),
        })
        .await
        .map_err(ClientError::from_call)?;
    Ok(messages)
}

/// 执行端口装配（G-4）：未注入执行端口时 Run 落 `FAILED`「执行器未装配」。
///
/// 「真实 LLM 调用产出事件」的一半由 `chat.multiturn` 与其余 `run.*` 覆盖（共享服务端已注入端口）；
/// 这里补另一半：本进程的装配从不注入执行端口，会话被占用后队列取到 Run 直接判失败。
fn execute(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { execute_body(context).await })
}

/// 用例主体：建会话 → 发消息（Run 停在 `QUEUED`）→ 占用（队列启动）→ 断言终态与事件文件。
async fn execute_body(context: &TestContext) -> Result<Vec<String>> {
    let root = temp_directory("execute")?;
    let state = runtime_state(&root).await?;
    let client_id = format!("execute-{}", context.token());
    state.register(ClientRecord::connected(client_id.clone()));
    let service = session_service().await?;
    let session = service
        .create_session(domain::SessionCreatePO {
            title: Some(format!("执行端口 {}", context.token())),
            model: None,
        })
        .await
        .context("创建会话失败")?;

    // 未占用：Run 停在 QUEUED（P1.1 的触发点是「会话被占用 + 消息创建」）。
    let sent = service
        .send_message(domain::MessageSendPO {
            session_id: session.id.clone(),
            content: format!("未装配执行器 {}", context.token()),
        })
        .await
        .context("发送消息失败")?;
    ensure!(
        sent.run.status == domain::RunStatus::Queued,
        "未占用时 Run 应为 Queued，实际 {:?}",
        sent.run.status
    );

    // 建立占用即启动执行队列：driver 为空，Run 直接落终态。
    let occupancy = state
        .acquire(OccupancyCreatePO {
            client_id: client_id.clone(),
            session_id: session.id.clone(),
        })
        .await
        .map_err(anyhow::Error::from)?;

    let deadline = Instant::now() + flow::RUN_TIMEOUT;
    let mut settled = None;
    while Instant::now() < deadline {
        let run = service
            .get_run(domain::RunGetParams {
                run_id: sent.run.id.clone(),
            })
            .await
            .context("查询 Run 失败")?;
        if is_domain_terminal(run.status) {
            settled = Some(run);
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    let finished = settled.context("执行队列未在超时内处理 QUEUED Run")?;
    ensure!(
        finished.status == domain::RunStatus::Failed,
        "未装配执行器时 Run 应落 Failed，实际 {:?}",
        finished.status
    );
    ensure!(
        finished.error.as_deref() == Some(NOT_ASSEMBLED),
        "失败原因应为 {NOT_ASSEMBLED}，实际 {:?}",
        finished.error
    );
    ensure!(finished.finished_at_ms.is_some(), "失败后应写 finishedAtMs");

    let messages = service
        .list_messages(domain::MessageListParams {
            session_id: session.id.clone(),
        })
        .await
        .context("读取消息失败")?;
    ensure!(
        messages.len() == 1 && messages[0].role == domain::MessageRole::User,
        "未装配执行器的 Run 不应写助手消息，实际 {} 条消息",
        messages.len()
    );

    // 事件文件（写者先落盘再转发）：以 `run_failed` 收尾。
    let path = journal::session_directory(&root, &session.id).join(format!("{}.jsonl", occupancy.runtime_session_id));
    let journal_deadline = Instant::now() + Duration::from_secs(10);
    let mut events = flow::read_journal(&path).unwrap_or_default();
    while events.last().is_none_or(|event| !flow::is_terminal(event.event)) {
        ensure!(
            Instant::now() < journal_deadline,
            "事件文件未在超时内以终态收尾：{}",
            path.display()
        );
        sleep(Duration::from_millis(20)).await;
        events = flow::read_journal(&path).unwrap_or_default();
    }
    let terminal = events.last().context("事件文件应有终态事件")?;
    ensure!(
        terminal.event == RunEventKind::RunFailed,
        "事件文件应以 run_failed 收尾，实际 {:?}",
        terminal.event
    );

    state.release_occupancy(&client_id).await.map_err(anyhow::Error::from)?;
    std::fs::remove_dir_all(&root).ok();

    Ok(vec![
        "真实执行的一半：chat.multiturn / run.* 已注入执行端口".to_string(),
        format!("未装配执行器：Run {} 落 Failed，原因 {NOT_ASSEMBLED}", sent.run.id),
        format!("事件文件：{} 条事件，以 run_failed 收尾", events.len()),
    ])
}
