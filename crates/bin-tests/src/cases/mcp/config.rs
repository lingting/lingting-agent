//! `mcp.toml` 的装载与校验（P4-004 / P4-018 的 `mcp.config`）：合法配置逐项落地、
//! 非法配置是启动期硬错误且文案点名 server `id` 与字段。
//!
//! 分两段：
//!
//! - 进程内：直接把 TOML 文本喂给 `lib_bin_cli::config::parse_mcp`（服务端启动时走同一条装载
//!   路径），核对示例、缺省值与全部非法形态的文案，不读任何日志；
//! - 进程级：起一次私有服务端，核对「不可达 server 不阻塞启动」（L-2）、「握手超时」（T-3）、
//!   「`enabled = false` 不产生子进程」（E-4）与「非法配置退出码 2」（E-5）。
//!
//! `mcp.file_access`（S-7）也在这里：它验证的是配置声明到工具自述的落地。

use agent_mcp::FileAccessRules;
use agent_tool::ToolFileAccess;
use anyhow::{Context, Result, ensure};
use lib_bin_cli::config::parse_mcp;
use lib_bin_cli::sdk::{McpStatusKind, RunEventKind, ToolCallStatus as CallStatus, ToolFileAccess as SdkAccess};
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use types_core::config::mcp::{
    DEFAULT_CONNECT_TIMEOUT_MS, DEFAULT_RECONNECT_INITIAL_BACKOFF_MS, DEFAULT_RECONNECT_MAX_BACKOFF_MS,
};
use types_core::config::{McpConcurrencyKind, McpFileAccessKind, McpPermissionKind, McpTransportConfig};

use super::case;
use crate::case::{Case, CaseFuture, CaseKind};
use crate::cases::mcp::{prompt, serve_plain, serve_with, server, servers, startup_exit_code, visible, wait_status};
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::flow::Occupied;
use crate::util::mcp as mcp_util;
use crate::util::server::free_port;
use crate::util::support::temp_directory;

/// README §4.2 的两段示例（`mcp.toml` 的全部内容）。
const SAMPLE: &str = r#"connect_timeout_ms = 10000
reconnect_initial_backoff_ms = 1000
reconnect_max_backoff_ms = 30000
concurrency = "parallel"

[[servers]]
id = "filesystem"
enabled = false
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "D:/code"]
env = { LOG_LEVEL = "info" }
cwd = ""
permission = "allow"
concurrency = "parallel"
file_access = { "*" = "read", "write*" = "write", "pre*write" = "read-write" }

[[servers]]
id = "remote"
enabled = true
transport = "http"
url = "https://example.com/mcp"
headers = { Authorization = "Bearer xxx" }
permission = "confirm"
file_access = { "*" = "none" }
"#;

/// 配置装载用例。
pub(super) fn cases() -> Vec<Case> {
    vec![
        case("mcp.config", CaseKind::SelfServed, config),
        case("mcp.file_access", CaseKind::SelfServed, file_access),
    ]
}

/// 示例装载、缺省值、非法配置文案、`file_access` 的三层优先级，以及进程级的启动行为。
fn config(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();

        // 一、README §4.2 的两段示例逐项落地。
        let sample = parse_mcp(SAMPLE)?;
        ensure!(
            sample.connect_timeout_ms == DEFAULT_CONNECT_TIMEOUT_MS
                && sample.reconnect_initial_backoff_ms == DEFAULT_RECONNECT_INITIAL_BACKOFF_MS
                && sample.reconnect_max_backoff_ms == DEFAULT_RECONNECT_MAX_BACKOFF_MS,
            "示例的超时 / 退避值与缺省值不一致：{sample:?}"
        );
        ensure!(
            sample.concurrency == McpConcurrencyKind::Parallel,
            "示例的顶层 concurrency 应为 parallel"
        );
        ensure!(
            sample.servers.len() == 2,
            "示例应装载出两个 server：{:?}",
            sample.servers
        );

        let stdio = &sample.servers[0];
        ensure!(
            stdio.id == "filesystem" && !stdio.enabled,
            "第一个 server 的 id / enabled 不符"
        );
        ensure!(
            stdio.permission == McpPermissionKind::Allow && stdio.concurrency == Some(McpConcurrencyKind::Parallel),
            "第一个 server 的 permission / concurrency 不符"
        );
        ensure!(
            stdio.file_access.len() == 3
                && stdio.file_access.get("*") == Some(&McpFileAccessKind::Read)
                && stdio.file_access.get("write*") == Some(&McpFileAccessKind::Write)
                && stdio.file_access.get("pre*write") == Some(&McpFileAccessKind::ReadWrite),
            "第一个 server 的 file_access 不符：{:?}",
            stdio.file_access
        );
        match &stdio.transport {
            McpTransportConfig::Stdio {
                command,
                args,
                env,
                cwd,
            } => {
                ensure!(command == "npx", "stdio 的 command 不符：{command}");
                ensure!(args.len() == 3 && args[0] == "-y", "stdio 的 args 不符：{args:?}");
                ensure!(
                    env.get("LOG_LEVEL").map(String::as_str) == Some("info"),
                    "stdio 的 env 不符"
                );
                ensure!(cwd.is_empty(), "stdio 的 cwd 缺省为空");
            }
            other => ensure!(false, "第一个 server 的 transport 应为 stdio：{other:?}"),
        }

        let http = &sample.servers[1];
        ensure!(
            http.id == "remote" && http.enabled,
            "第二个 server 的 id / enabled 不符"
        );
        ensure!(
            http.permission == McpPermissionKind::Confirm && http.concurrency.is_none(),
            "第二个 server 的 permission / concurrency 不符"
        );
        match &http.transport {
            McpTransportConfig::Http { url, headers } => {
                ensure!(url == "https://example.com/mcp", "http 的 url 不符：{url}");
                ensure!(
                    headers.get("Authorization").map(String::as_str) == Some("Bearer xxx"),
                    "http 的 headers 不符"
                );
            }
            other => ensure!(false, "第二个 server 的 transport 应为 http：{other:?}"),
        }
        notes.push("示例配置：顶层三值 + 两个 server（stdio / http）逐项落地".to_owned());

        // 二、缺省：文件缺失等价于零 server（`McpConfig::default()`），`servers` 缺省也为空。
        let defaults = parse_mcp("connect_timeout_ms = 10000\n")?;
        ensure!(defaults.servers.is_empty(), "只写顶层字段时应为零 server");
        ensure!(
            defaults.reconnect_max_backoff_ms == DEFAULT_RECONNECT_MAX_BACKOFF_MS
                && defaults.concurrency == McpConcurrencyKind::Parallel,
            "只写顶层字段时其余项应取缺省值"
        );
        notes.push("缺省：servers 为空，其余取 types-core 的缺省值".to_owned());

        // 三、非法配置一律硬错误，文案点名 server `id` 与字段。
        let rejected: Vec<(&str, String, &str)> = vec![
            ("空文件", String::new(), "mcp.toml 不能为空"),
            (
                "servers 不是数组",
                "servers = 1\n".to_owned(),
                "servers 必须是数组",
            ),
            (
                "缺少 transport",
                "[[servers]]\nid = \"fs\"\ncommand = \"x\"\n".to_owned(),
                "server fs 缺少 transport",
            ),
            (
                "未知 transport",
                "[[servers]]\nid = \"fs\"\ntransport = \"grpc\"\n".to_owned(),
                "server fs 的 transport 不合法",
            ),
            (
                "stdio 写 url",
                "[[servers]]\nid = \"fs\"\ntransport = \"stdio\"\ncommand = \"x\"\nurl = \"https://x\"\n".to_owned(),
                "server fs 是 stdio transport，不能写 url",
            ),
            (
                "http 写 command",
                "[[servers]]\nid = \"fs\"\ntransport = \"http\"\nurl = \"https://x\"\ncommand = \"x\"\n".to_owned(),
                "server fs 是 http transport，不能写 command",
            ),
            (
                "stdio 缺 command",
                "[[servers]]\nid = \"fs\"\ntransport = \"stdio\"\n".to_owned(),
                "server fs 缺少 command",
            ),
            (
                "stdio 的 command 为空",
                "[[servers]]\nid = \"fs\"\ntransport = \"stdio\"\ncommand = \"  \"\n".to_owned(),
                "server fs 的 command 必须是非空字符串",
            ),
            (
                "args 含空串",
                "[[servers]]\nid = \"fs\"\ntransport = \"stdio\"\ncommand = \"x\"\nargs = [\"a\", \"\"]\n".to_owned(),
                "server fs 的 args 不能包含空串",
            ),
            (
                "env 空键",
                "[[servers]]\nid = \"fs\"\ntransport = \"stdio\"\ncommand = \"x\"\nenv = { \"\" = \"v\" }\n".to_owned(),
                "server fs 的 env 不能有空键",
            ),
            (
                "http 的 url 协议不符",
                "[[servers]]\nid = \"fs\"\ntransport = \"http\"\nurl = \"ftp://x/mcp\"\n".to_owned(),
                "server fs 的 url 只支持 http / https",
            ),
            (
                "headers 空键",
                "[[servers]]\nid = \"fs\"\ntransport = \"http\"\nurl = \"https://x\"\nheaders = { \"\" = \"v\" }\n".to_owned(),
                "server fs 的 headers 不能有空键",
            ),
            (
                "id 不合法",
                "[[servers]]\nid = \"FileSystem\"\ntransport = \"stdio\"\ncommand = \"x\"\n".to_owned(),
                "server id 不合法",
            ),
            (
                "id 重复",
                "[[servers]]\nid = \"fs\"\ntransport = \"stdio\"\ncommand = \"x\"\n\n[[servers]]\nid = \"fs\"\ntransport = \"stdio\"\ncommand = \"y\"\n".to_owned(),
                "server id 重复",
            ),
            (
                "permission 取值不合法",
                "[[servers]]\nid = \"fs\"\ntransport = \"stdio\"\ncommand = \"x\"\npermission = \"alow\"\n".to_owned(),
                "server fs 的 permission 取值不合法",
            ),
            (
                "concurrency 取值不合法",
                "[[servers]]\nid = \"fs\"\ntransport = \"stdio\"\ncommand = \"x\"\nconcurrency = \"wide\"\n".to_owned(),
                "server fs 的 concurrency 取值不合法",
            ),
            (
                "file_access 取值不合法",
                "[[servers]]\nid = \"fs\"\ntransport = \"stdio\"\ncommand = \"x\"\nfile_access = { \"*\" = \"rw\" }\n".to_owned(),
                "server fs 的 file_access[*] 取值不合法",
            ),
            (
                "file_access 空键",
                "[[servers]]\nid = \"fs\"\ntransport = \"stdio\"\ncommand = \"x\"\nfile_access = { \"\" = \"read\" }\n".to_owned(),
                "server fs 的 file_access 不能有空模式",
            ),
            (
                "connect_timeout_ms 为 0",
                "connect_timeout_ms = 0\n".to_owned(),
                "mcp.toml connect_timeout_ms 必须大于 0",
            ),
            (
                "reconnect_initial_backoff_ms 为 0",
                "reconnect_initial_backoff_ms = 0\n".to_owned(),
                "mcp.toml reconnect_initial_backoff_ms 必须大于 0",
            ),
        ];
        for (label, text, expected) in rejected {
            let message = parse_mcp(&text)
                .err()
                .map(|error| format!("{error:#}"))
                .unwrap_or_default();
            ensure!(
                message.contains(expected),
                "{label} 的报错应点名 `{expected}`，实际：{message}"
            );
            notes.push(format!("{label}：{message}"));
        }

        // 四、`file_access` 的三层优先级：精确名 > 最具体模式 > 缺省 None（与注册表无关）。
        let rules = FileAccessRules::new([
            ("*".to_owned(), ToolFileAccess::Read),
            ("write*".to_owned(), ToolFileAccess::Write),
            ("pre*write".to_owned(), ToolFileAccess::ReadWrite),
            ("write_file".to_owned(), ToolFileAccess::None),
        ]);
        ensure!(
            rules.access_of("read_file") == ToolFileAccess::Read,
            "只命中 `*` 的工具应为 Read"
        );
        ensure!(
            rules.access_of("write_log") == ToolFileAccess::Write,
            "命中 `write*` 的工具应为 Write"
        );
        ensure!(
            rules.access_of("pre_commit_write") == ToolFileAccess::ReadWrite,
            "命中 `pre*write` 的工具应为 ReadWrite"
        );
        ensure!(
            rules.access_of("write_file") == ToolFileAccess::None,
            "精确名应盖过 `write*`"
        );

        // 去掉精确名后，同一个工具回落到 `write*`。
        let without_exact = FileAccessRules::new([
            ("*".to_owned(), ToolFileAccess::Read),
            ("write*".to_owned(), ToolFileAccess::Write),
            ("pre*write".to_owned(), ToolFileAccess::ReadWrite),
        ]);
        ensure!(
            without_exact.access_of("write_file") == ToolFileAccess::Write,
            "去掉精确名后 `write_file` 应命中 `write*`"
        );
        ensure!(
            without_exact.access_of("write_file_2") == ToolFileAccess::Write,
            "`*` 匹配任意长度：`write*` 应命中 `write_file_2`"
        );

        // 同分模式（非通配字符数相同）按字典序取第一个，保证同一份配置结果确定。
        let tie = FileAccessRules::new([
            ("a*b".to_owned(), ToolFileAccess::Write),
            ("*ab".to_owned(), ToolFileAccess::Read),
        ]);
        ensure!(
            tie.access_of("ab") == ToolFileAccess::Read,
            "同分模式应取字典序最小的 `*ab`（Read）"
        );

        // 没有任何键命中时是 None；连续 `*` 折叠等价，`?` 按字面处理。
        let none = FileAccessRules::new([("read*".to_owned(), ToolFileAccess::Read)]);
        ensure!(
            none.access_of("delete_all") == ToolFileAccess::None,
            "无命中时应为 None"
        );
        let folded = FileAccessRules::new([("re**ad".to_owned(), ToolFileAccess::Read)]);
        ensure!(
            folded.access_of("read") == ToolFileAccess::Read,
            "连续 `*` 应折叠为一个 `*`"
        );
        let literal = FileAccessRules::new([("re?ad".to_owned(), ToolFileAccess::Read)]);
        ensure!(
            literal.access_of("read") == ToolFileAccess::None,
            "`?` 不是通配符：`re?ad` 不应命中 `read`"
        );
        ensure!(literal.access_of("re?ad") == ToolFileAccess::Read, "`?` 只按字面匹配");
        notes.push("file_access：精确名 > 最具体模式 > None，同分取字典序、连续 `*` 折叠".to_owned());

        // 五、进程级：不可达 server 不阻塞启动、`enabled = false` 不产生子进程（L-2 / E-4）。
        let root = temp_directory("mcp-config")?;
        let evidence_ok = root.join("ok.json");
        let evidence_off = root.join("off.json");
        let pid_off = root.join("off.pid");
        let closed_port = free_port()?;
        let mcp = format!(
            "{}{}\n\n{}\n\n{}",
            mcp_util::header(5000, 0),
            mcp_util::stdio_server("ok", true, "normal", &evidence_ok, None, "")?,
            mcp_util::stdio_server("off", false, "normal", &evidence_off, Some(&pid_off), "")?,
            mcp_util::http_server("dead", &format!("http://127.0.0.1:{closed_port}/mcp"), ""),
        );
        let dead_error = serve_plain(context, "", &mcp, async {
            let list = servers(context).await?;
            ensure!(
                list.iter().map(|server| server.id.as_str()).collect::<Vec<_>>() == ["dead", "off", "ok"],
                "server/list 应按 id 升序且含 disabled：{list:?}"
            );

            let ok = wait_status(context, "ok", McpStatusKind::Ready).await?;
            ensure!(
                ok.status == McpStatusKind::Ready && ok.transport == "stdio" && ok.tool_count == 6,
                "stdio server 应 READY 并发现 6 个工具：{ok:?}"
            );
            ensure!(ok.last_connected_at_ms.is_some(), "READY 的 server 应有连接时间");

            let off = server(context, "off").await?;
            ensure!(
                !off.enabled && off.status == McpStatusKind::Disabled && off.tool_count == 0,
                "enabled = false 的 server 应 DISABLED 且没有工具：{off:?}"
            );
            ensure!(
                !pid_off.exists(),
                "enabled = false 的 server 不应产生子进程，PID 文件却存在：{}",
                pid_off.display()
            );

            let dead = wait_status(context, "dead", McpStatusKind::Failed).await?;
            ensure!(
                dead.status == McpStatusKind::Failed,
                "不可达的 server 应 FAILED（服务端照常启动）：{dead:?}"
            );
            Ok(dead.last_error.unwrap_or_default())
        })
        .await?;
        ensure!(dead_error.contains("MCP 服务器"), "失败文案应是固定中文：{dead_error}");
        ensure!(
            !dead_error.contains("127.0.0.1") && !dead_error.contains("http://"),
            "失败文案不应含地址：{dead_error}"
        );
        notes.push(format!(
            "进程级：READY / DISABLED / FAILED（{}），disabled 无子进程",
            dead_error
        ));

        // 六、握手超时（T-3）：端口可连但一个字节都不回，`connect_timeout_ms` 极小。
        let quiet = QuietListener::start().await?;
        let mcp = format!(
            "{}{}",
            mcp_util::header(300, 0),
            mcp_util::http_server("dead", &format!("http://127.0.0.1:{}/mcp", quiet.port()), "")
        );
        let timeout_error = serve_plain(context, "", &mcp, async {
            let dead = wait_status(context, "dead", McpStatusKind::Failed).await?;
            ensure!(
                dead.status == McpStatusKind::Failed,
                "不响应的 server 应 FAILED：{dead:?}"
            );
            Ok(dead.last_error.unwrap_or_default())
        })
        .await?;
        ensure!(
            timeout_error.contains("连接超时") && timeout_error.contains("300"),
            "握手超时应报固定文案与上限：{timeout_error}"
        );
        drop(quiet);
        notes.push(format!("握手超时：{timeout_error}"));

        // 七、非法配置是启动期硬错误（E-5）：退出码 2，服务端不进入装配。
        let broken = temp_directory("mcp-broken")?;
        mcp_util::write_config(
            &broken,
            "[[servers]]\nid = \"FS\"\ntransport = \"stdio\"\ncommand = \"x\"\n",
        )?;
        let code = startup_exit_code(&broken).await?;
        ensure!(code == Some(2), "非法 mcp.toml 应以退出码 2 结束，实际 {code:?}");
        notes.push("非法 mcp.toml：服务端退出码 2".to_owned());

        Ok(notes)
    })
}

/// `file_access` 的四级优先级、无命中与写声明的串行执行（S-7）。
fn file_access(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();
        let mock = MockServer::start().await?;
        let root = temp_directory("mcp-file-access")?;
        let evidence = root.join("evidence.json");
        let rules = "permission = 'allow'\nfile_access = { '*' = 'read', 'write*' = 'write', 'pre*write' = 'read-write', 'write_file' = 'none' }\n";
        let mcp = format!(
            "{}{}",
            mcp_util::header(5000, 0),
            mcp_util::stdio_server("mock", true, "normal", &evidence, None, rules)?
        );
        let names: Vec<String> = ["read_file", "write_log", "pre_commit_write", "write_file", "delete_all"]
            .iter()
            .map(|name| visible(name))
            .collect();

        // 一、四个声明层各命中一个工具，没有任何键命中的工具是 None。
        let first = call_round(context, &mock, &mcp, &names).await?;
        let map = access_map(&first)?;
        ensure!(
            map[&visible("read_file")] == SdkAccess::Read,
            "只命中 `*` 的 read_file 应为 READ：{map:?}"
        );
        ensure!(
            map[&visible("write_log")] == SdkAccess::Write,
            "命中 `write*` 的 write_log 应为 WRITE：{map:?}"
        );
        ensure!(
            map[&visible("pre_commit_write")] == SdkAccess::ReadWrite,
            "命中 `pre*write` 的 pre_commit_write 应为 READ_WRITE：{map:?}"
        );
        ensure!(
            map[&visible("write_file")] == SdkAccess::None,
            "精确名应盖过 `write*`（None）：{map:?}"
        );
        ensure!(
            map[&visible("delete_all")] == SdkAccess::Read,
            "只命中 `*` 的 delete_all 应为 READ（`*` 命中一切）：{map:?}"
        );
        ensure!(
            first.iter().all(|record| record.status == CallStatus::Ok),
            "声明只影响并发与审计，不影响执行结果：{first:?}"
        );
        notes.push(format!("四级优先级：{map:?}"));

        // 二、写声明的工具在同一轮里被串行：替身证据里的调用区间两两不重叠。
        let recorded = mcp_util::evidence(&evidence)?;
        let mut spans: Vec<(String, i64, i64)> =
            [visible("write_log"), visible("pre_commit_write"), visible("write_file")]
                .iter()
                .filter_map(|name| {
                    let original = name.trim_start_matches("mcp__mock__");
                    mcp_util::call_of(&recorded, original)
                        .and_then(|call| mcp_util::call_span(call).ok())
                        .map(|(started, ended)| (name.clone(), started, ended))
                })
                .collect();
        // 按开始时间排序后再两两比较：断言的是「不重叠」，不依赖调用在脚本里的顺序。
        spans.sort_by_key(|(_, started, _)| *started);
        ensure!(spans.len() == 3, "三个写声明工具都应有调用证据：{spans:?}");
        for (left, right) in spans.iter().zip(spans.iter().skip(1)) {
            ensure!(
                right.1 >= left.2,
                "写声明的工具应串行（{} 结束 {} / {} 开始 {}）",
                left.0,
                left.2,
                right.0,
                right.1
            );
        }
        notes.push(format!(
            "写声明串行：{}",
            spans
                .iter()
                .map(|(name, started, ended)| format!("{name} {started}~{ended}"))
                .collect::<Vec<_>>()
                .join("，")
        ));

        // 三、同一份配置两次启动结果一致（同分模式也只取一个确定值）。
        let again = call_round(context, &mock, &mcp, &names).await?;
        ensure!(access_map(&again)? == map, "同一份配置两次启动的 fileAccess 必须一致");

        // 四、去掉精确名后同一个工具回落到 `write*`。
        let without_exact =
            "permission = 'allow'\nfile_access = { '*' = 'read', 'write*' = 'write', 'pre*write' = 'read-write' }\n";
        let mcp = format!(
            "{}{}",
            mcp_util::header(5000, 0),
            mcp_util::stdio_server("mock", true, "normal", &evidence, None, without_exact)?
        );
        let fallback = call_round(context, &mock, &mcp, &[visible("write_file")]).await?;
        let map = access_map(&fallback)?;
        ensure!(
            map[&visible("write_file")] == SdkAccess::Write,
            "去掉精确名后 write_file 应命中 `write*`（WRITE）：{map:?}"
        );
        notes.push("去掉精确名：write_file → WRITE".to_owned());
        // 五、没有任何键命中（连 `*` 都没写）时是 NONE。
        let no_pattern = "permission = 'allow'\nfile_access = { 'read*' = 'read' }\n";
        let mcp = format!(
            "{}{}",
            mcp_util::header(5000, 0),
            mcp_util::stdio_server("mock", true, "normal", &evidence, None, no_pattern)?
        );
        let unmatched = access_map(&call_round(context, &mock, &mcp, &[visible("write_file")]).await?)?;
        ensure!(
            unmatched[&visible("write_file")] == SdkAccess::None,
            "没有任何键命中时应为 NONE：{unmatched:?}"
        );
        notes.push("无命中：write_file → NONE".to_owned());
        Ok(notes)
    })
}

/// 一轮里按顺序调用给定的可见名，返回审计记录（`api/tool/audit/list`）。
async fn call_round(
    context: &TestContext,
    mock: &MockServer,
    mcp: &str,
    names: &[String],
) -> Result<Vec<lib_bin_cli::sdk::ToolCallVO>> {
    let calls: Vec<serde_json::Value> = names
        .iter()
        .map(|name| json!({"name": name, "arguments": {"path": "fixture.txt"}}))
        .collect();
    let spec = json!({ "calls": calls });
    serve_with(context, mock, "", mcp, async {
        let mut occupied = Occupied::start(context, "mcp.file_access").await?;
        let (sent, drained) = occupied.say(context, &prompt(spec)).await?;
        ensure!(
            drained.terminal.event == RunEventKind::RunCompleted,
            "文件操作声明不影响 Run 收尾，实际 {:?}",
            drained.terminal.event
        );
        let records = crate::cases::tool::audit(context, &sent.run.id).await?;
        ensure!(
            records.len() == names.len(),
            "应有一次调用一条审计：{} != {}",
            records.len(),
            names.len()
        );
        occupied.release(context).await?;
        Ok(records)
    })
    .await
}

/// 审计记录 → 「可见名 → `fileAccess`」。
fn access_map(records: &[lib_bin_cli::sdk::ToolCallVO]) -> Result<std::collections::BTreeMap<String, SdkAccess>> {
    let mut map = std::collections::BTreeMap::new();
    for record in records {
        map.insert(record.tool_name.clone(), record.file_access);
    }
    Ok(map)
}

/// 只接受连接、一个字节都不回的服务端：客户端握手必然超时（T-3）。
struct QuietListener {
    port: u16,
    accept: tokio::task::JoinHandle<()>,
}

impl QuietListener {
    /// 绑定随机端口并开始接受连接（读到的请求一律不回应）。
    async fn start() -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.context("绑定静默端口失败")?;
        let port = listener.local_addr().context("读取静默端口失败")?.port();
        let accept = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buffer = [0u8; 1024];
                    // 一直读到对端离开：请求永远等不到响应。
                    loop {
                        match socket.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(_) => {}
                        }
                    }
                });
            }
        });
        Ok(Self { port, accept })
    }

    /// 监听端口。
    fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for QuietListener {
    fn drop(&mut self) {
        self.accept.abort();
    }
}
