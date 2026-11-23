//! 用例共用的小工具：临时目录与轮询等待。
//!
//! 只放与业务无关的基础动作，避免各领域用例重复实现同样的等待与清理逻辑。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result};
use lib_core::time;
use tokio::time::sleep;

/// 单元用例临时目录的根：与端到端用例的运行目录同级，便于统一清理。
const UNIT_ROOT: &str = "lingting-agent-tests/units";
/// 轮询等待的默认超时：写者任务与断连处理链都在后台跑，只等待它们跑完。
const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
/// 轮询间隔。
const WAIT_INTERVAL: Duration = Duration::from_millis(10);

/// 为用例创建独立临时目录（每次调用都不同，避免用例之间互相污染）。
pub fn temp_directory(name: &str) -> Result<PathBuf> {
    let directory = std::env::temp_dir()
        .join(UNIT_ROOT)
        .join(format!("{name}-{}", time::now_millis()));
    std::fs::create_dir_all(&directory).with_context(|| format!("创建临时目录失败：{}", directory.display()))?;
    Ok(directory)
}

/// 本测试进程的运行时快照：`metrics` 特性下 `manager-agent` 的进程内累加值，直接读内存。
///
/// 只覆盖本进程装配的 holder；服务端子进程的数字由 [`crate::util::server::capture`] 取。
pub fn local_metrics() -> manager_agent::runtime::metrics::MetricsSnapshot {
    manager_agent::runtime::metrics::snapshot()
}

/// 轮询等待条件成立；超时返回 `false`，由调用方给出可读断言。
pub async fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        if condition() {
            return true;
        }
        sleep(WAIT_INTERVAL).await;
    }
    condition()
}

/// 共享装配使用的事件文件根目录；装配时写入，用例读它断言事件文件。
static EVENTS_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// 进程内用例的共享装配：整个测试进程只装配一次，之后复用同一份 holder。
///
/// 装配走临时目录里的数据库（`agent.db`）与事件目录，不碰真实应用目录；用例只共享
/// 「会话业务服务 + 运行时状态」，会话 ID 由服务生成，因此不同用例之间不会撞数据。
pub(crate) async fn installed() -> Result<&'static manager_agent::AgentHolder> {
    // 装配串行化：多个用例并发进入时只装一次，失败信息保留给后续调用。
    // 不用 `OnceCell::get_or_try_init`：它把闭包返回的 future 类型带进外层 future，
    // 会让用例的 `Box<dyn Future + Send>` 触发 `Send is not general enough` 的编译错误。
    static LOCK: std::sync::LazyLock<tokio::sync::Mutex<Option<String>>> =
        std::sync::LazyLock::new(|| tokio::sync::Mutex::new(None));
    let mut failure = LOCK.lock().await;
    if manager_agent::holder().is_none() && failure.is_none() {
        match assemble().await {
            Ok(()) => {}
            Err(error) => *failure = Some(format!("{error:#}")),
        }
    }
    if let Some(message) = failure.as_ref() {
        anyhow::bail!("{message}");
    }
    drop(failure);
    manager_agent::holder().context("装配未注入")
}

/// 真正执行装配与注入；只由 [`installed`] 在持锁状态下调用一次。
async fn assemble() -> Result<()> {
    let root = temp_directory("assembly")?;
    let events = root.join("events");
    let state = manager_agent::assemble_with(
        root.join("agent.db"),
        manager_agent::AssemblyOptions {
            events_root: Some(events.clone()),
            ..Default::default()
        },
    )
    .await?;
    if !manager_agent::install(state) {
        anyhow::bail!("装配结果已被其他用例注入");
    }
    EVENTS_ROOT.set(events).ok();
    Ok(())
}

/// 共享装配的事件文件根目录（进程内用例断言事件文件时用）。
pub(crate) async fn assembly_events_root() -> Result<&'static Path> {
    installed().await?;
    EVENTS_ROOT
        .get()
        .map(PathBuf::as_path)
        .context("事件文件根目录未初始化")
}

/// 进程内用例共用的会话业务服务（需要建会话、发消息时用它）。
pub(crate) async fn session_service() -> Result<std::sync::Arc<service_core::session::SessionService>> {
    Ok(std::sync::Arc::clone(installed().await?.session()))
}

/// 不注入 holder 的独立运行时状态：事件文件写进用例自己的临时目录，会话服务仍用共享装配。
///
/// 只操作注册表 / 事件文件的用例用它，进程级 holder 仍然只有一个（装配只发生一次）。
pub(crate) async fn runtime_state(root: &Path) -> Result<manager_agent::runtime::RuntimeState> {
    let service = session_service().await?;
    Ok(manager_agent::runtime::RuntimeState::new(service).with_events_root(root))
}
