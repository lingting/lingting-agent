//! `web.fallback`：前端路由回落 index.html，且不破坏 API 的 404 语义。

use crate::case::CaseFuture;
use crate::context::TestContext;
use anyhow::ensure;
use framework_http::{HttpMethod, HttpRequest};

/// 断言 SPA 回落与 API 404 的边界（D-027）。
pub(super) fn fallback(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let route = super::send(HttpMethod::Get, HttpRequest::new(super::url(context, "/chat/abc"))).await?;
        ensure!(route.status == 200, "前端路由应回落首页，实际 {}", route.status);
        ensure!(
            route.mime_type().as_deref() == Some("text/html"),
            "前端路由 content-type 应为 html，实际 {:?}",
            route.content_type()
        );
        ensure!(route.string().contains("id=\"root\""), "回落内容不是首页");

        // 不存在的 API 路径必须保持 API 404 结构，不能被回落吃成首页。
        let missing = super::send(HttpMethod::Get, HttpRequest::new(super::url(context, "/api/not-exist"))).await?;
        ensure!(missing.status == 404, "未知 API 应返回 404，实际 {}", missing.status);
        ensure!(
            missing.mime_type().as_deref() == Some("application/json"),
            "未知 API 的错误体应为 JSON，实际 {:?}",
            missing.content_type()
        );
        let error = super::api_error(&missing)?;
        ensure!(error.code == 404, "未知 API 错误码应为 404，实际 {}", error.code);
        ensure!(
            error.message == "请求接口不存在",
            "未知 API 错误消息不符：{}",
            error.message
        );

        // api/ 前缀永不进入静态查找：带扩展名也一样。
        let api_asset = super::send(
            HttpMethod::Get,
            HttpRequest::new(super::url(context, "/api/unknown.js")),
        )
        .await?;
        ensure!(
            api_asset.status == 404,
            "api/ 下的资源路径应返回 404，实际 {}",
            api_asset.status
        );
        ensure!(
            api_asset.mime_type().as_deref() == Some("application/json"),
            "api/ 下的资源路径应返回 API 错误体，实际 {:?}",
            api_asset.content_type()
        );

        // 非 GET 不进静态托管。
        let post = super::send(HttpMethod::Post, HttpRequest::new(super::url(context, "/chat/abc"))).await?;
        ensure!(post.status == 404, "非 GET 的前端路由应返回 404，实际 {}", post.status);
        ensure!(
            post.mime_type().as_deref() == Some("application/json"),
            "非 GET 的前端路由应返回 API 错误体，实际 {:?}",
            post.content_type()
        );

        Ok(vec![
            format!("GET /chat/abc 回落首页（{} 字节）", route.body.len()),
            format!("GET /api/not-exist → 404 {}：{}", error.code, error.message),
            "GET /api/unknown.js 与 POST /chat/abc 均为 API 404".to_string(),
        ])
    })
}
