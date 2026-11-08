//! 用例调度：共享资源、并发执行、超时与失败汇总。

use crate::case::{Case, CaseKind};
use crate::context::TestContext;
use crate::server::{self, Server};
use anyhow::{Context, Result, anyhow};
use futures_util::FutureExt;
use lib_core::time;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;

/// 单个用例的超时：Mock 事件流数秒内结束，超时只用于防止挂死。
const CASE_TIMEOUT: Duration = Duration::from_secs(60);
/// 运行目录所在根目录名。
const RUN_ROOT: &str = "lingting-agent-tests";
/// 共享服务端的目录名。
const SHARED_DIR: &str = "shard-shared";

/// 一次运行的配置。
pub struct Config {
    /// 已选中的用例。
    pub cases: Vec<Case>,
    /// 并发上限。
    pub concurrency: usize,
}

/// 运行结果汇总。
pub struct Summary {
    /// 失败用例数。
    failed: usize,
}

impl Summary {
    /// 全部用例是否通过。
    pub fn is_success(&self) -> bool {
        self.failed == 0
    }
}

/// 单个用例的执行结果。
struct CaseResult {
    name: &'static str,
    kind: CaseKind,
    log_dir: PathBuf,
    elapsed: Duration,
    error: Option<anyhow::Error>,
}

/// 执行全部选中用例；共享服务端只在存在 `Shared` 用例时启动一次。
pub async fn execute(config: Config) -> Result<Summary> {
    let run_dir = create_run_dir()?;
    println!("测试运行目录：{}", run_dir.display());

    let shared = start_shared(&config.cases, &run_dir).await?;
    let shared_context = match &shared {
        Some(server) => Some(Arc::new(TestContext::new("shared", server.base_url().to_string())?)),
        None => None,
    };

    let total = config.cases.len();
    let semaphore = Arc::new(Semaphore::new(config.concurrency));
    let mut tasks = JoinSet::new();

    for case in config.cases {
        let permit = semaphore.clone().acquire_owned().await.context("获取并发许可失败")?;
        let shared_context = shared_context.clone();
        let run_dir = run_dir.clone();
        tasks.spawn(async move {
            let _permit = permit;
            run_case(case, shared_context, run_dir).await
        });
    }

    let mut failed = 0;
    while let Some(joined) = tasks.join_next().await {
        let result = joined.context("用例任务异常退出")?;
        report(&result);
        if result.error.is_some() {
            failed += 1;
        }
    }

    if let Some(server) = shared
        && let Err(error) = server.stop().await
    {
        eprintln!("停止共享服务端失败：{error:#}");
    }

    print_summary(total, failed, &run_dir);
    Ok(Summary { failed })
}

/// 执行单个用例：共享用例直接复用上下文，隔离用例自起服务端。
async fn run_case(case: Case, shared: Option<Arc<TestContext>>, run_dir: PathBuf) -> CaseResult {
    let started = Instant::now();
    let outcome = match case.kind {
        CaseKind::Shared => match shared {
            Some(context) => run_with_timeout(&case, &context).await,
            None => Err(anyhow!("共享服务端未启动")),
        },
        CaseKind::Isolated => run_isolated(case, &run_dir).await,
    };

    CaseResult {
        name: case.name,
        kind: case.kind,
        log_dir: case_dir(&run_dir, case),
        elapsed: started.elapsed(),
        error: outcome.err(),
    }
}

/// 隔离用例：启动独立服务端，执行完后无论成败都停止。
async fn run_isolated(case: Case, run_dir: &Path) -> Result<()> {
    let log_dir = run_dir.join(case.name);
    let port = server::free_port()?;
    let server = Server::start(port, log_dir.clone())
        .await
        .with_context(|| format!("启动隔离服务端失败（用例 {}）", case.name))?;
    let context = TestContext::new(case.name, server.base_url().to_string())?;

    let outcome = run_with_timeout(&case, &context).await;
    let stopped = server.stop().await;
    match outcome {
        Ok(()) => stopped,
        Err(error) => Err(error),
    }
}

/// 带超时与 panic 兜底执行用例；panic 不让整个测试进程退出。
async fn run_with_timeout(case: &Case, context: &TestContext) -> Result<()> {
    let run = AssertUnwindSafe((case.run)(context)).catch_unwind();
    match timeout(CASE_TIMEOUT, run).await {
        Err(_) => Err(anyhow!("用例超时（{} 秒）", CASE_TIMEOUT.as_secs())),
        Ok(Err(payload)) => Err(anyhow!("用例 panic：{}", panic_message(payload))),
        Ok(Ok(result)) => result,
    }
}

/// 启动共享服务端；没有 `Shared` 用例时不启动。
async fn start_shared(cases: &[Case], run_dir: &Path) -> Result<Option<Server>> {
    if !cases.iter().any(|case| case.kind == CaseKind::Shared) {
        return Ok(None);
    }

    let log_dir = run_dir.join(SHARED_DIR);
    let server = Server::start(server::SHARED_PORT, log_dir).await.with_context(|| {
        format!(
            "启动共享服务端失败（端口 {}，被占用时请先结束遗留进程）",
            server::SHARED_PORT
        )
    })?;
    println!("共享服务端：{}", server.base_url());
    Ok(Some(server))
}

/// 创建本次运行的目录：系统临时目录下按运行 ID 隔离。
fn create_run_dir() -> Result<PathBuf> {
    let run_id = format!("run-{}-{}", time::now_millis(), std::process::id());
    let root = std::env::temp_dir().join(RUN_ROOT).join(run_id);
    std::fs::create_dir_all(&root).with_context(|| format!("创建测试运行目录失败：{}", root.display()))?;
    Ok(root)
}

/// 用例对应的文件目录：共享用例落在 `shard-shared`，隔离用例落在用例名下。
fn case_dir(run_dir: &Path, case: Case) -> PathBuf {
    match case.kind {
        CaseKind::Shared => run_dir.join(SHARED_DIR),
        CaseKind::Isolated => run_dir.join(case.name),
    }
}

/// 输出单个用例结果。
fn report(result: &CaseResult) {
    let elapsed = result.elapsed.as_millis();
    match &result.error {
        None => println!("[ok]   {}（{}，{elapsed}ms）", result.name, result.kind.label()),
        Some(error) => {
            println!("[FAIL] {}（{}，{elapsed}ms）", result.name, result.kind.label());
            for line in format!("{error:#}").lines() {
                println!("       {line}");
            }
            println!("       日志：{}", result.log_dir.display());
        }
    }
}

/// 输出汇总与运行目录。
fn print_summary(total: usize, failed: usize, run_dir: &Path) {
    println!();
    if failed == 0 {
        println!("全部通过：{total} 个用例");
    } else {
        println!("{total} 个用例：{} 通过，{failed} 失败", total - failed);
    }
    println!("运行目录：{}", run_dir.display());
}

/// 提取 panic 载荷里的可读消息。
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "未知 panic 载荷".to_string()
}
