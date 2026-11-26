//! `mcp.toml` 的装载与校验（P4-004 / P4-018 的 `mcp.config`）：合法配置逐项落地、
//! 非法配置是启动期硬错误且文案点名 server `id` 与字段。
//!
//! 进程内执行：直接把 TOML 文本喂给 `lib_bin_cli::config::parse_mcp`（服务端启动时走同一条
//! 装载路径），因此不需要起服务端、也不读任何日志。文件缺失等价于零 server 由装载层负责
//! （`McpConfig::default()`），这里核对缺省值本身。

use agent_mcp::FileAccessRules;
use agent_tool::ToolFileAccess;
use anyhow::ensure;
use lib_bin_cli::config::parse_mcp;
use types_core::config::mcp::{
    DEFAULT_CONNECT_TIMEOUT_MS, DEFAULT_RECONNECT_INITIAL_BACKOFF_MS, DEFAULT_RECONNECT_MAX_BACKOFF_MS,
};
use types_core::config::{McpConcurrencyKind, McpFileAccessKind, McpPermissionKind, McpTransportConfig};

use super::case;
use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;

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
    vec![case("mcp.config", CaseKind::Shared, config)]
}

/// 示例装载、缺省值、非法配置文案与 `file_access` 的三层优先级。
fn config(_context: &TestContext) -> CaseFuture<'_> {
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

        Ok(notes)
    })
}
