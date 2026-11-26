//! 工具发现与命名（P4-018）：`mcp.discover`（T-1 / T-4 / S-4 / L-6）、`mcp.name`（S-3）、
//! `mcp.schema`（S-2）。
//!
//! `mcp.discover` 与 `mcp.schema` 是进程级用例：真实服务端 + 替身 + Mock LLM，证据来自接口响应、
//! 模型请求体与替身的结构化快照。`mcp.name` 走进程内：直接驱动 `agent-mcp` 的门面，只核对命名
//! 规则本身（不需要服务端与模型）。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use agent_mcp::{McpConfig, McpManager, McpServerSpec, McpTransport};
use agent_tool::{ToolPolicy, ToolRegistry};
use anyhow::{Context, Result, ensure};
use lib_bin_cli::sdk::McpStatusKind;
use serde_json::Value;

use super::{SERVER, case, request_tool_names, request_tools, serve_with, tools, visible, wait_status, wait_tool};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::mock::llm::MockServer;
use crate::mock::mcp::WEIRD_SCHEMA;
use crate::util::context::TestContext;
use crate::util::flow::Occupied;
use crate::util::mcp as mcp_util;
use crate::util::support::temp_directory;

/// 发现与命名用例。
pub(super) fn cases() -> Vec<Case> {
    vec![
        case("mcp.discover", CaseKind::SelfServed, discover),
        case("mcp.name", CaseKind::SelfServed, name),
        case("mcp.schema", CaseKind::SelfServed, schema),
    ]
}

/// 常规剧本的工具清单（`tools/list` 的原名）。
const NORMAL_TOOLS: [&str; 6] = [
    "read_file",
    "write_file",
    "write_log",
    "pre_commit_write",
    "delete_all",
    "echo",
];

/// 工具发现：`READY`、`clientInfo`、`tool/list` 与请求体一致、清单变化后自动重新发现（T-1 / T-4 / S-4 / L-6）。
fn discover(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();

        // 一、stdio server 被启动并握手（T-1），可见名以 `mcp__<serverId>__` 开头（S-4）。
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-discover")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "normal", "")?;
        serve_with(context, &mock, "", &mcp, async {
            let snapshot = wait_status(context, SERVER, McpStatusKind::Ready).await?;
            ensure!(
                snapshot.status == McpStatusKind::Ready && snapshot.tool_count as usize == NORMAL_TOOLS.len(),
                "stdio server 应 READY 且发现 {} 个工具：{snapshot:?}",
                NORMAL_TOOLS.len()
            );

            // T-4：替身记录到的 `clientInfo` 是本进程的名字与版本。
            let recorded = mcp_util::wait_evidence(&evidence, |value| value["clientInfo"].is_object()).await?;
            let client = &recorded["clientInfo"];
            ensure!(
                client["name"] == Value::String(agent_mcp::CLIENT_NAME.to_owned()),
                "clientInfo.name 应为 {}：{client}",
                agent_mcp::CLIENT_NAME
            );
            ensure!(
                client["version"] == Value::String(env!("CARGO_PKG_VERSION").to_owned()),
                "clientInfo.version 应等于 workspace 版本：{client}"
            );

            // S-4：清单按可见名升序，前缀与原名可查。
            let listed = tools(context, Some(SERVER)).await?;
            let names = super::tool_names(&listed);
            let mut sorted = names.clone();
            sorted.sort();
            ensure!(names == sorted, "tool/list 应按可见名升序：{names:?}");
            for tool in &listed {
                ensure!(
                    tool.tool_name == visible(&tool.original_name),
                    "可见名应是 `mcp__{SERVER}__<清洗后的原名>`：{tool:?}"
                );
                ensure!(!tool.description.trim().is_empty(), "工具说明不应为空：{tool:?}");
            }
            // 清单按可见名排序，因此这里比对集合而不是顺序。
            let mut originals: Vec<&str> = listed.iter().map(|tool| tool.original_name.as_str()).collect();
            originals.sort_unstable();
            let mut expected = NORMAL_TOOLS.to_vec();
            expected.sort_unstable();
            ensure!(originals == expected, "原名集合应与剧本一致：{listed:?}");

            // 模型的 `tools` 字段与接口清单同源：MCP 工具按名字进同一份排序清单。
            let mut occupied = Occupied::start(context, "mcp.discover").await?;
            occupied.say(context, "第一次请求").await?;
            let requested = request_tools(&mock).await?;
            let requested_names = request_tool_names(&requested);
            for name in &names {
                ensure!(
                    requested_names.contains(name),
                    "请求体里缺少 {name}：{requested_names:?}"
                );
            }
            let mut ordered = requested_names.clone();
            ordered.sort();
            ensure!(
                requested_names == ordered,
                "请求体的工具清单应按名字排序：{requested_names:?}"
            );
            occupied.release(context).await?;
            notes.push(format!(
                "stdio：READY，{} 个工具进模型清单（clientInfo={client}）",
                names.len()
            ));

            // L-6：替身发 `notifications/tools/list_changed` 后新增的工具自动进注册表与请求体。
            // 同一个服务端的第二个 server 用 `list_changed` 剧本，避免重启服务端。
            Ok(())
        })
        .await?;

        // 二、清单变化：第一次发现只有 echo，通知之后多出 late_tool（L-6）。
        let root = temp_directory("mcp-list-changed")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "list_changed", "")?;
        let mock = MockServer::start().await?;
        let (before, after) = serve_with(context, &mock, "", &mcp, async {
            wait_status(context, SERVER, McpStatusKind::Ready).await?;
            // 第一次发现的原始清单来自替身证据：不依赖用例读到接口的时机。
            let first = mcp_util::wait_evidence(&evidence, |value| {
                value["toolLists"].as_array().is_some_and(|rounds| !rounds.is_empty())
            })
            .await?;
            let first_round = evidence_round(&first, 0);
            ensure!(first_round == ["echo"], "第一次发现应只有 echo：{first_round:?}");

            // 替身发过通知之后重新发现：证据里出现第二轮清单，其中含新工具。
            let recorded = mcp_util::wait_evidence(&evidence, |value| {
                value["listChangedSent"].as_u64().unwrap_or(0) >= 1
                    && value["toolLists"].as_array().is_some_and(|rounds| rounds.len() >= 2)
            })
            .await?;
            let last = recorded["toolLists"].as_array().map_or(0, Vec::len).saturating_sub(1);
            let discovered = evidence_round(&recorded, last);
            ensure!(
                discovered.contains(&"late_tool".to_owned()),
                "重发现的清单应含 late_tool：{discovered:?}"
            );

            let after = super::tool_names(&wait_tool(context, &visible("late_tool")).await?);
            ensure!(
                after.contains(&visible("late_tool")),
                "清单变化后应出现 late_tool：{after:?}"
            );

            // 下一轮请求的 `tools` 字段同步包含新工具。
            let mut occupied = Occupied::start(context, "mcp.discover").await?;
            occupied.say(context, "变化后的请求").await?;
            let requested = request_tool_names(&request_tools(&mock).await?);
            ensure!(
                requested.contains(&visible("late_tool")) && requested.contains(&visible("echo")),
                "请求体应同步包含新增工具：{requested:?}"
            );
            occupied.release(context).await?;
            Ok((first_round, after))
        })
        .await?;
        ensure!(
            before.len() + 1 == after.len(),
            "清单变化应只新增一个工具：{before:?} → {after:?}"
        );
        notes.push(format!("清单变化：{before:?} → {after:?}（通知触发重新发现）"));

        Ok(notes)
    })
}

/// 命名规则（S-3）：非法字符 / 大写 / 空名 / 数字开头 / 超长 / 撞名，两次发现的可见名一致。
fn name(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();
        let root = temp_directory("mcp-name")?;
        let evidence = root.join("evidence.json");
        let binary = mcp_util::test_binary()?;
        let spec = McpServerSpec::new(
            "naming",
            McpTransport::Stdio {
                command: binary.display().to_string(),
                args: vec![
                    "mock-mcp".to_owned(),
                    "--transport".to_owned(),
                    "stdio".to_owned(),
                    "--scenario".to_owned(),
                    "naming".to_owned(),
                    "--evidence".to_owned(),
                    evidence.display().to_string(),
                ],
                env: BTreeMap::new(),
                cwd: None,
            },
        )
        .with_enabled(true);
        let registry = Arc::new(ToolRegistry::new(ToolPolicy::empty()));
        let manager = McpManager::new(
            McpConfig {
                servers: vec![spec],
                ..McpConfig::default()
            },
            Arc::clone(&registry),
        );
        manager.connect_enabled_all().await;

        let first = manager.tool_snapshot();
        ensure!(
            first.len() == 6,
            "命名剧本应发现 6 个工具：{:?}",
            first.iter().map(|tool| &tool.tool_name).collect::<Vec<_>>()
        );
        let map: BTreeMap<&str, &str> = first
            .iter()
            .map(|tool| (tool.original_name.as_str(), tool.tool_name.as_str()))
            .collect();
        ensure!(
            map["Read-File!"] == "mcp__naming__read_file",
            "非法字符应清洗成 `_`：{map:?}"
        );
        ensure!(map["UPPER_CASE"] == "mcp__naming__upper_case", "大写应小写化：{map:?}");
        ensure!(map["***"] == "mcp__naming__tool", "清洗后为空应退化成 `tool`：{map:?}");
        ensure!(
            map["9lives"] == "mcp__naming__t_9lives",
            "不是字母开头应补 `t_` 前缀：{map:?}"
        );
        // 撞名：原文排序里 `Read-File!` 在前，`read_file` 后到者追加哈希后缀。
        let collided = map["read_file"];
        ensure!(
            collided.starts_with("mcp__naming__read_file_") && collided.len() == "mcp__naming__read_file_".len() + 8,
            "撞名应追加 8 位哈希后缀：{collided}"
        );
        // 超长：截断 + 哈希后缀，总长不超过 64。
        let long = map["long_tool_name_that_exceeds_the_sixty_four_character_limit_by_far"];
        ensure!(
            long.len() == agent_mcp::MAX_NAME_LEN,
            "超长名应截断到 64 字符并带哈希后缀：{long}"
        );
        ensure!(
            long.starts_with("mcp__naming__long_tool_name"),
            "超长名应保留前缀与工具名开头：{long}"
        );
        ensure!(
            long.rsplit('_')
                .next()
                .is_some_and(|hash| { hash.len() == 8 && hash.chars().all(|character| character.is_ascii_hexdigit()) }),
            "超长名的后缀应是 8 位十六进制哈希：{long}"
        );
        for tool in &first {
            ensure!(
                valid_name(&tool.tool_name),
                "可见名应满足 `^[a-z][a-z0-9_]{{1,63}}$`：{}",
                tool.tool_name
            );
        }
        notes.push(format!("命名规则：{map:?}"));

        // 两次发现（重连后再来一遍）必须产出完全相同的可见名。
        manager.reconnect("naming").await?;
        let second = manager.tool_snapshot();
        let names_of = |snapshot: &[agent_mcp::McpToolVO]| {
            snapshot
                .iter()
                .map(|tool| (tool.original_name.clone(), tool.tool_name.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        ensure!(
            names_of(&first) == names_of(&second),
            "两次发现的可见名必须一致：{:?} vs {:?}",
            names_of(&first),
            names_of(&second)
        );
        ensure!(manager.tool_snapshot().len() == 6, "重连后应重新发现全部工具");
        notes.push("两次发现的可见名一致".to_owned());

        manager.shutdown_all().await;
        let recorded = mcp_util::evidence(&evidence)?;
        ensure!(
            recorded["scenario"] == Value::String("naming".to_owned()),
            "证据文件应属于命名剧本：{recorded}"
        );
        // 上下文只用于临时目录之外的临时目录命名，这里显式标注它的用途。
        let _ = context.token();
        Ok(notes)
    })
}

/// 外部 schema 原样透传（S-2）：接口的 `parameters` 与请求体的 `parameters` 等于剧本的 `inputSchema`。
fn schema(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-schema")?;
        let evidence = root.join("evidence.json");
        let mcp = single_stdio(&evidence, "schema", "")?;
        let expected: Value = serde_json::from_str(WEIRD_SCHEMA).context("剧本 schema 不是 JSON")?;
        serve_with(context, &mock, "", &mcp, async {
            wait_status(context, SERVER, McpStatusKind::Ready).await?;
            let listed = tools(context, Some(SERVER)).await?;
            ensure!(listed.len() == 1, "schema 剧本只有一个工具：{listed:?}");
            let parameters: Value =
                serde_json::from_str(&listed[0].parameters).context("`parameters` 不是 JSON 文本")?;
            ensure!(
                parameters == expected,
                "`tool/list` 的 parameters 应原样等于剧本的 inputSchema：\n{parameters}\n{expected}"
            );

            // 请求体里的 parameters 与接口同源。
            let mut occupied = Occupied::start(context, "mcp.schema").await?;
            occupied.say(context, "schema 请求").await?;
            let requested = request_tools(&mock).await?;
            let tool = requested
                .iter()
                .find(|tool| tool["function"]["name"] == Value::String(visible("weird_schema")))
                .context("请求体里应有 weird_schema")?;
            ensure!(
                tool["function"]["parameters"] == expected,
                "请求体的 parameters 应与 inputSchema 逐字段相等：{}",
                tool["function"]["parameters"]
            );
            ensure!(
                parameters["x-custom"]["keep"] == Value::Bool(true)
                    && parameters["properties"]["mode"]["anyOf"]
                        .as_array()
                        .is_some_and(|items| items.len() == 2)
                    && parameters["required"] == serde_json::json!(["mode"]),
                "方言键与嵌套结构不应被剥掉：{parameters}"
            );
            occupied.release(context).await?;
            Ok(())
        })
        .await?;
        notes.push(format!("schema 原样透传：{expected}"));
        Ok(notes)
    })
}

/// 一条 stdio 替身的 `mcp.toml`（默认 server `id`）。
fn single_stdio(evidence: &Path, scenario: &str, extra: &str) -> Result<String> {
    Ok(format!(
        "{}{}",
        mcp_util::header(5000, 0),
        mcp_util::stdio_server(SERVER, true, scenario, evidence, None, extra)?
    ))
}

/// 证据里第 `round` 次 `tools/list` 返回的工具原名（越界时为空）。
fn evidence_round(recorded: &Value, round: usize) -> Vec<String> {
    recorded["toolLists"]
        .as_array()
        .and_then(|rounds| rounds.get(round))
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// 可见名是否满足 `^[a-z][a-z0-9_]{1,63}$`。
fn valid_name(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let rest = characters.collect::<Vec<_>>();
    !rest.is_empty()
        && rest.len() <= 63
        && rest
            .iter()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || *character == '_')
}
