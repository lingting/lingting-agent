//! 浏览器跨域策略：只放行本地回环来源，不使用通配来源。

use framework_web::WebContext;
use framework_web_axum::AxumCors;

/// 允许的浏览器来源：服务端与开发环境 WebUI 的回环地址（端口见 D-022）。
const ALLOWED_ORIGINS: [&str; 4] = [
    "http://127.0.0.1:20100",
    "http://localhost:20100",
    "http://127.0.0.1:20101",
    "http://localhost:20101",
];

/// 允许的请求方法：覆盖当前 API 使用的方法与预检自身。
const ALLOW_METHODS: &str = "GET, POST, PUT, PATCH, DELETE, OPTIONS";

/// 允许的请求头：JSON 请求体与链路标识。
const ALLOW_HEADERS: &str = "content-type, x-trace-id";

/// 预检结果缓存秒数。
const MAX_AGE: &str = "600";

/// 按请求来源解析 CORS 配置。
///
/// 来源不在白名单时不输出任何 CORS 响应头，由浏览器阻断跨域读取；
/// 预检请求（OPTIONS）由框架统一返回 204，这里只负责补全响应头。
pub fn resolve(context: &WebContext) -> AxumCors {
    let origin = context
        .request()
        .headers
        .get_first("origin")
        .map(|value| value.trim())
        .filter(|value| ALLOWED_ORIGINS.contains(value));

    match origin {
        Some(origin) => AxumCors {
            origin: Some(origin.to_owned()),
            methods: Some(ALLOW_METHODS.to_owned()),
            headers: Some(ALLOW_HEADERS.to_owned()),
            expose: Some("x-trace-id".to_owned()),
            credentials: None,
            max_age: Some(MAX_AGE.to_owned()),
        },
        None => AxumCors::fixed().clone(),
    }
}
