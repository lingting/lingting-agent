//! 管理接口与指标（P4-018）：`mcp.api`（L-1 / I-2 / I-3 / I-4）、`mcp.metrics`（P4-013）。
//!
//! 三条接口的载荷字段、排序与错误文案在 `docs/api/mcp.md` 里定稿；这里逐条核对，并顺带证明
//! 「动作幂等」与「状态只经接口与 `metrics` 暴露」（D-P4-019）。

use std::path::Path;

use anyhow::{Result, ensure};
use lib_bin_cli::sdk::{McpAction, McpStatusKind, RunEventKind};

use super::{
    case, prompt, serve_plain, serve_with, servers, tool_names, tools, update, update_rejected, visible, wait_status,
};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::mcp as mcp_util;
use crate::util::support::temp_directory;

/// 接口与指标用例。
pub(super) fn cases() -> Vec<Case> {
    vec![
        case("mcp.api", CaseKind::SelfServed, api),
        case("mcp.metrics", CaseKind::SelfServed, metrics),
    ]
}

/// 一条 stdio 替身的 `mcp.toml`。
fn stdio_block(id: &str, enabled: bool, evidence: &Path) -> Result<String> {
    mcp_util::stdio_server(id, enabled, "normal", evidence, None, "")
}

/// I-2 / I-3 / I-4 + L-1：`server/list` 语义、`tool/list` 语义、四个动作与未知 `serverId`。
fn api(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();
        let root = temp_directory("mcp-api")?;
        let alpha_evidence = root.join("alpha.json");
        let beta_evidence = root.join("beta.json");
        let mcp = format!(
            "{}{}\n\n{}",
            mcp_util::header(5000, 0),
            stdio_block("beta", true, &beta_evidence)?,
            stdio_block("alpha", false, &alpha_evidence)?
        );
        serve_plain(context, "", &mcp, async {
            // I-2：含 disabled 的 server，按 id 升序，字段与文档一致。
            let list = servers(context).await?;
            ensure!(
                list.iter().map(|server| server.id.as_str()).collect::<Vec<_>>() == ["alpha", "beta"],
                "server/list 应按 id 升序且包含 disabled：{list:?}"
            );
            let alpha = &list[0];
            ensure!(
                !alpha.enabled
                    && alpha.status == McpStatusKind::Disabled
                    && alpha.transport == "stdio"
                    && alpha.tool_count == 0
                    && alpha.last_error.is_none()
                    && alpha.last_connected_at_ms.is_none()
                    && alpha.next_retry_at_ms.is_none(),
                "disabled server 的字段应全是缺省值：{alpha:?}"
            );
            let beta = wait_status(context, "beta", McpStatusKind::Ready).await?;
            ensure!(
                beta.enabled
                    && beta.transport == "stdio"
                    && beta.tool_count as usize == 6
                    && beta.last_error.is_none()
                    && beta.last_connected_at_ms.is_some(),
                "enabled server 应 READY 且带连接时间：{beta:?}"
            );
            notes.push(format!(
                "server/list：alpha={:?}，beta={:?}（id 升序）",
                alpha.status, beta.status
            ));

            // I-3：按可见名升序、`serverId` 过滤、未知 `serverId` 返回空列表。
            let all = tools(context, None).await?;
            let names = tool_names(&all);
            let mut sorted = names.clone();
            sorted.sort();
            ensure!(names == sorted, "tool/list 应按可见名升序：{names:?}");
            ensure!(all.len() == 6, "只有 beta 注册了工具：{names:?}");
            ensure!(
                tools(context, Some("beta")).await?.len() == all.len(),
                "按 serverId 过滤应只返回该 server 的工具"
            );
            ensure!(
                tools(context, Some("alpha")).await?.is_empty(),
                "disabled 的 server 没有工具"
            );
            ensure!(
                tools(context, Some("nope")).await?.is_empty(),
                "未知 serverId 应返回空列表而不是 404"
            );
            notes.push(format!("tool/list：{} 个工具，过滤与未知 id 均符合", all.len()));

            // L-1 / I-4：ENABLE 幂等；DISABLE 幂等；PROBE 不建连接；未知 id 是 400。
            let enabled = update(context, "alpha", McpAction::Enable).await?;
            ensure!(
                enabled.status == McpStatusKind::Ready && enabled.tool_count as usize == 6,
                "ENABLE 应连接并发现工具：{enabled:?}"
            );
            let enabled_again = update(context, "alpha", McpAction::Enable).await?;
            ensure!(
                enabled_again.status == enabled.status && enabled_again.tool_count == enabled.tool_count,
                "连续两次 ENABLE 的状态与工具数应一致：{enabled_again:?}"
            );
            ensure!(
                tools(context, None).await?.len() == 12,
                "两个 server 的工具应同时在册（12 条）"
            );
            let disabled = update(context, "alpha", McpAction::Disable).await?;
            ensure!(
                disabled.status == McpStatusKind::Disabled && disabled.tool_count == 0,
                "DISABLE 应注销工具：{disabled:?}"
            );
            let disabled_again = update(context, "alpha", McpAction::Disable).await?;
            ensure!(
                disabled_again.status == disabled.status && disabled_again.tool_count == disabled.tool_count,
                "连续两次 DISABLE 的状态与工具数应一致：{disabled_again:?}"
            );
            ensure!(tools(context, None).await?.len() == 6, "DISABLE 之后工具数应回到 6");
            let probed = update(context, "alpha", McpAction::Probe).await?;
            ensure!(
                probed.status == McpStatusKind::Disabled,
                "对 DISABLED 的 server 发 PROBE 应返回状态本身：{probed:?}"
            );

            // RECONNECT 会新起一个替身进程：证据文件里的 pid 变了，说明连接被重建、工具重新发现。
            let before = mcp_util::evidence(&beta_evidence)?;
            let previous_pid = before["pid"].as_u64().unwrap_or_default();
            let reconnected = update(context, "beta", McpAction::Reconnect).await?;
            ensure!(
                reconnected.status == McpStatusKind::Ready
                    && reconnected.tool_count as usize == 6
                    && reconnected.last_connected_at_ms >= beta.last_connected_at_ms,
                "RECONNECT 应回到 READY 且刷新连接时间：{reconnected:?}"
            );
            let after = mcp_util::wait_evidence(&beta_evidence, |value| {
                value["pid"].as_u64().unwrap_or_default() != previous_pid
            })
            .await?;
            ensure!(
                after["toolsListCalls"].as_u64().unwrap_or_default() >= 1,
                "RECONNECT 之后应重新 `tools/list`：{after}"
            );

            let message = update_rejected(context, "nope", McpAction::Enable).await?;
            ensure!(message == "MCP 服务器不存在", "未知 serverId 的固定文案不符：{message}");
            notes.push("动作：ENABLE / DISABLE 幂等，PROBE 返回状态，RECONNECT 重新发现，未知 id=400".to_owned());
            Ok(())
        })
        .await?;
        Ok(notes)
    })
}

/// `metrics` 的 `mcp` 段有值（未开特性时显式记「未采集」）。
fn metrics(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-metrics")?;
        let evidence = root.join("evidence.json");
        let mcp = format!(
            "{}{}",
            mcp_util::header(5000, 0),
            stdio_block(super::SERVER, true, &evidence)?
        );
        let notes = serve_with(context, &mock, "", &mcp, async {
            wait_status(context, super::SERVER, McpStatusKind::Ready).await?;
            let mut occupied = crate::util::flow::Occupied::start(context, "mcp.metrics").await?;
            let spec = serde_json::json!({"calls": [{"name": visible("echo"), "arguments": {"text": "metrics"}}]});
            let (_, drained) = occupied.say(context, &prompt(spec)).await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "指标用例的 Run 应完成：{:?}",
                drained.terminal.event
            );

            // 快照必须在服务端仍存活时取（`serve_once` 的采集在停止之前）。
            match crate::util::server::capture(context.base_url()).await {
                Ok(snapshot) => {
                    let mcp = snapshot.mcp;
                    ensure!(
                        mcp.servers == 1
                            && mcp.ready == 1
                            && mcp.tools == 6
                            && mcp.connects >= 1
                            && mcp.calls >= 1,
                        "`metrics.mcp` 的状态与累计量都应有值：{mcp:?}"
                    );
                    occupied.release(context).await?;
                    Ok(vec![format!(
                        "metrics.mcp：servers={} ready={} tools={} connects={} calls={} call_errors={} timeouts={} kills={}",
                        mcp.servers,
                        mcp.ready,
                        mcp.tools,
                        mcp.connects,
                        mcp.calls,
                        mcp.call_errors,
                        mcp.call_timeouts,
                        mcp.shutdown_kills
                    )])
                }
                Err(reason) => {
                    // 未启用 `metrics` 特性时接口不存在：显式记「未采集」，不算通过也不算失败。
                    occupied.release(context).await?;
                    Ok(vec![format!("未采集：{reason}")])
                }
            }
        })
        .await?;
        Ok(notes)
    })
}
