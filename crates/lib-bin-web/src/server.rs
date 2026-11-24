//! Server 启动：绑定监听地址、挂载 Web 路由，并让请求日志带上 trace id。

use crate::{cors, shutdown, webui};
use anyhow::{Context, Result, anyhow};
use framework_web::{WebRoute, use_web};
use framework_web_axum::{WebRouteWrapper, axum_builder};
use manager_agent::AssemblyOptions;
use std::sync::Arc;
use std::time::Instant;
use tracing::{Instrument, info, info_span};

/// 启动 Web Server 并常驻运行，收到关闭信号后完成优雅关闭再返回。
///
/// `options` 是装配参数（事件文件轮转、Provider / Agent 配置等）：由 `bin-agent` 从 `config.toml`
/// 解析后透传，装配时注入一次。
/// 关闭链路与超时策略见 D-028：超时未能排空时以错误返回，由调用方转成非 0 退出码。
/// 内部创建 Tokio 运行时，调用方按同步方式调用即可。
pub fn serve(host: &str, port: u16, options: AssemblyOptions) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("创建 Tokio 运行时失败")?;

    let result = runtime.block_on(run(host, port, options));
    if let Err(error) = &result {
        // 日志已初始化，运行期错误写入日志（控制台 + 文件），调用方只需处理退出码。
        tracing::error!("服务运行失败：{error:#}");
    }
    result
}

async fn run(host: &str, port: u16, options: AssemblyOptions) -> Result<()> {
    // 记录启动时刻，供 health 接口返回启动时间与运行时长。
    lib_web::probe::mark_startup();
    // 装配（P2-013）：应用目录 → 数据库（迁移 + 自愈）→ Repository → Service → 运行时状态，
    // 任一步失败都带着可读错误返回，由调用方转成非 0 退出码。
    let state = manager_agent::assemble(options).await?;
    // 执行端口（P2-022 / D-P2-028 第 3 条）：由启动装配构造并注入。未注入的进程
    // （`api_probe_server`、部分进程内用例）里 Run 会直接落 FAILED「执行器未装配」。
    // 工具注册表与执行参数同样出自装配结果（P3-014）：注册表在装配期建一次，运行期只读。
    let driver = agent_runtime::AgentRuntime::new(
        Arc::clone(state.session()),
        state.provider().clone(),
        state.agent().clone(),
        state.home(),
        Arc::clone(state.tools()),
        state.tool_limits(),
    );
    if !state.runtime().install_driver(Arc::new(driver)) {
        tracing::warn!("执行端口已注入，跳过重复注入");
    }
    if !manager_agent::install(state) {
        tracing::warn!("装配结果已注入，跳过重复注入");
    }
    // 快照聚合的限流来源（P2.2-001）：本 Crate 是唯一同时依赖 `agent-runtime` 与 `manager-agent`
    // 的装配方，因此由它把执行侧的快照接进聚合层，`manager-agent` 不必反向依赖 `agent-runtime`。
    #[cfg(feature = "metrics")]
    if !manager_agent::runtime::metrics::install_limit_provider(limit_snapshot) {
        tracing::warn!("限流快照来源已注册，跳过重复注册");
    }
    // 运行时定时任务（心跳扫描 / 问询投递 / 超时标记 / 事件文件轮转）：运行时唯一，关闭后由任务自己停手。
    framework_timer::start(None, framework_timer::TimerOption::default());
    // 链接锚点：不引用路由收集函数时，inventory 注册项会被链接器丢弃，接口会全部 404。
    let routes = lib_web::web_routes();
    info!(count = routes.len(), "已收集的接口数量");

    // WebUI 静态资源与 SPA 兜底：只接管路由未命中的 GET，api/ 前缀仍归框架的 404。
    let webui = webui::WebUiAssets;
    webui.log_ready();

    // 端口冲突等绑定失败在此返回，由调用方转换为可读错误与退出码（不做随机端口回退）。
    let server = axum_builder(host, i32::from(port))
        .with_cors(cors::resolve)
        .push_interceptor(Arc::new(webui))
        .push_interceptor(Arc::new(shutdown::ShutdownGuard))
        .bind()
        .await?;
    info!("listening on {}", server.context().address());

    // 关闭链路：信号（或 debug 探针）→ 记录关闭请求 → 拒绝窗口 → 框架优雅关闭；
    // 超时则放弃等待进行中的请求。钩子必须在启动前注册。
    shutdown::install(server.shutdown());
    tokio::spawn(shutdown::watch());

    let started = Instant::now();
    let stopping = server.context().subscribe_stopping();
    let result = tokio::select! {
        result = server.run(Some(trace_wrapper())) => result,
        () = shutdown::deadline(stopping) => Err(anyhow!(
            "优雅关闭超时（{}s），已强制退出",
            shutdown::TIMEOUT.as_secs()
        )),
    };
    if result.is_ok() {
        info!(elapsed_ms = started.elapsed().as_millis(), "服务已停止");
    }
    result
}

/// 转写执行侧（`agent-runtime`）的限流快照：字段一一对应，不做换算。
#[cfg(feature = "metrics")]
fn limit_snapshot() -> manager_agent::runtime::metrics::LimitMetrics {
    let limit = agent_runtime::metrics::snapshot();
    manager_agent::runtime::metrics::LimitMetrics {
        queued: limit.queued,
        timeouts: limit.timeouts,
        cancelled: limit.cancelled,
        wait_total_millis: limit.wait_total_millis,
        wait_max_millis: limit.wait_max_millis,
        queued_max: limit.queued_max,
        running_max: limit.running_max,
        running_now: limit.running_now,
    }
}

/// 路由包装器：为每次请求建立带 trace id 的日志作用域，并记录处理结果。
///
/// `x-trace-id` 的透传与回写由框架的分发层完成，这里只负责让日志带上同一个值。
fn trace_wrapper() -> WebRouteWrapper {
    Arc::new(|route: Arc<WebRoute>| {
        let trace_id = use_web()
            .map(|context| context.request().trace_id.clone())
            .unwrap_or_default();
        let span = info_span!(
            "request",
            trace_id = %trace_id,
            method = %route.method,
            path = %route.path
        );

        Box::pin(
            async move {
                info!("收到请求");
                let response = route.invoke().await;
                info!(status = response.status, "请求处理完成");
                Ok(response)
            }
            .instrument(span),
        )
    })
}
