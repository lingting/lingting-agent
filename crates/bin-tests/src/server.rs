//! 服务端子进程：端口、就绪探测、输出重定向与停止。
//!
//! 测试进程只作为客户端，服务端一律以 `bin-agent server` 子进程运行：关闭标记、日志初始化
//! 与信号处理都是进程级单例，同进程起多个实例会互相干扰。

use anyhow::{Context, Result, bail};
use lib_bin_cli::client::Client;
use lib_bin_cli::sdk::ApiClient;
use lib_core::directorys::ENV_HOME;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::time::sleep;

/// 共享服务端固定端口（见 `docs/develop.md` 端口表）。
pub const SHARED_PORT: u16 = 20104;
/// 服务端就绪等待上限。
const READY_TIMEOUT: Duration = Duration::from_secs(20);
/// 就绪探测间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// 子进程输出文件名。
const LOG_FILE: &str = "server.log";

/// 运行中的服务端子进程。
pub struct Server {
    child: Child,
    base_url: String,
    log_dir: PathBuf,
}

impl Server {
    /// 启动服务端并等待探活通过。
    ///
    /// `log_dir` 同时作为子进程的 `AGENT_HOME`，配置、数据与应用日志都落在该目录内。
    pub async fn start(port: u16, log_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&log_dir).with_context(|| format!("创建测试目录失败：{}", log_dir.display()))?;
        let output = std::fs::File::create(log_dir.join(LOG_FILE))
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
            .arg("warn")
            .env(ENV_HOME, &log_dir)
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

    /// 服务基地址。
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 停止服务端并回收子进程。
    pub async fn stop(mut self) -> Result<()> {
        let _ = self.child.start_kill();
        self.child.wait().await.context("回收服务端子进程失败")?;
        Ok(())
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

/// 取一个当前空闲的端口：先 bind 0 取端口再释放给子进程，仅用于隔离用例。
///
/// 释放到子进程绑定之间存在极小的竞态窗口；共享端口固定为 [`SHARED_PORT`]，冲突时直接报错。
pub fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).context("申请空闲端口失败")?;
    Ok(listener.local_addr().context("读取空闲端口失败")?.port())
}

/// 服务端二进制：默认与测试入口同目录，可用 `AGENT_TEST_SERVER_BIN` 覆盖。
fn server_binary() -> Result<PathBuf> {
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
