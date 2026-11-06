//! API 探针服务（P1-004 验证 / P1-005 与 P1-006 的 SDK 联调）。
//!
//! 启动后按 `lib_web::probe` 的路径提供服务：
//!
//! ```text
//! cargo run -p lib-web --example api_probe_server            # 默认 127.0.0.1:8799
//! AGENT_PROBE_PORT=9000 cargo run -p lib-web --example api_probe_server
//! ```

use framework_web_axum::axum_builder;

/// 默认监听端口。
const DEFAULT_PORT: i32 = 8799;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 链接锚点：不引用 lib-web 的符号时，注册项会被链接器丢弃，路由全部 404。
    let routes = lib_web::web_routes();
    let port = std::env::var("AGENT_PROBE_PORT")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(DEFAULT_PORT);
    let server = axum_builder("127.0.0.1", port).bind().await?;
    println!(
        "probe server listening on http://{} ({} routes)",
        server.context().address(),
        routes.len()
    );
    server.run(None).await
}
