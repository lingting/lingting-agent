//! `resume.retry`（P6A-017 / D-P6-012）：Run 级重试有上限、可复算，不可重试分类不消耗预算。
//!
//! 证据只有两个来源：`/mock/stats` 的上游请求计数（模型确实被重新请求了几次）与
//! `GET api/probe/metrics` 的限流段（`limit.retries`）。**不读日志、不读事件文件**。
//!
//! 为什么用 `mock:upstream`：503 属「上游瞬时错误」，是可重试分类里最稳定的一种；Mock 每次都
//! 返回 503，因此「1 次失败 + N 次重试」的请求次数是可复算的整数，不需要靠 sleep 猜窗口。
//!
//! `run_max_retries = 0`（缺省）的行为不在本用例重复：它由既有 `run.failed_upstream` 覆盖，
//! 那条用例断言的是同一句上游脱敏文案，说明「不重试」时改动前后逐字段一致。

use anyhow::{Result, anyhow, ensure};
use lib_bin_cli::sdk::{ApiClient, RunEventKind, RunGetParams, RunStatus};

use crate::case::CaseFuture;
use crate::mock::llm::{MockServer, observe};
use crate::util::context::TestContext;
use crate::util::flow::{self, Reopened};
use crate::util::server::{Server, config};

/// 配置的重试次数：`1 + RETRIES` 次上游请求（一次失败 + 每次重试各一次）。
const RETRIES: u64 = 2;
/// 重试用尽的固定文案（与 `agent-runtime` 的 `error::RETRY_EXHAUSTED` 逐字一致）。
const RETRY_EXHAUSTED: &str = "模型调用重试次数已用尽";
/// 认证失败的脱敏文案：不可重试分类沿用原始文案（`agent-route` 的 `public_message`）。
const AUTH_ERROR: &str = "调用模型失败：认证失败";

/// 用例入口。
pub(super) fn case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { require(context).await })
}

async fn require(context: &TestContext) -> Result<Vec<String>> {
    // 需要独立的 Mock 观测记录与快照：服务端由用例自己启停。
    let mock = MockServer::start().await?;
    let extra = format!("[agent]\nrun_max_retries = {RETRIES}\nrun_retry_backoff_ms = 10\n");
    let text = config(mock.base_url(), &extra);
    let server = Server::start_with(context.port()?, context.home().to_path_buf(), &text).await?;

    let session = flow::create_session(context, "重试").await?;
    let mut link = Reopened::open(context, &session.id).await?;

    // 一、可重试分类：上游 503 被重试 `RETRIES` 次，用尽后落固定文案。
    let prompt = format!("mock:upstream {}", context.token());
    let sent = flow::send_message(context, &session.id, &prompt).await?;
    let drained = flow::drain_run(&mut link.stream, &sent.run.id).await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunFailed,
        "重试用尽后应以 run_failed 收尾，实际 {:?}",
        drained.terminal.event
    );
    ensure!(
        drained.terminal.error.as_deref() == Some(RETRY_EXHAUSTED),
        "重试用尽的文案应是「{RETRY_EXHAUSTED}」，实际 {:?}",
        drained.terminal.error
    );
    let run = run_of(context, &sent.run.id).await?;
    ensure!(
        run.status == RunStatus::Failed && run.error.as_deref() == Some(RETRY_EXHAUSTED),
        "Run 应落 FAILED 且 error 是重试用尽的固定文案：{run:?}"
    );
    ensure!(
        run.resume_count == 0,
        "重试与恢复无关，不可重试路径不应累加 resumeCount：{run:?}"
    );

    // Mock 侧事实：上游被请求了 `1 + RETRIES` 次（每次重试都真的重发了同一份请求）。
    let requests = upstream_requests(&mock).await?;
    ensure!(
        requests == RETRIES + 1,
        "上游应被请求 {} 次（1 次失败 + {RETRIES} 次重试），实际 {requests}",
        RETRIES + 1
    );

    // 快照侧事实：重试计数等于配置的次数，因此「不超过上限」可直接复算。
    let capture = server.metrics().await;
    let metrics = capture.map_err(|reason| anyhow!("未采集：{reason}"))?;
    ensure!(
        metrics.limit.retries == RETRIES,
        "limit.retries 应等于配置的重试次数 {RETRIES}，实际 {}",
        metrics.limit.retries
    );

    // 重发的确实是同一份请求：Mock 收到的最后一条消息仍是本次输入（不是新拼一份请求）。
    let last = observe::last_request_messages(&mock).await?;
    ensure!(
        last.last().and_then(|message| message["content"].as_str()) == Some(prompt.as_str()),
        "重发的请求应仍以本次输入结尾：{last:?}"
    );

    // 二、不可重试分类：认证失败不消耗重试预算，上游只被请求一次，文案仍是原始脱敏文案。
    //
    // 这一段用一条新会话：同一会话里连续两条用户消息会被 Provider 合并成「多段正文」，
    // 而 Mock 的场景关键字只认纯文本正文（单条用户消息的形态），合并后就认不出来。
    let other = flow::create_session(context, "重试-不可重试").await?;
    let mut other_link = Reopened::open(context, &other.id).await?;
    let sent = flow::send_message(context, &other.id, &format!("mock:auth {}", context.token())).await?;
    let drained = flow::drain_run(&mut other_link.stream, &sent.run.id).await?;
    let auth_requests = auth_requests(&mock).await?;
    let requests_after = upstream_requests(&mock).await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunFailed && drained.terminal.error.as_deref() == Some(AUTH_ERROR),
        "认证失败应直接落 run_failed 且文案是「{AUTH_ERROR}」，实际 {:?} / {:?}",
        drained.terminal.event,
        drained.terminal.error
    );
    ensure!(
        auth_requests == 1,
        "不可重试分类只应请求上游一次，实际 {auth_requests} 次"
    );
    ensure!(
        requests_after == requests,
        "认证失败不应触发对上游的重试：{requests} → {requests_after}"
    );
    let capture = server.metrics().await;
    let metrics_after = capture.map_err(|reason| anyhow!("未采集：{reason}"))?;
    ensure!(
        metrics_after.limit.retries == RETRIES,
        "不可重试路径不应增加重试计数：{} → {}",
        metrics.limit.retries,
        metrics_after.limit.retries
    );

    other_link.release(context).await?;
    link.release(context).await?;
    context.observe_metrics(Ok(metrics_after));
    server.stop().await?;
    Ok(vec![
        format!(
            "上游 503 重试 {RETRIES} 次后用尽：run/get 落 FAILED + 「{RETRY_EXHAUSTED}」，Mock 侧共 {} 次请求",
            RETRIES + 1
        ),
        format!(
            "limit.retries = {}（等于配置次数，未超过上限）",
            metrics_after.limit.retries
        ),
        format!("认证失败不可重试：上游只被请求 1 次，文案为「{AUTH_ERROR}」，重试计数不变"),
    ])
}

/// `/mock/stats` 的上游 5xx 场景请求次数。
async fn upstream_requests(mock: &MockServer) -> Result<u64> {
    Ok(observe::stats(mock).await?["scenarios"]["upstream"]
        .as_u64()
        .unwrap_or_default())
}

/// `/mock/stats` 的认证失败场景请求次数。
async fn auth_requests(mock: &MockServer) -> Result<u64> {
    Ok(observe::stats(mock).await?["scenarios"]["auth"]
        .as_u64()
        .unwrap_or_default())
}

/// `api/run/get`。
async fn run_of(context: &TestContext, run_id: &str) -> Result<lib_bin_cli::sdk::Run> {
    context
        .client()
        .run_get(RunGetParams {
            run_id: run_id.to_owned(),
        })
        .await
        .map_err(lib_bin_cli::client::ClientError::from_call)
        .map_err(Into::into)
}
