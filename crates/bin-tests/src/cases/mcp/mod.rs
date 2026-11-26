//! MCP 用例（P4-018）：配置装载、工具发现与命名、调用语义、生命周期与三个管理接口。
//!
//! 断言口径与 P3 一致：数据只来自四类来源——接口响应（`api/mcp/*`、`api/tool/audit/list`）、
//! 客户端事件、Mock LLM 的 `/mock/last-request`、`metrics` 快照与被测进程外的替身证据文件；
//! **不读日志**。
//!
//! 用例形态：
//!
//! - 进程内用例（`mcp.config` 的装载部分、`mcp.name`）：直接用 `agent-mcp` 的公开契约核对，
//!   不需要起服务端；
//! - 进程级用例：`SelfServed` 起一次私有服务端，`mcp.toml` 里的 stdio server 指向 `bin-tests`
//!   自己的隐藏子命令 `mock-mcp`（P4-017），HTTP server 指向用例自己起的替身进程。

mod api;
mod call;
mod config;
mod discover;
mod lifecycle;

use std::future::Future;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{
    ApiClient, McpAction, McpServerUpdatePO, McpServerVO, McpStatusKind, McpToolListParams, McpToolVO,
};
use serde_json::Value;
use tokio::time::sleep;

use crate::case::{Case, CaseFn, CaseKind};
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::server;

/// 默认 server 的配置 `id`：同时是模型可见名的前缀来源（`mcp__mock__<tool>`）。
pub(super) const SERVER: &str = "mock";

/// 轮询接口状态的上限：连接、退避重连与注销都是异步的。
const STATUS_TIMEOUT: Duration = Duration::from_secs(15);
/// 轮询间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// MCP 用例；排序由上层统一处理。
pub(crate) fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    cases.extend(config::cases());
    cases.extend(discover::cases());
    cases.extend(call::cases());
    cases.extend(lifecycle::cases());
    cases.extend(api::cases());
    cases
}

/// 声明一个用例：MCP 用例都不需要额外的前置产物。
pub(super) fn case(name: &'static str, kind: CaseKind, run: CaseFn) -> Case {
    Case {
        name,
        kind,
        prep: &[],
        run,
    }
}

/// 工具在模型侧的可见名：`mcp__<server>__<原名清洗后>`。
pub(super) fn visible(tool: &str) -> String {
    format!("mcp__{SERVER}__{tool}")
}

/// 用户输入 + 末尾的工具调用脚本（脚本必须落在最后一条消息的末尾）。
pub(super) fn prompt(spec: Value) -> String {
    format!("按脚本处理这一轮。\ntool:{spec}")
}

/// 用私有 Mock 的 `[provider]` 配置 + `mcp.toml` 起一次私有服务端，跑完用例体后采集快照并停止。
pub(super) async fn serve_with<T>(
    context: &TestContext,
    mock: &MockServer,
    extra: &str,
    mcp: &str,
    body: impl Future<Output = Result<T>>,
) -> Result<T> {
    let text = server::config(mock.base_url(), extra);
    server::serve_once_mcp(context, &text, mcp, body).await
}

/// 不需要模型调用的进程级用例：`[provider]` 指向一个必然连不上的地址（用例不建会话）。
pub(super) async fn serve_plain<T>(
    context: &TestContext,
    extra: &str,
    mcp: &str,
    body: impl Future<Output = Result<T>>,
) -> Result<T> {
    let text = server::config("http://127.0.0.1:9", extra);
    server::serve_once_mcp(context, &text, mcp, body).await
}

/// 直接起一个 `bin-agent server` 子进程并等它退出：只用于「启动期硬错误」的退出码断言。
///
/// 失败原因写在子进程 stderr 与日志里，用例**不读**它们，只看退出码（E-5 的判定口径）。
pub(super) async fn startup_exit_code(home: &std::path::Path) -> Result<Option<i32>> {
    let port = server::free_port()?;
    let output = tokio::process::Command::new(server::server_binary()?)
        .arg("server")
        .arg("--addr")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--log-level")
        .arg("warn")
        .env(lib_core::directorys::ENV_HOME, home)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .context("启动服务端子进程失败")?;
    Ok(output.status.code())
}

/// 全部 server 的状态快照（`GET api/mcp/server/list`）。
pub(super) async fn servers(context: &TestContext) -> Result<Vec<McpServerVO>> {
    let servers = context
        .client()
        .mcp_server_list()
        .await
        .map_err(ClientError::from_call)?;
    Ok(servers)
}

/// 按 `id` 取一个 server 的状态快照；不存在时失败并列出实际清单。
pub(super) async fn server(context: &TestContext, id: &str) -> Result<McpServerVO> {
    let all = servers(context).await?;
    all.iter()
        .find(|server| server.id == id)
        .cloned()
        .with_context(|| format!("server/list 里没有 {id}：{all:?}"))
}

/// 工具快照（`GET api/mcp/tool/list`）；`server_id` 为空时返回全部。
pub(super) async fn tools(context: &TestContext, server_id: Option<&str>) -> Result<Vec<McpToolVO>> {
    let tools = context
        .client()
        .mcp_tool_list(McpToolListParams {
            server_id: server_id.map(str::to_owned),
        })
        .await
        .map_err(ClientError::from_call)?;
    Ok(tools)
}

/// 工具快照里的可见名（按接口返回的顺序）。
pub(super) fn tool_names(tools: &[McpToolVO]) -> Vec<String> {
    tools.iter().map(|tool| tool.tool_name.clone()).collect()
}

/// 执行一个管理动作（`POST api/mcp/server/update`）。
pub(super) async fn update(context: &TestContext, id: &str, action: McpAction) -> Result<McpServerVO> {
    let server = context
        .client()
        .mcp_server_update(McpServerUpdatePO {
            server_id: id.to_owned(),
            action,
        })
        .await
        .map_err(ClientError::from_call)?;
    Ok(server)
}

/// 执行一个动作并断言它是个固定 400（未知 `serverId` 的判定口径）。
pub(super) async fn update_rejected(context: &TestContext, id: &str, action: McpAction) -> Result<String> {
    match context
        .client()
        .mcp_server_update(McpServerUpdatePO {
            server_id: id.to_owned(),
            action,
        })
        .await
        .map_err(ClientError::from_call)
    {
        Err(ClientError::Status { code, message }) => {
            ensure!(code == 400, "未知 serverId 应返回 400，实际 {code}（{message}）");
            Ok(message)
        }
        other => bail!("未知 serverId 应返回固定 400，实际 {other:?}"),
    }
}

/// 轮询 `server/list` 直到该 server 满足条件；超时返回最后一次读到的快照。
pub(super) async fn wait_server(
    context: &TestContext,
    id: &str,
    done: impl Fn(&McpServerVO) -> bool,
) -> Result<McpServerVO> {
    let deadline = Instant::now() + STATUS_TIMEOUT;
    loop {
        let snapshot = server(context, id).await?;
        if done(&snapshot) || Instant::now() >= deadline {
            return Ok(snapshot);
        }
        sleep(POLL_INTERVAL).await;
    }
}

/// 轮询 `server/list` 直到该 server 抵指定状态。
pub(super) async fn wait_status(context: &TestContext, id: &str, want: McpStatusKind) -> Result<McpServerVO> {
    wait_server(context, id, |snapshot| snapshot.status == want).await
}

/// 轮询 `tool/list` 直到出现指定可见名；超时返回最后一次读到的清单。
pub(super) async fn wait_tool(context: &TestContext, name: &str) -> Result<Vec<McpToolVO>> {
    let deadline = Instant::now() + STATUS_TIMEOUT;
    loop {
        let snapshot = tools(context, None).await?;
        if snapshot.iter().any(|tool| tool.tool_name == name) || Instant::now() >= deadline {
            return Ok(snapshot);
        }
        sleep(POLL_INTERVAL).await;
    }
}

/// 最近一次模型请求里的工具清单（`/mock/last-request` 的 `tools` 数组）。
pub(super) async fn request_tools(mock: &MockServer) -> Result<Vec<Value>> {
    let recorded = crate::mock::llm::observe::last_request(mock).await?;
    Ok(recorded["tools"].as_array().cloned().unwrap_or_default())
}

/// 请求体里工具清单的名字（按请求顺序）。
pub(super) fn request_tool_names(tools: &[Value]) -> Vec<String> {
    tools
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_owned))
        .collect()
}
