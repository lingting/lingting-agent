//! 服务端子进程：端口、就绪探测、输出重定向与停止。
//!
//! 测试进程只作为客户端，服务端一律以 `bin-agent server` 子进程运行：关闭标记、日志初始化
//! 与信号处理都是进程级单例，同进程起多个实例会互相干扰。

use anyhow::{Context, Result, bail};
use framework_http::{HttpClient, HttpRequest};
use lib_bin_cli::client::Client;
use lib_bin_cli::sdk::ApiClient;
use lib_core::directorys::{CONFIG_FILE, ENV_HOME};
use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{LazyLock, OnceLock};
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::time::sleep;

use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::observability::Capture;

/// 共享服务端固定端口（见 `docs/develop.md` 端口表）。
pub const SHARED_PORT: u16 = 20104;
/// 服务端就绪等待上限。
const READY_TIMEOUT: Duration = Duration::from_secs(20);
/// 就绪探测间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// 子进程输出文件名。
const LOG_FILE: &str = "server.log";
/// 快照探针路径：只在 debug + `metrics` 构建注册（P2.2-002），`make gen` 不启用该特性，
/// 因此它不在生成的 SDK 里，这里只能写字面量（与 `api/probe/shutdown` 同规格）。
const METRICS_PATH: &str = "api/probe/metrics";
/// 取快照的超时：探针是纯内存读，超时只用于避免服务端卡住时拖住用例。
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// 服务端的 `config.toml` 模板：`[provider]` 指向内置 Mock LLM，`extra` 追加 `[agent]` 等段。
///
/// `default_model` 与服务端原来的兜底值同名（`mock-model`），因此其余用例的可见行为不变。
/// Mock 的场景按最后一条消息里的 `mock:<场景>` 关键字选择（P2-018），因此异常场景不需要改配置。
pub(crate) fn config(base_url: &str, extra: &str) -> String {
    format!(
        r#"[provider]
id = "mock"
kind = "openai-compatible"
base_url = "{base_url}/v1"
api_key = ""
default_model = "mock-model"

[[provider.models]]
id = "mock-model"
context_window = 8192
max_output_tokens = 1024

{extra}"#
    )
}

/// 进程级共享的 Mock LLM：共享服务端与隔离服务端都指向它，用例按关键字选场景。
///
/// 自己启动 Mock 的用例（断言 `/mock/last-request` / `/mock/stats`）不走这里：
/// 共享实例的观测记录会被并发用例互相覆盖。
pub(crate) async fn shared_mock() -> Result<&'static MockServer> {
    static LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));
    static SHARED: OnceLock<MockServer> = OnceLock::new();
    let _guard = LOCK.lock().await;
    if let Some(mock) = SHARED.get() {
        return Ok(mock);
    }
    SHARED.set(MockServer::start().await?).ok();
    SHARED.get().context("共享 Mock LLM 未启动")
}

/// 运行中的服务端子进程。
pub struct Server {
    child: Child,
    base_url: String,
    log_dir: PathBuf,
}

impl Server {
    /// 启动服务端并等待探活通过：配置指向进程级共享 Mock LLM。
    ///
    /// `log_dir` 同时作为子进程的 `AGENT_HOME`，配置、数据与应用日志都落在该目录内。
    pub async fn start(port: u16, log_dir: PathBuf) -> Result<Self> {
        let mock = shared_mock().await?;
        let config = config(mock.base_url(), "");
        Self::start_with(port, log_dir, &config).await
    }

    /// 用给定配置启动服务端：需要 `[agent]` 预算或指向独立 Mock 的用例走这里。
    pub async fn start_with(port: u16, log_dir: PathBuf, config: &str) -> Result<Self> {
        Self::start_with_level(port, log_dir, config, "warn").await
    }

    /// 同 [`Self::start_with`]，但自定义日志级别：需要检索日志内容的用例（`llm.canary`）用 `trace`。
    /// 同 [`Self::start_with`]，但自定义日志级别：需要检索日志内容的用例（`llm.canary`）用 `trace`。
    pub async fn start_with_level(port: u16, log_dir: PathBuf, config: &str, level: &str) -> Result<Self> {
        Self::start_inner(port, log_dir, config, None, level).await
    }

    /// 同时写 `mcp.toml` 启动：MCP 用例要的是「服务端按配置连替身」（P4-018）。
    pub async fn start_with_mcp(port: u16, log_dir: PathBuf, config: &str, mcp: &str) -> Result<Self> {
        Self::start_inner(port, log_dir, config, Some(mcp), "warn").await
    }

    /// 真正的启动流程：配置随应用根目录落地 → 起子进程 → 等探活。
    async fn start_inner(port: u16, log_dir: PathBuf, config: &str, mcp: Option<&str>, level: &str) -> Result<Self> {
        std::fs::create_dir_all(&log_dir).with_context(|| format!("创建测试目录失败：{}", log_dir.display()))?;
        // 配置随应用根目录落地：`[provider]` 段存在时 `session/create` 会校验模型归属（D-P2-015）。
        std::fs::write(log_dir.join(CONFIG_FILE), config)
            .with_context(|| format!("写入测试配置失败：{}", log_dir.display()))?;
        // MCP 配置独立于 `config.toml`：同一应用根目录跑第二次时按这次传的内容覆盖。
        if let Some(mcp) = mcp {
            crate::util::mcp::write_config(&log_dir, mcp)?;
        }
        // 追加写：同一 `AGENT_HOME` 重启时保留上一次运行的日志，便于排查启动失败。
        let output = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join(LOG_FILE))
            .with_context(|| format!("创建服务端日志失败：{}", log_dir.display()))?;
        let errors = output.try_clone().context("复制服务端日志句柄失败")?;
        let binary = server_binary()?;

        let child = Command::new(&binary)
            .arg("server")
            .arg("--addr")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--log-level")
            .arg(level)
            .env(ENV_HOME, &log_dir)
            // 静态资源目录（`lingting-agent-ui/dist`）按服务端进程工作目录解析，固定到仓库根目录，
            // 否则从其他目录跑测试时 WebUI 会 503。
            .current_dir(crate::util::paths::repo_root()?)
            .stdin(Stdio::null())
            .stdout(Stdio::from(output))
            .stderr(Stdio::from(errors))
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("启动服务端失败：{}", binary.display()))?;

        let mut server = Self {
            child,
            base_url: format!("http://127.0.0.1:{port}"),
            log_dir,
        };
        server.wait_ready().await?;
        Ok(server)
    }

    /// 服务端的应用根目录：配置、数据与日志都落在它下面。
    pub fn log_dir(&self) -> &std::path::Path {
        &self.log_dir
    }

    /// 服务基地址。
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 取一份运行时快照：**必须在停止之前**调用，进程一退就再也取不到（P2.2-005）。
    ///
    /// 拿不到时返回「未采集」的原因，由调用方决定是写进报告还是让断言失败。
    pub async fn metrics(&self) -> Capture {
        capture(&self.base_url).await
    }

    /// 停止服务端并回收子进程。
    pub async fn stop(mut self) -> Result<()> {
        let _ = self.child.start_kill();
        self.child.wait().await.context("回收服务端子进程失败")?;
        Ok(())
    }

    /// 同一应用目录与端口上的重启：先停进程，再按同一个 `AGENT_HOME` 启动。
    ///
    /// 持久化用例据此验证「重启后数据仍在、残留 Run 被自愈、占用不恢复」（P2-014）。
    pub async fn restart(self, port: u16) -> Result<Self> {
        let home = self.log_dir.clone();
        self.stop().await?;
        Self::start(port, home).await
    }

    /// 轮询探活接口，直到服务端可访问或超时。
    async fn wait_ready(&mut self) -> Result<()> {
        let client = Client::new(self.base_url.clone()).context("创建探活客户端失败")?;
        let deadline = Instant::now() + READY_TIMEOUT;

        loop {
            if let Some(status) = self.child.try_wait().context("读取服务端状态失败")? {
                bail!(
                    "服务端启动后立即退出（{status}），日志：{}",
                    self.log_dir.join(LOG_FILE).display()
                );
            }
            if client.probe_health().await.is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "服务端在 {} 秒内未就绪，日志：{}",
                    READY_TIMEOUT.as_secs(),
                    self.log_dir.join(LOG_FILE).display()
                );
            }
            sleep(POLL_INTERVAL).await;
        }
    }
}

/// 自管用例的启动辅助：按给定配置在用例目录与端口上起一次服务端，用例结束后无论成败都停止它。
///
/// `chat.*` / `run.*` 需要 `[agent]` 预算或独立 Mock 的用例走这里：配置写进用例自己的
/// `AGENT_HOME`，事件文件与数据库不会影响其他用例。
pub(crate) async fn serve_once<T>(
    context: &TestContext,
    config: &str,
    body: impl Future<Output = Result<T>>,
) -> Result<T> {
    serve_once_inner(context, config, None, body).await
}

/// 同 [`serve_once`]，但先把 `mcp.toml` 写进同一个应用根目录：MCP 用例走这里（P4-018）。
pub(crate) async fn serve_once_mcp<T>(
    context: &TestContext,
    config: &str,
    mcp: &str,
    body: impl Future<Output = Result<T>>,
) -> Result<T> {
    serve_once_inner(context, config, Some(mcp), body).await
}

/// 启动 → 跑用例 → 采集快照 → 停止；`mcp` 为空时不写 `mcp.toml`。
async fn serve_once_inner<T>(
    context: &TestContext,
    config: &str,
    mcp: Option<&str>,
    body: impl Future<Output = Result<T>>,
) -> Result<T> {
    let home = context.home().to_path_buf();
    let server = match mcp {
        Some(mcp) => Server::start_with_mcp(context.port()?, home, config, mcp).await?,
        None => Server::start_with(context.port()?, home, config).await?,
    };
    let result = body.await;
    // 采集必须在停止之前：停止走 `start_kill`，进程一退就再也取不到快照（P2.2-005）。
    context.observe_metrics(server.metrics().await);
    let stopped = server.stop().await;
    match (result, stopped) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), _) => Err(error),
    }
}

/// 取一个当前空闲的端口：先 bind 0 取端口再释放给子进程，仅用于隔离用例。
///
/// 释放到子进程绑定之间存在极小的竞态窗口；共享端口固定为 [`SHARED_PORT`]，冲突时直接报错。
pub fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).context("申请空闲端口失败")?;
    Ok(listener.local_addr().context("读取空闲端口失败")?.port())
}

/// 从服务端取一份运行时快照（P2.2-005）：只在服务端存活时可用。
///
/// 返回「未采集」的原因而不是报错：接口不存在（普通构建 / release）、进程已退出、
/// 响应无法解析都作为「未采集」的原因写进报告；要不要因此让用例失败由用例自己决定。
pub(crate) async fn capture(base_url: &str) -> Capture {
    let url = format!("{}/{METRICS_PATH}", base_url.trim_end_matches('/'));
    let http = match HttpClient::with(|builder| builder.timeout(Some(CAPTURE_TIMEOUT))) {
        Ok(http) => http,
        Err(error) => return Err(format!("创建客户端失败：{error:#}")),
    };
    let response = match http.get(HttpRequest::new(url)).await {
        Ok(response) => response,
        Err(error) => return Err(format!("连接失败：{error:#}")),
    };
    if response.status == 404 {
        return Err("接口不存在（用 `make test` 构建）".to_string());
    }
    if !response.is_success() {
        return Err(format!("HTTP {}（服务端未就绪或正在关闭）", response.status));
    }
    // 响应体是框架的 `R<T>` 包装：载荷在 `data` 里。
    let envelope = match serde_json::from_str::<serde_json::Value>(&response.string()) {
        Ok(envelope) => envelope,
        Err(error) => return Err(format!("响应不是 JSON：{error}")),
    };
    let Some(data) = envelope.get("data").filter(|data| !data.is_null()) else {
        return Err("响应的 data 段为空".to_string());
    };
    serde_json::from_value(data.clone()).map_err(|error| format!("快照字段不匹配：{error}"))
}

/// 服务端二进制：默认与测试入口同目录，可用 `AGENT_TEST_SERVER_BIN` 覆盖。
pub(crate) fn server_binary() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("AGENT_TEST_SERVER_BIN") {
        return Ok(PathBuf::from(path));
    }

    let current = std::env::current_exe().context("获取测试入口路径失败")?;
    let binary = current
        .parent()
        .context("测试入口没有父目录")?
        .join(format!("bin-agent{}", std::env::consts::EXE_SUFFIX));
    if !binary.is_file() {
        bail!(
            "未找到服务端二进制 {}，请先执行 make build 或 make test",
            binary.display()
        );
    }
    Ok(binary)
}
