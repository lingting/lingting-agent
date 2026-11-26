//! 传输层构造：两种 transport 的差异只出现在这里（README §4.3）。
//!
//! 状态机、握手、发现、调用、关闭全部共用 [`crate::client::McpClient`] 的一份实现；
//! 本模块只回答「怎么把一个 server 变成可 `serve` 的 transport」：
//!
//! - stdio：`tokio::process::Command` + `kill_on_drop(true)` + 全 piped，stderr 接到 `debug` 日志；
//! - http：`reqwest` 客户端 + 静态请求头，SSE 重连交给 rmcp（本项目不额外实现）。
//!
//! 请求头与命令行不写进任何错误文案（判据 E-2 / E-3）：出错只有固定文案 + `server_id`。

use std::collections::HashMap;
use std::process::Stdio;

use reqwest::header::{HeaderName, HeaderValue};
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::transport::streamable_http_client::{StreamableHttpClientTransport, StreamableHttpClientTransportConfig};
use tokio::process::Command;

use crate::config::{McpServerSpec, McpTransport};
use crate::error::McpError;

/// 建 stdio transport：命令不存在 / 无权限时返回「服务器不可用」。
pub(crate) fn stdio(spec: &McpServerSpec) -> Result<TokioChildProcess, McpError> {
    let McpTransport::Stdio {
        command,
        args,
        env,
        cwd,
    } = &spec.transport
    else {
        return Err(McpError::unavailable(&spec.id));
    };

    let mut builder = Command::new(command);
    // `kill_on_drop` 是最后兜底：`TokioChildProcess` 自己也会在 drop 时杀掉直接子进程。
    builder.args(args).kill_on_drop(true);
    if let Some(cwd) = cwd {
        builder.current_dir(cwd);
    }
    // `env` 只追加不覆盖：`PATH` 等继续从当前进程继承。
    for (key, value) in env {
        builder.env(key, value);
    }

    let (transport, stderr) = TokioChildProcess::builder(builder)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            // 只记「起不来」，不记命令行：命令的路径可能带用户目录。
            tracing::debug!(server = %spec.id, %error, "MCP 子进程启动失败");
            McpError::unavailable(&spec.id)
        })?;
    if let Some(stderr) = stderr {
        spawn_stderr_reader(spec.id.clone(), stderr);
    }
    tracing::debug!(server = %spec.id, pid = ?transport.id(), "MCP 子进程已启动");
    Ok(transport)
}

/// 建 http transport：请求头不合法时返回「服务器不可用」。
pub(crate) fn http(spec: &McpServerSpec) -> Result<StreamableHttpClientTransport<reqwest::Client>, McpError> {
    let McpTransport::Http { url, headers } = &spec.transport else {
        return Err(McpError::unavailable(&spec.id));
    };

    let client = reqwest::Client::builder().build().map_err(|error| {
        tracing::debug!(server = %spec.id, %error, "MCP HTTP 客户端构建失败");
        McpError::unavailable(&spec.id)
    })?;

    let mut custom: HashMap<HeaderName, HeaderValue> = HashMap::with_capacity(headers.len());
    for (name, value) in headers {
        let Ok(header_name) = name.parse::<HeaderName>() else {
            tracing::debug!(server = %spec.id, header = %name, "MCP 请求头名称不合法");
            return Err(McpError::unavailable(&spec.id));
        };
        let Ok(header_value) = value.parse::<HeaderValue>() else {
            // 只记「哪个头不合法」，不记头的值（可能是凭证）。
            tracing::debug!(server = %spec.id, header = %name, "MCP 请求头的值不合法");
            return Err(McpError::unavailable(&spec.id));
        };
        custom.insert(header_name, header_value);
    }

    let config = StreamableHttpClientTransportConfig::with_uri(url.clone()).custom_headers(custom);
    Ok(StreamableHttpClientTransport::with_client(client, config))
}

/// 把子进程的 stderr 接到 `debug` 日志（不落事件文件，也不进错误文案）。
fn spawn_stderr_reader(server_id: String, stderr: tokio::process::ChildStderr) {
    tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(server = %server_id, "mcp stderr: {line}");
        }
    });
}
