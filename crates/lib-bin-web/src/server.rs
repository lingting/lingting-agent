//! Server 启动：绑定监听地址、挂载 Web 路由，并让请求日志带上 trace id。

use anyhow::{Context, Result};
use framework_web::{WebRoute, use_web};
use framework_web_axum::{WebRouteWrapper, axum_builder};
use std::sync::Arc;
use tracing::{Instrument, info, info_span};

/// 启动 Web Server 并常驻运行。
///
/// 内部创建 Tokio 运行时，调用方按同步方式调用即可。
pub fn serve(host: &str, port: u16) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("创建 Tokio 运行时失败")?;

    let result = runtime.block_on(run(host, port));
    if let Err(error) = &result {
        // 日志已初始化，运行期错误写入日志（控制台 + 文件），调用方只需处理退出码。
        tracing::error!("服务运行失败：{error:#}");
    }
    result
}

async fn run(host: &str, port: u16) -> Result<()> {
    // 链接锚点：不引用路由收集函数时，inventory 注册项会被链接器丢弃，接口会全部 404。
    let routes = lib_web::web_routes();
    info!(count = routes.len(), "已收集的接口数量");

    // 端口冲突等绑定失败在此返回，由调用方转换为可读错误与退出码（不做随机端口回退）。
    let server = axum_builder(host, i32::from(port)).bind().await?;
    info!("listening on {}", server.context().address());
    server.run(Some(trace_wrapper())).await
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
