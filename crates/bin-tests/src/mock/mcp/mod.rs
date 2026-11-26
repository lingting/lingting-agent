//! MCP 测试替身（P4-017）：一个会讲 MCP 的子进程，stdio 与 HTTP 两种传输共用一份实现。
//!
//! 用途：MCP 用例需要一个「能被启动、能握手、能按剧本返回」的 server，而本项目**不实现** MCP
//! server（Out of Scope）。替身因此只存在于 `bin-tests`：
//!
//! - 入口是隐藏子命令 `bin-tests mock-mcp`（`--help` 里不出现，见 `cli`）；
//! - 剧本见 [`scenario`]：常规、命名、schema、结果映射、失败、崩溃、慢响应、清单变化；
//! - 观察到的客户端信息与每次调用写进 `--evidence` 的结构化快照，用例只读结构化数据，
//!   不读日志（`AGENTS.md` 的测试数据采集条款）。

mod scenario;
mod server;

use std::path::PathBuf;

use anyhow::Result;

pub(crate) use scenario::{IMAGE_DATA, IMAGE_TEXT, Scenario, WEIRD_SCHEMA, WRITE_DELAY_MS, structured_content};

/// 替身的传输方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MockTransport {
    /// 走 stdin / stdout：由被测服务端的 stdio transport 启动为子进程。
    Stdio,
    /// 走本地 streamable HTTP：由用例自己启动，服务端按 `url` 连。
    Http,
}

impl MockTransport {
    /// 传输方式标签（证据文件、日志与配置里都用它）。
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
        }
    }

    /// 解析 `--transport` 的取值。
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "stdio" => Some(Self::Stdio),
            "http" => Some(Self::Http),
            _ => None,
        }
    }
}

/// 一次替身运行的参数。
pub(crate) struct MockConfig {
    /// 传输方式。
    pub(crate) transport: MockTransport,
    /// HTTP 监听端口；`stdio` 时忽略。
    pub(crate) port: Option<u16>,
    /// 剧本。
    pub(crate) scenario: Scenario,
    /// 证据文件路径。
    pub(crate) evidence: Option<PathBuf>,
    /// PID 文件路径（「无孤儿进程」断言用）。
    pub(crate) pid_file: Option<PathBuf>,
}

/// 运行替身直到进程结束（崩溃剧本会在这里直接退出，属预期行为）。
pub(crate) async fn serve(config: MockConfig) -> Result<()> {
    server::serve(
        config.transport,
        config.port,
        config.scenario,
        config.evidence,
        config.pid_file,
    )
    .await
}
