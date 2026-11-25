//! `[tools]` 配置装载与校验（P-7）：非法配置是启动期硬错误，文案点名字段。
//!
//! 进程内执行：直接把 TOML 文本喂给 `lib_bin_cli::config::parse_llm`（服务端启动时走同一条
//! 装载路径），因此不需要起服务端、也不读任何日志。

use anyhow::ensure;
use lib_bin_cli::config::parse_llm;
use types_core::config::tools::{
    BUILTIN_TOOL_NAMES, DEFAULT_FILE_MAX_BYTES, DEFAULT_HTTP_MAX_BYTES, DEFAULT_MAX_PARALLEL_CALLS,
    DEFAULT_MAX_RESULT_CHARS, DEFAULT_TIMEOUT_MS,
};

use super::case;
use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;

/// 合法的最小 `[provider]` 段：只为让整份配置能装载。
const PROVIDER: &str = r#"[provider]
id = "mock"
kind = "openai-compatible"
base_url = "http://127.0.0.1:1/v1"
api_key = ""
default_model = "mock-model"

[[provider.models]]
id = "mock-model"
"#;

/// 配置校验用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("tool.config", CaseKind::Shared, config)]
}

/// 缺省值与非法配置的报错逐条核对。
fn config(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();

        // 一、整段缺省：全部取 types-core 的缺省值，权限表为空。
        let minimal = parse_llm(PROVIDER)?;
        let tools = &minimal.tools;
        ensure!(tools.timeout_ms == DEFAULT_TIMEOUT_MS, "[tools] timeout_ms 缺省值不符");
        ensure!(
            tools.max_result_chars == DEFAULT_MAX_RESULT_CHARS,
            "[tools] max_result_chars 缺省值不符"
        );
        ensure!(
            tools.max_parallel_calls == DEFAULT_MAX_PARALLEL_CALLS,
            "[tools] max_parallel_calls 缺省值不符"
        );
        ensure!(
            tools.file.max_bytes == DEFAULT_FILE_MAX_BYTES && tools.http.max_bytes == DEFAULT_HTTP_MAX_BYTES,
            "[tools.file] / [tools.http] 的字节上限缺省值不符"
        );
        ensure!(
            tools.permissions.is_empty() && tools.file.allow_dirs.is_empty() && tools.http.allow_hosts.is_empty(),
            "缺省权限与白名单应为空（安全侧缺省）"
        );
        notes.push(format!(
            "[tools] 缺省：timeout {}ms、max_result_chars {}、max_parallel_calls {}",
            tools.timeout_ms, tools.max_result_chars, tools.max_parallel_calls
        ));

        // 二、写下的每一项都落地。
        let written = parse_llm(&format!(
            "{PROVIDER}
[tools]
timeout_ms = 900
max_result_chars = 100
max_parallel_calls = 2
max_retries = 1

[tools.permissions]
http_request = \"allow\"
calculator = \"deny\"
file_search = \"confirm\"

[tools.file]
max_bytes = 1024

[tools.http]
allow_hosts = [\"example.com\", \"127.0.0.1:8080\"]
max_bytes = 2048
"
        ))?;
        ensure!(
            written.tools.timeout_ms == 900 && written.tools.max_retries == 1,
            "[tools] 数值未落地"
        );
        ensure!(written.tools.permissions.len() == 3, "[tools.permissions] 未落地");
        ensure!(
            written.tools.http.allow_hosts.len() == 2 && written.tools.http.max_bytes == 2048,
            "[tools.http] 未落地"
        );
        notes.push("[tools] 显式配置全部落地".to_string());

        // 三、非法配置：启动期硬错误，且报错点名字段。
        let rejected: [(&str, String, &str); 6] = [
            (
                "timeout_ms = 0",
                format!("{PROVIDER}\n[tools]\ntimeout_ms = 0\n"),
                "[tools] timeout_ms",
            ),
            (
                "max_result_chars = 0",
                format!("{PROVIDER}\n[tools]\nmax_result_chars = 0\n"),
                "[tools] max_result_chars",
            ),
            (
                "max_parallel_calls = 0",
                format!("{PROVIDER}\n[tools]\nmax_parallel_calls = 0\n"),
                "[tools] max_parallel_calls",
            ),
            (
                "未知工具名",
                format!("{PROVIDER}\n[tools.permissions]\nnope = \"allow\"\n"),
                "[tools.permissions] 未知的工具名",
            ),
            (
                "相对路径白名单",
                format!("{PROVIDER}\n[tools.file]\nallow_dirs = [\"relative/dir\"]\n"),
                "[tools.file] allow_dirs 必须是绝对路径",
            ),
            (
                "带协议的 host",
                format!("{PROVIDER}\n[tools.http]\nallow_hosts = [\"http://example.com\"]\n"),
                "[tools.http] allow_hosts 只能写 host 或 host:port",
            ),
        ];
        for (label, text, expected) in rejected {
            let error = parse_llm(&text).err().map(|error| format!("{error:#}"));
            let message = error.unwrap_or_default();
            ensure!(
                message.contains(expected),
                "{label} 的报错应点名 `{expected}`，实际：{message}"
            );
            notes.push(format!("{label}：{message}"));
        }

        // 四、已知工具名常量与内置工具表一致（写错名字不会被静默忽略）。
        ensure!(
            BUILTIN_TOOL_NAMES == ["calculator", "current_time", "file_read", "file_search", "http_request"],
            "BUILTIN_TOOL_NAMES 与内置工具不一致：{BUILTIN_TOOL_NAMES:?}"
        );

        Ok(notes)
    })
}
