//! 生命周期与传输（P4-018）：`mcp.crash`（L-3 / L-4）、`mcp.reconnect`（L-5）、
//! `mcp.shutdown`（L-7 / L-8）、`mcp.http`（T-2 / E-2）。
//!
//! 崩溃、重连与关闭都是异步的：断言一律「轮询到条件成立或超时」，超时后把最后一次读到的状态写进
//! 失败信息，不需要翻日志。`mcp.shutdown` 走进程内装配（`assemble_with`），因为它要断言的是
//! 「关闭后替身进程不存在」，而这正是 `McpManager::shutdown_all` 的职责。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use agent_mcp::{McpConfig, McpManager, McpServerSpec, McpStatusKind as ManagerStatus, McpTransport};
use agent_tool::{ToolContext, ToolPolicy, ToolRegistry};
use anyhow::{Context, Result, ensure};
use lib_bin_cli::config::parse_mcp;
use lib_bin_cli::sdk::{McpAction, McpStatusKind, RunEventKind, ToolCallStatus};
use serde_json::{Value, json};

use super::{case, prompt, serve_with, visible, wait_status as wait_mcp_status};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::cases::tool::audit;
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::mcp as mcp_util;
use crate::util::support::{temp_directory, wait_until};

/// 崩溃用例的配置：关闭自动重连，状态停在 `FAILED`（D-P4-011 的 `0` 语义）。
const NO_RECONNECT: u64 = 0;

/// 生命周期与传输用例。
pub(super) fn cases() -> Vec<Case> {
    vec![
        case("mcp.crash", CaseKind::SelfServed, crash),
        case("mcp.http", CaseKind::SelfServed, http),
        case("mcp.reconnect", CaseKind::SelfServed, reconnect),
        case("mcp.shutdown", CaseKind::SelfServed, shutdown),
    ]
}

/// 一条 stdio 替身的 `mcp.toml`。
fn single_stdio(evidence: &Path, scenario: &str, extra: &str) -> Result<String> {
    Ok(format!(
        "{}{}",
        mcp_util::header(5000, NO_RECONNECT),
        mcp_util::stdio_server(super::SERVER, true, scenario, evidence, None, extra)?
    ))
}

/// 一轮脚本：只要求模型调用一个工具。
fn one_call(tool: &str, arguments: Value) -> Value {
    json!({ "calls": [{ "name": visible(tool), "arguments": arguments }] })
}

/// L-3 / L-4：替身退出后状态可见，之后的调用得到可读错误而不是挂死。
fn crash(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-crash")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "crash", "")?;
        let notes = serve_with(context, &mock, "", &mcp, async {
            wait_mcp_status(context, super::SERVER, McpStatusKind::Ready).await?;
            let mut occupied = crate::util::flow::Occupied::start(context, "mcp.crash").await?;

            // 第一次调用：替身记录之后直接退出进程。
            let (first, drained) = occupied.say(context, &prompt(one_call("crash", json!({})))).await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "替身崩溃也是工具结果，Run 应完成：{:?}",
                drained.terminal.event
            );
            let failed = audit(context, &first.run.id).await?;
            ensure!(
                failed.len() == 1 && failed[0].status == ToolCallStatus::Error,
                "崩溃的调用应落 ERROR：{failed:?}"
            );
            let first_error = failed[0].result.clone().unwrap_or_default();
            ensure!(
                first_error.contains("MCP 服务器"),
                "崩溃的可读文案应点名服务器：{first_error}"
            );

            // L-3：状态立刻变 FAILED，原因可读。
            let snapshot = wait_mcp_status(context, super::SERVER, McpStatusKind::Failed).await?;
            ensure!(
                snapshot.status == McpStatusKind::Failed,
                "替身退出后状态应为 FAILED：{snapshot:?}"
            );
            let reason = snapshot.last_error.clone().unwrap_or_default();
            ensure!(!reason.trim().is_empty(), "FAILED 应带可读原因：{snapshot:?}");
            ensure!(
                !reason.contains("127.0.0.1") && !reason.contains("mock-mcp"),
                "原因不应含命令行或地址：{reason}"
            );

            // L-4：崩溃之后的调用在超时内返回可读错误，Run 正常收尾。
            let started = std::time::Instant::now();
            let (second, drained) = occupied
                .say(context, &prompt(one_call("echo", json!({"text": "崩后"}))))
                .await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "崩溃后的调用也是工具结果，Run 应完成：{:?}",
                drained.terminal.event
            );
            ensure!(
                started.elapsed() < Duration::from_secs(20),
                "崩溃后的调用不应挂死：{:?}",
                started.elapsed()
            );
            let after = audit(context, &second.run.id).await?;
            ensure!(
                after.len() == 1 && after[0].status == ToolCallStatus::Error,
                "崩溃后的调用应落 ERROR：{after:?}"
            );
            let text = after[0].result.clone().unwrap_or_default();
            ensure!(text.contains("MCP 服务器"), "崩溃后的错误文案应可读：{text}");
            let recorded = mcp_util::evidence(&evidence)?;
            ensure!(
                mcp_util::call_of(&recorded, "crash").is_some() && mcp_util::call_of(&recorded, "echo").is_none(),
                "替身应只收到过 crash 调用：{recorded}"
            );
            occupied.release(context).await?;
            Ok(vec![
                format!("崩溃：{first_error} → {reason}"),
                format!("崩溃后的调用：{text}"),
            ])
        })
        .await?;
        Ok(notes)
    })
}

/// 门面快照里唯一的 server 是否 `READY`。
fn ready(manager: &McpManager) -> bool {
    manager
        .snapshot()
        .first()
        .is_some_and(|server| server.status == ManagerStatus::Ready)
}

/// L-5：`RECONNECT` 把 `FAILED` 拉回 `READY`；关闭自动重连时不增长 `reconnects`。
fn reconnect(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();
        let mock = MockServer::start().await?;

        // 一、`reconnect_max_backoff_ms = 0`：崩溃后停在 FAILED，只有手动 RECONNECT 能恢复。
        let root = temp_directory("mcp-reconnect-off")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "crash", "")?;
        serve_with(context, &mock, "", &mcp, async {
            wait_mcp_status(context, super::SERVER, McpStatusKind::Ready).await?;
            let mut occupied = crate::util::flow::Occupied::start(context, "mcp.reconnect").await?;
            occupied.say(context, &prompt(one_call("crash", json!({})))).await?;
            let before = crate::util::server::capture(context.base_url())
                .await
                .map_err(|reason| anyhow::anyhow!("崩溃后的快照未采集：{reason}"))?;
            let failed = wait_mcp_status(context, super::SERVER, McpStatusKind::Failed).await?;
            ensure!(failed.status == McpStatusKind::Failed, "崩溃后应 FAILED：{failed:?}");

            // 关闭自动重连：等一段明显的退避窗口后仍是 FAILED，且 `reconnects` 不增长。
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let still = super::server(context, super::SERVER).await?;
            ensure!(
                still.status == McpStatusKind::Failed && still.next_retry_at_ms.is_none(),
                "关闭自动重连后不应排重连：{still:?}"
            );
            let after = crate::util::server::capture(context.base_url())
                .await
                .map_err(|reason| anyhow::anyhow!("等待后的快照未采集：{reason}"))?;
            ensure!(
                after.mcp.reconnects == before.mcp.reconnects,
                "关闭自动重连时 reconnects 不应增长：{} → {}",
                before.mcp.reconnects,
                after.mcp.reconnects
            );

            // 手动 RECONNECT：新起一个替身进程，重新发现工具。
            let recovered = super::update(context, super::SERVER, McpAction::Reconnect).await?;
            ensure!(
                recovered.status == McpStatusKind::Ready && recovered.tool_count as usize == 2,
                "RECONNECT 应回到 READY 并重新发现工具：{recovered:?}"
            );
            let (_, drained) = occupied
                .say(context, &prompt(one_call("echo", json!({"text": "恢复"}))))
                .await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted && drained.content.contains("恢复"),
                "恢复后的调用应成功：{:?}",
                drained.terminal.event
            );
            occupied.release(context).await?;
            Ok::<(), anyhow::Error>(())
        })
        .await?;
        notes.push("关闭自动重连：FAILED 保持，reconnects 不增长；RECONNECT → READY（2 个工具）".to_owned());

        // 二、默认退避（初始 200ms）：崩溃后自动重连回 READY，`reconnects` 增长。
        //
        // 这一段走进程内门面：退避与重连是 `agent-mcp` 自己的行为，进程内只等内存状态，
        // 不受服务端 HTTP 轮询时机影响，`metrics` 也直接读本进程的累计量（取前后差值）。
        let root = temp_directory("mcp-reconnect-auto")?;
        let evidence = root.join("evidence.json");
        let binary = mcp_util::test_binary()?;
        let spec = McpServerSpec::new(
            super::SERVER,
            McpTransport::Stdio {
                command: binary.display().to_string(),
                args: vec![
                    "mock-mcp".to_owned(),
                    "--transport".to_owned(),
                    "stdio".to_owned(),
                    "--scenario".to_owned(),
                    "crash".to_owned(),
                    "--evidence".to_owned(),
                    evidence.display().to_string(),
                ],
                env: std::collections::BTreeMap::new(),
                cwd: None,
            },
        )
        .with_enabled(true);
        let registry = Arc::new(ToolRegistry::new(ToolPolicy::empty()));
        let manager = McpManager::new(
            McpConfig {
                reconnect_initial_backoff: Duration::from_millis(200),
                reconnect_max_backoff: Duration::from_millis(2000),
                servers: vec![spec],
            },
            Arc::clone(&registry),
        );
        manager.connect_enabled_all().await;
        ensure!(ready(&manager), "替身应先连上 READY：{:?}", manager.snapshot());
        let before = crate::util::support::local_metrics().mcp.reconnects;

        // 通过注册表调用 crash 工具（与运行时同一条执行路径），替身随后退出进程。
        let tool = registry.lookup(&visible("crash")).context("注册表里应有 crash 工具")?;
        let tool_context = ToolContext {
            home: root.clone(),
            session_id: "mcp-reconnect".to_owned(),
            run_id: "mcp-reconnect".to_owned(),
            tool_call_id: "mcp-reconnect".to_owned(),
            deadline: std::time::Instant::now() + Duration::from_secs(5),
        };
        let error = tool
            .call(&tool_context, json!({}))
            .await
            .err()
            .context("替身退出后调用应失败")?;
        ensure!(
            error.to_string().contains("MCP 服务器"),
            "失败文案应点名服务器：{error}"
        );
        ensure!(
            wait_until(|| manager.snapshot()[0].status == ManagerStatus::Failed).await,
            "崩溃后应先 FAILED：{:?}",
            manager.snapshot()
        );
        ensure!(
            wait_until(|| ready(&manager)).await,
            "默认退避下应自动重连回 READY：{:?}",
            manager.snapshot()
        );
        let after = crate::util::support::local_metrics().mcp.reconnects;
        ensure!(after > before, "自动重连应让 reconnects 增长：{before} → {after}");
        let recorded = mcp_util::evidence(&evidence)?;
        ensure!(
            recorded["scenario"] == Value::String("crash".to_owned()),
            "证据文件应属于 crash 剧本：{recorded}"
        );
        manager.shutdown_all().await;
        notes.push(format!(
            "默认退避：FAILED → READY 自动恢复，reconnects {before} → {after}（调用：{error}）"
        ));
        Ok(notes)
    })
}

/// L-7 / L-8：关闭后替身进程不存在、工具全部注销、状态全部 `STOPPED`。
fn shutdown(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();
        let root = temp_directory("mcp-shutdown")?;
        let evidence = root.join("evidence.json");
        let pid_file = root.join("mock.pid");
        let text = format!(
            "{}{}",
            mcp_util::header(5000, NO_RECONNECT),
            mcp_util::stdio_server(super::SERVER, true, "normal", &evidence, Some(&pid_file), "")?
        );
        let options = manager_agent::AssemblyOptions {
            home: Some(root.clone()),
            events_root: Some(root.join("events")),
            mcp: parse_mcp(&text)?,
            ..Default::default()
        };
        let holder = manager_agent::assemble_with(root.join("agent.db"), options).await?;

        // 装配完成即连接：等 READY 与工具注册。
        ensure!(
            wait_until(|| holder
                .mcp()
                .snapshot()
                .iter()
                .all(|server| server.status == agent_mcp::McpStatusKind::Ready))
            .await,
            "MCP server 应连接到 READY：{:?}",
            holder.mcp().snapshot()
        );
        let tools = holder.mcp().tool_snapshot();
        ensure!(
            tools.iter().any(|tool| tool.tool_name == visible("echo")),
            "READY 后应注册替身的工具：{tools:?}"
        );
        ensure!(
            holder.tools().lookup(&visible("echo")).is_some(),
            "工具应进同一个注册表（与内置工具同构）"
        );
        let pid = mcp_util::pid_of(&pid_file)?;
        ensure!(mcp_util::process_alive(pid), "替身子进程应在运行：pid={pid}");

        // 关闭：先停运行时（Run 落终态）再关 MCP；这里断言后半段的可见结果。
        let kills = holder.mcp().shutdown_all().await;
        ensure!(
            holder
                .mcp()
                .snapshot()
                .iter()
                .all(|server| server.status == agent_mcp::McpStatusKind::Stopped),
            "关闭后状态应全部 STOPPED：{:?}",
            holder.mcp().snapshot()
        );
        ensure!(holder.mcp().tool_snapshot().is_empty(), "关闭后应注销全部 MCP 工具");
        ensure!(
            holder.tools().lookup(&visible("echo")).is_none(),
            "注册表里也不应留下该工具"
        );
        ensure!(
            mcp_util::wait_process_gone(pid).await,
            "关闭后替身子进程应不存在：pid={pid}"
        );
        let metrics = crate::util::support::local_metrics().mcp;
        notes.push(format!(
            "关闭：STOPPED、工具注销、pid {pid} 已退出（kills={kills}，shutdown_kills={}）",
            metrics.shutdown_kills
        ));
        Ok(notes)
    })
}

/// T-2 / E-2：HTTP transport 走通一次调用，且凭证与地址不出现在任何输出里。
fn http(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let remote = mcp_util::HttpMock::start("normal").await?;
        let token = "Bearer mcp-http-secret-token";
        let mcp = format!(
            "{}{}",
            mcp_util::header(5000, NO_RECONNECT),
            mcp_util::http_server(
                super::SERVER,
                remote.url(),
                &format!("headers = {{ Authorization = '{token}' }}\npermission = 'allow'\n")
            )
        );
        let notes = serve_with(context, &mock, "", &mcp, async {
            let ready = wait_mcp_status(context, super::SERVER, McpStatusKind::Ready).await?;
            ensure!(
                ready.status == McpStatusKind::Ready && ready.transport == "http",
                "HTTP server 应 READY：{ready:?}"
            );
            let listed = super::tools(context, Some(super::SERVER)).await?;
            ensure!(listed.len() == 6, "HTTP transport 应发现同一份剧本：{listed:?}");

            let mut occupied = crate::util::flow::Occupied::start(context, "mcp.http").await?;
            let (sent, drained) = occupied
                .say(context, &prompt(one_call("echo", json!({"text": "http"}))))
                .await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted && drained.content.contains("http"),
                "HTTP 调用应成功闭环：{:?}",
                drained.terminal.event
            );
            let records = audit(context, &sent.run.id).await?;
            ensure!(
                records.len() == 1 && records[0].status == ToolCallStatus::Ok,
                "HTTP 调用审计应为 OK：{records:?}"
            );
            let recorded =
                mcp_util::wait_evidence(remote.evidence(), |value| !mcp_util::calls(value).is_empty()).await?;
            ensure!(
                recorded["transport"] == Value::String("http".to_owned())
                    && mcp_util::call_of(&recorded, "echo").is_some(),
                "替身应经 HTTP 收到调用：{recorded}"
            );

            // E-2：凭证、地址与命令行不出现在事件、审计与接口响应里。
            let mut seen = Vec::new();
            seen.push(serde_json::to_string(&drained.events)?);
            seen.push(serde_json::to_string(&records)?);
            seen.push(serde_json::to_string(&super::servers(context).await?)?);
            seen.push(serde_json::to_string(&super::tools(context, None).await?)?);
            let haystack = seen.join("\n");
            for secret in [token, remote.url(), "mock-mcp", "127.0.0.1"] {
                ensure!(!haystack.contains(secret), "输出里不应出现 `{secret}`：{haystack}");
            }
            occupied.release(context).await?;
            Ok(vec![
                format!("HTTP：{} 个工具，一次调用 OK", listed.len()),
                format!("凭证与地址 0 命中（token={}…、url={}）", &token[..7], remote.url()),
            ])
        })
        .await?;
        // 替身自己的 PID 文件：关闭前它必须还在（用例负责杀它）。
        let pid = mcp_util::pid_of(remote.pid_file())?;
        ensure!(mcp_util::process_alive(pid), "HTTP 替身应在运行：pid={pid}");
        remote.stop().await?;
        Ok(notes)
    })
}
