//! `llm.canary`：密钥不进日志、不进响应（Z-1、Z-2）。
//!
//! 与 `llm.security` 互补：那里查进程内可输出文本（`Debug` / `Display` / 错误文案），
//! 这里用真实服务端 + 带 canary 的 `config.toml` 跑成功与失败两条路径，再检索
//! 服务端日志文件与下行 / 查询响应。

use anyhow::{Context, Result, ensure};
use lib_bin_cli::client::{ClientError, EventStream};
use lib_bin_cli::sdk::{
    ApiClient, ClientStreamParams, MessageListParams, MessageSendPO, RunEventKind, RunGetParams, SessionCreatePO,
    SessionSummary,
};

use crate::case::CaseFuture;
use crate::cases::llm::mock::MockServer;
use crate::context::TestContext;
use crate::flow;
use crate::server::Server;

/// canary：只要出现在日志或响应里就算泄漏。
const CANARY: &str = "sk-canary-5e1c9d7a2f84";
/// 成功后返回检索过的位置清单，作为 Z-1 / Z-2 的证据。
pub(super) fn canary(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { canary_body(context).await })
}

/// 用例主体：带 canary 起服务端 → 成功一轮 → 失败一轮 → 检索日志与响应。
async fn canary_body(context: &TestContext) -> Result<Vec<String>> {
    let mock = MockServer::start().await?;
    let port = context.port()?;
    let _server = Server::start_with_level(
        port,
        context.home().to_path_buf(),
        &canary_config(mock.base_url()),
        "trace",
    )
    .await
    .context("启动带 canary 密钥的服务端失败")?;

    let client_id = flow::register(context).await?;
    let session = create_session(context).await?;
    flow::occupy(context, &client_id, &session.id).await?;
    let mut stream = subscribe(context, &client_id).await?;

    // 成功路径：真实执行一轮对话。
    let sent = send(context, &session.id, &format!("canary {}", context.token())).await?;
    let drained = flow::drain_run(&mut stream, &sent.run.id).await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCompleted,
        "成功路径应以 run_completed 收尾，实际 {:?}",
        drained.terminal.event
    );

    // 失败路径：认证失败的错误文案同样不能带密钥。
    let failed = send(context, &session.id, &format!("mock:auth canary {}", context.token())).await?;
    let failed_drained = flow::drain_run(&mut stream, &failed.run.id).await?;
    ensure!(
        failed_drained.terminal.event == RunEventKind::RunFailed,
        "失败路径应以 run_failed 收尾，实际 {:?}",
        failed_drained.terminal.event
    );

    // 响应侧：失败 Run、消息列表与探活响应都不能带密钥。
    let run = context
        .client()
        .run_get(RunGetParams {
            run_id: failed.run.id.clone(),
        })
        .await
        .map_err(ClientError::from_call)?;
    let messages = context
        .client()
        .message_list(MessageListParams {
            session_id: session.id.clone(),
        })
        .await
        .map_err(ClientError::from_call)?;
    let health = context.client().probe_health().await.map_err(ClientError::from_call)?;
    let rendered = [
        ("run/get", serde_json::to_string(&run)?),
        ("message/list", serde_json::to_string(&messages)?),
        ("probe/health", serde_json::to_string(&health)?),
        ("下行成功事件", format!("{:?}", drained.events)),
        ("下行失败事件", format!("{:?}", failed_drained.events)),
    ];
    for (source, text) in &rendered {
        ensure!(!text.contains(CANARY), "{source} 的响应里出现了 canary 密钥");
    }

    // 日志侧：服务端进程的 stdout / stderr 与落盘日志都不能带密钥。
    let log = std::fs::read_to_string(context.home().join("server.log")).unwrap_or_default();
    ensure!(!log.contains(CANARY), "服务端日志里出现了 canary 密钥");
    let log_files = server_log_files(context)?;
    for (path, text) in &log_files {
        ensure!(!text.contains(CANARY), "日志文件 {path} 里出现了 canary 密钥");
    }
    let scanned_lines: usize =
        log.lines().count() + log_files.iter().map(|(_, text)| text.lines().count()).sum::<usize>();
    ensure!(
        scanned_lines > 0,
        "服务端日志为空（server.log 与 {} 个日志文件），无法判定 Z-1",
        log_files.len()
    );

    flow::release(context, &client_id).await?;

    Ok(vec![
        format!("成功路径：run_completed，正文 {} 字符", drained.content.chars().count()),
        format!("失败路径：run_failed（{:?}）", failed_drained.terminal.error),
        format!("响应检索：{} 处，均无 canary", rendered.len()),
        format!(
            "日志检索：server.log（{} 行）+ {} 个日志文件，共 {scanned_lines} 行，均无 canary",
            log.lines().count(),
            log_files.len()
        ),
    ])
}

/// 带 canary 密钥的服务端配置：`[provider]` 指向本用例的 Mock LLM。
fn canary_config(base_url: &str) -> String {
    format!(
        r#"[provider]
id = "canary"
kind = "openai-compatible"
base_url = "{base_url}/v1"
api_key = "{CANARY}"
default_model = "mock-model"

[[provider.models]]
id = "mock-model"
context_window = 8192
max_output_tokens = 1024
"#
    )
}

/// 建一个会话，返回会话摘要。
async fn create_session(context: &TestContext) -> Result<SessionSummary> {
    context
        .client()
        .session_create(SessionCreatePO {
            title: Some(format!("密钥安全 {}", context.token())),
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

/// 应用日志目录下的全部日志文件（`logs/` 与 `logs/archive/`）。
fn server_log_files(context: &TestContext) -> Result<Vec<(String, String)>> {
    let root = context.home().join("logs");
    let mut files = Vec::new();
    for directory in [root.clone(), root.join("archive")] {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|extension| extension == "log") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                files.push((path.display().to_string(), text));
            }
        }
    }
    ensure!(!files.is_empty(), "没有找到任何服务端日志文件，无法判定 Z-1");
    Ok(files)
}
