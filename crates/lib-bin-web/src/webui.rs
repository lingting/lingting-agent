//! WebUI 静态资源托管：提供前端构建产物，未命中的前端路由回落到 `index.html`。
//!
//! 路径隔离规则：所有 `api/` 前缀的请求不进入静态查找，未命中时仍由框架返回 API 404；
//! 其余 GET 请求按“带扩展名视为资源、否则视为前端路由”处理。
//! 资源来源见 `assets`：release 用内嵌字节，debug 读本地 `dist`（D-009）。

mod assets;

use anyhow::Result;
use async_trait::async_trait;
use framework_web::{WebContext, WebInterceptResult, WebInterceptor, WebMethod, WebResponse, WebRoute};
use std::path::Path;
use std::sync::Arc;

/// SPA 入口文件。
const INDEX: &str = "index.html";

/// API 路径前缀，命中后保持 API 的 404 语义。
const API_PREFIX: &str = "api";

/// 静态资源拦截器：仅在路由未命中的 GET 请求上生效。
pub struct WebUiAssets;

impl WebUiAssets {
    /// 启动时记录资源状态，避免资源缺失只能在请求期发现。
    pub fn log_ready(&self) {
        assets::log_ready();
    }

    /// 生成静态响应：入口文件、静态文件与 SPA 兜底都在这里决定。
    fn respond(&self, path: &str) -> WebResponse {
        let relative = path.trim_start_matches('/');
        if relative.is_empty() {
            return self.index();
        }
        // 带扩展名视为资源请求：缺失时返回 404，避免把 index.html 当成脚本或样式返回。
        if Path::new(relative).extension().is_some() {
            return self.file(relative).unwrap_or_else(|| text(404, "静态资源不存在"));
        }
        self.index()
    }

    /// 入口文件缺失时返回可读的 503，而不是伪装成 API 404。
    fn index(&self) -> WebResponse {
        self.file(INDEX)
            .unwrap_or_else(|| text(503, "WebUI 静态资源未就绪：缺少 index.html"))
    }

    fn file(&self, relative: &str) -> Option<WebResponse> {
        let asset = assets::read(relative)?;
        let mut response = WebResponse::empty();
        response.status = 200;
        response.headers.set_content_type(content_type(relative));
        response.headers.set_content_length(asset.body.len());
        response.headers.set("cache-control", cache_control(relative));
        if asset.gzip {
            response.headers.set("content-encoding", "gzip");
        }
        response.body = asset.body.into();

        Some(response)
    }
}

#[async_trait]
impl WebInterceptor for WebUiAssets {
    fn order(&self) -> i32 {
        // 只处理路由未命中的 GET，不与其他拦截器竞争，使用默认位置。
        0
    }

    fn should_intercept(&self, context: Arc<WebContext>, route: Option<Arc<WebRoute>>) -> bool {
        let request = context.request();
        route.is_none() && request.method == WebMethod::Get && !is_api_path(&request.path)
    }

    async fn intercept(&self, context: Arc<WebContext>, _route: Option<Arc<WebRoute>>) -> Result<WebInterceptResult> {
        Ok(WebInterceptResult::Respond(self.respond(&context.request().path)))
    }
}

/// 是否为 API 路径：`api` 与 `api/...` 都算，大小写敏感（与路由匹配一致）。
fn is_api_path(path: &str) -> bool {
    path.trim_start_matches('/')
        .strip_prefix(API_PREFIX)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn text(status: u16, message: &str) -> WebResponse {
    let body = message.as_bytes().to_vec();
    let mut response = WebResponse::empty();
    response.status = status;
    response.headers.set_content_type("text/plain; charset=utf-8");
    response.headers.set_content_length(body.len());
    response.headers.set("cache-control", "no-store");
    response.body = body.into();

    response
}

/// 按扩展名推断 `Content-Type`，未识别的类型按二进制流返回。
fn content_type(relative: &str) -> &'static str {
    let extension = Path::new(relative)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);

    match extension.as_deref() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") | Some("map") => "application/json; charset=utf-8",
        Some("txt") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml; charset=utf-8",
        Some("webmanifest") => "application/manifest+json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// 入口文件每次校验，其余产物带内容哈希可长期缓存。
fn cache_control(relative: &str) -> &'static str {
    let is_index = Path::new(relative).file_name().is_some_and(|name| name == INDEX);

    if is_index {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    }
}
