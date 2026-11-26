//! MCP 用例的测试设施：`mcp.toml` 生成、替身子进程与证据读取（P4-017 / P4-018）。
//!
//! 三个语义各只有一处实现：
//!
//! - **配置生成**：`mcp.toml` 是应用根目录下的独立文件（`lib_core::directorys::MCP_CONFIG_FILE`），
//!   stdio server 指向测试二进制自己的隐藏子命令 `mock-mcp`，HTTP server 指向替身的监听地址；
//! - **替身进程**：HTTP 模式由用例自己起停，stdio 模式由被测服务端当子进程启动，两边都靠
//!   `--evidence` 的结构化快照取证（不读日志）；
//! - **进程存活**：读系统进程表判断 PID 是否存在（不调用系统管理命令、不解析日志）。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use lib_core::directorys::MCP_CONFIG_FILE;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::sleep;

use crate::util::server::free_port;
use crate::util::support::temp_directory;

/// 等替身监听的超时。
const PORT_TIMEOUT: Duration = Duration::from_secs(10);
/// 端口轮询间隔。
const PORT_INTERVAL: Duration = Duration::from_millis(20);
/// 等证据文件满足条件的超时。
const EVIDENCE_TIMEOUT: Duration = Duration::from_secs(15);
/// 证据轮询间隔。
const EVIDENCE_INTERVAL: Duration = Duration::from_millis(20);

/// 把 `mcp.toml` 写进应用根目录：服务端启动时按同一条装载路径读它。
pub(crate) fn write_config(home: &Path, text: &str) -> Result<PathBuf> {
    let path = home.join(MCP_CONFIG_FILE);
    std::fs::write(&path, text).with_context(|| format!("写 mcp.toml 失败：{}", path.display()))?;
    Ok(path)
}

/// 测试二进制自身：stdio 替身由被测服务端用 `command` + `args` 启动它。
pub(crate) fn test_binary() -> Result<PathBuf> {
    std::env::current_exe().context("获取测试入口路径失败")
}

/// TOML 字面量字符串：Windows 路径里的 `\` 原样保留，不做转义。
fn literal(text: &str) -> String {
    format!("'{text}'")
}

/// 一条 stdio 替身的 `[[servers]]` 段：其余字段（权限、并发、`file_access`）由 `extra` 追加。
pub(crate) fn stdio_server(
    id: &str,
    enabled: bool,
    scenario: &str,
    evidence: &Path,
    pid_file: Option<&Path>,
    extra: &str,
) -> Result<String> {
    let binary = test_binary()?;
    let mut args = vec![
        "mock-mcp".to_owned(),
        "--transport".to_owned(),
        "stdio".to_owned(),
        "--scenario".to_owned(),
        scenario.to_owned(),
        "--evidence".to_owned(),
        evidence.display().to_string(),
    ];
    if let Some(pid_file) = pid_file {
        args.push("--pid-file".to_owned());
        args.push(pid_file.display().to_string());
    }
    let args = args
        .iter()
        .map(|argument| literal(argument))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "[[servers]]\nid = {}\nenabled = {enabled}\ntransport = 'stdio'\ncommand = {}\nargs = [{}]\n{}",
        literal(id),
        literal(&binary.display().to_string()),
        args,
        extra
    ))
}

/// 一条 http 替身的 `[[servers]]` 段。
pub(crate) fn http_server(id: &str, url: &str, extra: &str) -> String {
    format!(
        "[[servers]]\nid = {}\nenabled = true\ntransport = 'http'\nurl = {}\n{}",
        literal(id),
        literal(url),
        extra
    )
}

/// `mcp.toml` 的顶层：一个不可达 server 也要能启动（L-2），因此超时值由用例显式给出。
pub(crate) fn header(connect_timeout_ms: u64, max_backoff_ms: u64) -> String {
    format!("connect_timeout_ms = {connect_timeout_ms}\nreconnect_max_backoff_ms = {max_backoff_ms}\n")
}

/// 起一个 HTTP 替身：PORT 由系统分配，`--pid-file` 与证据文件都在用例自己的临时目录里。
pub(crate) struct HttpMock {
    child: Child,
    url: String,
    port: u16,
    evidence: PathBuf,
    pid_file: PathBuf,
}

impl HttpMock {
    /// 启动替身并等它开始监听。
    pub(crate) async fn start(scenario: &str) -> Result<Self> {
        let root = temp_directory("mcp-http")?;
        let evidence = root.join("evidence.json");
        let pid_file = root.join("mock.pid");
        let port = free_port()?;
        let child = Command::new(test_binary()?)
            .arg("mock-mcp")
            .arg("--transport")
            .arg("http")
            .arg("--port")
            .arg(port.to_string())
            .arg("--scenario")
            .arg(scenario)
            .arg("--evidence")
            .arg(&evidence)
            .arg("--pid-file")
            .arg(&pid_file)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("启动 MCP HTTP 替身失败")?;
        let mock = Self {
            child,
            url: format!("http://127.0.0.1:{port}/mcp"),
            port,
            evidence,
            pid_file,
        };
        mock.wait_listening().await?;
        Ok(mock)
    }

    /// 替身的 MCP 地址（服务端配置里的 `url`）。
    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    /// 证据文件路径。
    pub(crate) fn evidence(&self) -> &Path {
        &self.evidence
    }

    /// PID 文件路径。
    pub(crate) fn pid_file(&self) -> &Path {
        &self.pid_file
    }

    /// 停止替身并回收子进程。
    pub(crate) async fn stop(mut self) -> Result<()> {
        let _ = self.child.start_kill();
        self.child.wait().await.context("回收 MCP HTTP 替身失败")?;
        Ok(())
    }

    /// 轮询端口，直到替身开始接受连接。
    async fn wait_listening(&self) -> Result<()> {
        let deadline = Instant::now() + PORT_TIMEOUT;
        loop {
            if TcpStream::connect(("127.0.0.1", self.port)).await.is_ok() {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "MCP HTTP 替身没有在 {} 秒内监听端口 {}",
                PORT_TIMEOUT.as_secs(),
                self.port
            );
            sleep(PORT_INTERVAL).await;
        }
    }
}

impl Drop for HttpMock {
    fn drop(&mut self) {
        // 用例失败也要杀掉替身：`kill_on_drop` 兜底。
        let _ = self.child.start_kill();
    }
}

/// 读一次证据文件。
pub(crate) fn evidence(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path).with_context(|| format!("读证据文件失败：{}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("证据文件不是 JSON：{}", path.display()))
}

/// 轮询证据文件直到满足条件（替身是另一个进程，观察有延迟）。
pub(crate) async fn wait_evidence(path: &Path, done: impl Fn(&Value) -> bool) -> Result<Value> {
    let deadline = Instant::now() + EVIDENCE_TIMEOUT;
    loop {
        if let Ok(value) = evidence(path)
            && done(&value)
        {
            return Ok(value);
        }
        ensure!(
            Instant::now() < deadline,
            "证据文件 {} 在 {} 秒内没有满足条件的内容",
            path.display(),
            EVIDENCE_TIMEOUT.as_secs()
        );
        sleep(EVIDENCE_INTERVAL).await;
    }
}

/// 读 PID 文件里的进程号（替身启动时写入）。
pub(crate) fn pid_of(path: &Path) -> Result<u32> {
    let text = std::fs::read_to_string(path).with_context(|| format!("读 PID 文件失败：{}", path.display()))?;
    text.trim()
        .parse::<u32>()
        .with_context(|| format!("PID 文件内容不是进程号：{}", path.display()))
}

/// 轮询等待进程消失：关闭流程是异步的，最多等 10 秒。
pub(crate) async fn wait_process_gone(pid: u32) -> bool {
    let deadline = Instant::now() + PORT_TIMEOUT;
    while Instant::now() < deadline {
        if !process_alive(pid) {
            return true;
        }
        sleep(Duration::from_millis(50)).await;
    }
    !process_alive(pid)
}

/// 进程是否还在（读系统进程表，不调用系统管理命令）。
pub(crate) fn process_alive(pid: u32) -> bool {
    let pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).is_some()
}

/// 证据里的调用清单。
pub(crate) fn calls(evidence: &Value) -> &[Value] {
    evidence["calls"].as_array().map(Vec::as_slice).unwrap_or_default()
}

/// 证据里按原名取第一次调用。
pub(crate) fn call_of<'a>(evidence: &'a Value, name: &str) -> Option<&'a Value> {
    calls(evidence)
        .iter()
        .find(|call| call["name"] == Value::String(name.to_owned()))
}

/// 一次调用的开始 / 结束时间（毫秒）；未结束时返回 `None`。
pub(crate) fn call_span(call: &Value) -> Result<(i64, i64)> {
    let started = call["startedAtMs"].as_i64().context("证据里缺少 startedAtMs")?;
    let ended = call["endedAtMs"]
        .as_i64()
        .with_context(|| format!("调用还没有结束：{call}"))?;
    Ok((started, ended))
}
