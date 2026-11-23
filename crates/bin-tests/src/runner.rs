//! 用例调度：共享资源、并发执行、超时、结果汇总与报告落盘。

use crate::case::{Case, CaseKind};
use crate::report::{self, CaseReport, RunReport};
use crate::util::context::TestContext;
use crate::util::observability::{Capture, SOURCE_OWN, SOURCE_SHARED};
use crate::util::prepare;
use crate::util::server::{self, Server};
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

/// 单个用例的执行结果；错误在报告阶段转成文本。
struct CaseRun {
    base_url: String,
    notes: Vec<String>,
    measures: Vec<(String, String)>,
    snapshot: Capture,
    snapshot_source: &'static str,
    error: Option<anyhow::Error>,
}

impl CaseRun {
    /// 在跑到采集阶段之前就失败：快照显式记「未采集」，报告据此写原因。
    fn failed(base_url: impl Into<String>, snapshot_source: &'static str, reason: &str, error: anyhow::Error) -> Self {
        Self {
            base_url: base_url.into(),
            notes: Vec::new(),
            measures: Vec::new(),
            snapshot: Err(reason.to_string()),
            snapshot_source,
            error: Some(error),
        }
    }

    /// 由执行结果、用例度量与已采集的快照合成结果：失败时保留已采集到的数据。
    fn settle(
        base_url: String,
        snapshot_source: &'static str,
        context: &TestContext,
        snapshot: Capture,
        result: Result<Vec<String>>,
    ) -> Self {
        match result {
            Ok(notes) => Self {
                base_url,
                notes,
                measures: context.measures(),
                snapshot,
                snapshot_source,
                error: None,
            },
            Err(error) => Self {
                base_url,
                notes: Vec::new(),
                measures: context.measures(),
                snapshot,
                snapshot_source,
                error: Some(error),
            },
        }
    }
}

/// 执行全部选中用例；共享服务端只在存在 `Shared` 用例时启动一次。
pub async fn execute(config: Config) -> Result<Summary> {
    let started_at_ms = time::now_millis();
    let run_id = format!("run-{started_at_ms}-{}", std::process::id());
    let run_dir = create_run_dir(&run_id)?;
    println!("测试运行目录：{}", run_dir.display());

    // 准备必须早于服务端启动：静态托管在启动时记录产物状态，用例执行期间不再重建。
    prepare::ensure(&config.cases).await?;
    let shared = start_shared(&config.cases, &run_dir).await?;
    let shared_base_url = shared.as_ref().map(|server| server.base_url().to_string());
    // 共享服务端只提供接入信息：每个用例建自己的上下文，度量条目与快照不跨用例混。
    let shared_endpoint = shared.as_ref().map(|server| {
        Arc::new(SharedEndpoint {
            base_url: server.base_url().to_string(),
            home: server.log_dir().to_path_buf(),
        })
    });

    let total = config.cases.len();
    let concurrency = config.concurrency;
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut tasks = JoinSet::new();

    for case in config.cases {
        let permit = semaphore.clone().acquire_owned().await.context("获取并发许可失败")?;
        let shared_endpoint = shared_endpoint.clone();
        let run_dir = run_dir.clone();
        tasks.spawn(async move {
            let _permit = permit;
            run_case(case, shared_endpoint, run_dir).await
        });
    }

    let mut failed = 0;
    let mut reports: Vec<CaseReport> = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        let case = joined.context("用例任务异常退出")?;
        print_case(&case);
        if !case.is_success() {
            failed += 1;
        }
        reports.push(case);
    }

    if let Some(server) = shared
        && let Err(error) = server.stop().await
    {
        eprintln!("停止共享服务端失败：{error:#}");
    }

    reports.sort_by_key(|case| case.name);
    print_summary(total, failed, &run_dir);
    report::write(&RunReport {
        run_id: &run_id,
        started_at_ms,
        concurrency,
        shared_base_url: shared_base_url.as_deref(),
        log_root: &run_dir,
        cases: &reports,
    })?;

    Ok(Summary { failed })
}

/// 共享服务端的接入信息：每个用例据此建自己的上下文（度量与快照按用例隔离）。
struct SharedEndpoint {
    base_url: String,
    home: PathBuf,
}

/// 执行单个用例：共享用例复用共享服务端、隔离用例自起服务端、自管用例自己启停。
async fn run_case(case: Case, shared: Option<Arc<SharedEndpoint>>, run_dir: PathBuf) -> CaseReport {
    let started = Instant::now();
    let run = match case.kind {
        CaseKind::Shared => run_shared(case, shared).await,
        CaseKind::Isolated => run_isolated(case, &run_dir).await,
        CaseKind::SelfServed => run_self_served(case, &run_dir).await,
    };

    CaseReport {
        name: case.name,
        kind: case.kind,
        base_url: run.base_url,
        log_dir: case_dir(&run_dir, case),
        elapsed: started.elapsed(),
        measures: run.measures,
        snapshot: run.snapshot,
        snapshot_source: run.snapshot_source,
        notes: run.notes,
        error: run.error.map(|error| format!("{error:#}")),
    }
}

/// 共享用例：只建上下文，服务端由入口统一启动与停止。
async fn run_shared(case: Case, shared: Option<Arc<SharedEndpoint>>) -> CaseRun {
    let Some(endpoint) = shared else {
        return CaseRun::failed("-", SOURCE_SHARED, "共享服务端未启动", anyhow!("共享服务端未启动"));
    };
    let base_url = endpoint.base_url.clone();
    let context = match TestContext::new(case.name, base_url.clone(), endpoint.home.clone()) {
        Ok(context) => context,
        Err(error) => return CaseRun::failed(base_url, SOURCE_SHARED, "上下文创建失败", error),
    };

    let result = run_with_timeout(&case, &context).await;
    let snapshot = snapshot_of(&base_url, &context).await;
    CaseRun::settle(base_url, SOURCE_SHARED, &context, snapshot, result)
}

/// 隔离用例：启动独立服务端，执行完后无论成败都停止。
async fn run_isolated(case: Case, run_dir: &Path) -> CaseRun {
    let log_dir = run_dir.join(case.name);
    let port = match server::free_port() {
        Ok(port) => port,
        Err(error) => return CaseRun::failed("-", SOURCE_OWN, "服务端未启动", error),
    };
    let base_url = format!("http://127.0.0.1:{port}");

    let server = match Server::start(port, log_dir.clone()).await {
        Ok(server) => server,
        Err(error) => {
            return CaseRun::failed(
                base_url,
                SOURCE_OWN,
                "服务端未就绪",
                error.context(format!("启动隔离服务端失败（用例 {}）", case.name)),
            );
        }
    };
    let context = match TestContext::new(case.name, base_url.clone(), log_dir.clone()) {
        Ok(context) => context,
        Err(error) => {
            let _ = server.stop().await;
            return CaseRun::failed(base_url, SOURCE_OWN, "上下文创建失败", error);
        }
    };

    let result = run_with_timeout(&case, &context).await;
    // 采集必须在停止之前：停止走 `start_kill`，进程一退就再也取不到快照（P2.2-005）。
    let snapshot = server.metrics().await;
    let stopped = server.stop().await;
    CaseRun::settle(base_url, SOURCE_OWN, &context, snapshot, join(result, stopped))
}

/// 自管用例：只准备上下文（基地址与 `AGENT_HOME`），服务端由用例自己启停。
///
/// 用于「同一数据目录重启」这类需要把握启动时机的用例（P2-014）：入口若先启动一次，
/// 用例就没法在首次启动前后断言迁移与自愈的行为。
async fn run_self_served(case: Case, run_dir: &Path) -> CaseRun {
    let log_dir = run_dir.join(case.name);
    if let Err(error) = std::fs::create_dir_all(&log_dir) {
        return CaseRun::failed(
            "-",
            SOURCE_OWN,
            "用例目录创建失败",
            anyhow!("创建用例目录失败（{}）：{error}", log_dir.display()),
        );
    }
    let port = match server::free_port() {
        Ok(port) => port,
        Err(error) => return CaseRun::failed("-", SOURCE_OWN, "服务端未启动", error),
    };
    let base_url = format!("http://127.0.0.1:{port}");
    let context = match TestContext::new(case.name, base_url.clone(), log_dir) {
        Ok(context) => context,
        Err(error) => return CaseRun::failed(base_url, SOURCE_OWN, "上下文创建失败", error),
    };

    let result = run_with_timeout(&case, &context).await;
    let snapshot = snapshot_of(&base_url, &context).await;
    CaseRun::settle(base_url, SOURCE_OWN, &context, snapshot, result)
}

/// 服务端快照：先直接取；服务端已在用例内停掉时，退回用例在停止前记录的那一份。
async fn snapshot_of(base_url: &str, context: &TestContext) -> Capture {
    match server::capture(base_url).await {
        Ok(metrics) => Ok(metrics),
        Err(reason) => context.observed_metrics().unwrap_or(Err(reason)),
    }
}

/// 合并「用例结果」与「停止服务端结果」：任一方失败都算用例失败。
fn join(result: Result<Vec<String>>, stopped: Result<()>) -> Result<Vec<String>> {
    match (result, stopped) {
        (Ok(notes), Ok(())) => Ok(notes),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), _) => Err(error),
    }
}

/// 带超时与 panic 兜底执行用例；panic 不让整个测试进程退出。
async fn run_with_timeout(case: &Case, context: &TestContext) -> Result<Vec<String>> {
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

/// 创建本次运行的日志目录：系统临时目录下按运行 ID 隔离。
fn create_run_dir(run_id: &str) -> Result<PathBuf> {
    let root = std::env::temp_dir().join(RUN_ROOT).join(run_id);
    std::fs::create_dir_all(&root).with_context(|| format!("创建测试运行目录失败：{}", root.display()))?;
    Ok(root)
}

/// 用例对应的文件目录：共享用例落在 `shard-shared`，隔离用例落在用例名下。
fn case_dir(run_dir: &Path, case: Case) -> PathBuf {
    match case.kind {
        CaseKind::Shared => run_dir.join(SHARED_DIR),
        CaseKind::Isolated | CaseKind::SelfServed => run_dir.join(case.name),
    }
}

/// 输出单个用例结果。
fn print_case(case: &CaseReport) {
    let elapsed = case.elapsed.as_millis();
    match &case.error {
        None => println!("[ok]   {}（{}，{elapsed}ms）", case.name, case.kind.label()),
        Some(error) => {
            println!("[FAIL] {}（{}，{elapsed}ms）", case.name, case.kind.label());
            for line in error.lines() {
                println!("       {line}");
            }
            println!("       日志：{}", case.log_dir.display());
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
