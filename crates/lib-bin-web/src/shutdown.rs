//! 优雅关闭：关闭信号、拒绝窗口、兜底超时与关闭期间的请求拒绝（D-028）。
//!
//! 关闭链路固定为：信号（或 debug 探针）→ 记录关闭请求（`lib_core::lifecycle`）→ 拒绝窗口 →
//! 框架的优雅关闭入口。记录关闭请求在前，因为 SSE 生产方与请求守门都读同一个标记；
//! 拒绝窗口让关闭期间的新请求拿到可读的 503，而不是连接被直接拒绝；框架的停止流程最后执行，
//! 负责停止接受新连接并等待进行中的请求结束。

use anyhow::Result;
use async_trait::async_trait;
use framework_web::{WebContext, WebInterceptResult, WebInterceptor, WebResponse, WebRoute};
use framework_web_axum::{AxumShutdown, AxumStatus};
use lib_core::lifecycle;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::info;

/// 优雅关闭的兜底超时：超时后不再等待进行中的请求，进程以退出码 1 结束。
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// 关闭期间的拒绝窗口：先让新请求拿到 503，再停止监听，避免表现为连接被直接拒绝。
const REJECT_WINDOW: Duration = Duration::from_millis(200);

/// 关闭期间拒绝请求时返回的提示。
const REJECT_MESSAGE: &str = "服务正在关闭，请稍后重试";

/// 关闭期间的请求守门：已进入关闭流程后，所有请求直接返回 503。
pub struct ShutdownGuard;

#[async_trait]
impl WebInterceptor for ShutdownGuard {
    fn order(&self) -> i32 {
        // 最先执行：关闭流程中不再进入静态托管与业务处理。
        -1
    }

    fn should_intercept(&self, _context: Arc<WebContext>, _route: Option<Arc<WebRoute>>) -> bool {
        lifecycle::is_shutdown()
    }

    async fn intercept(&self, _context: Arc<WebContext>, _route: Option<Arc<WebRoute>>) -> Result<WebInterceptResult> {
        Ok(WebInterceptResult::Respond(reject()))
    }
}

/// 503 响应：与 API 错误响应同形状（`{code, message}`，D-024），客户端可直接展示。
fn reject() -> WebResponse {
    let body = format!(r#"{{"code":503,"message":"{REJECT_MESSAGE}"}}"#);
    let mut response = WebResponse::empty();
    response.status = 503;
    response.headers.set_content_type("application/json; charset=utf-8");
    response.headers.set_content_length(body.len());
    response.body = body.into_bytes().into();
    response
}

/// 监听关闭信号：Ctrl+C（全平台）与 SIGTERM（Unix）。
pub async fn signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal as unix_signal};
        match unix_signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => "ctrl_c",
                    _ = terminate.recv() => "sigterm",
                }
            }
            Err(error) => {
                tracing::warn!("监听 SIGTERM 失败，只响应 Ctrl+C：{error}");
                let _ = tokio::signal::ctrl_c().await;
                "ctrl_c"
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "ctrl_c"
    }
}

/// 安装关闭钩子：关闭请求首次生效后先进入拒绝窗口，再交给框架停止服务。
///
/// 钩子要在服务启动前注册，否则关闭请求只会置位标记而不会停止服务。
pub fn install(shutdown: AxumShutdown) {
    lifecycle::set_shutdown_hook(move || {
        // 钩子由触发方同步调用，等待动作放到独立任务里，避免阻塞触发方（信号任务或请求任务）。
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            info!(
                reject_window_ms = REJECT_WINDOW.as_millis(),
                "进入关闭拒绝窗口，新请求返回 503"
            );
            sleep(REJECT_WINDOW).await;
            shutdown.shutdown();
        });
    });
}

/// 关闭信号的消费者：收到信号即记录关闭请求，后续动作由 [`install`] 注册的钩子完成。
pub async fn watch() {
    let signal = signal().await;
    info!(signal, "收到关闭信号，开始优雅关闭");
    lifecycle::request_shutdown();
}

/// 兜底超时：框架进入停止流程后，再等 [`TIMEOUT`] 仍未结束即视为超时。
pub async fn deadline(stopping: impl Future<Output = AxumStatus>) {
    stopping.await;
    sleep(TIMEOUT).await;
}
