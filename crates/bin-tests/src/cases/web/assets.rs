//! `web.assets`：静态资源托管（首页、构建产物与缺失资源）。

use crate::case::CaseFuture;
use crate::context::TestContext;
use anyhow::{Result, bail, ensure};
use framework_http::{HttpMethod, HttpRequest};

/// 断言首页、构建产物与缺失资源三种响应。
pub(super) fn assets(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let index = super::send(HttpMethod::Get, HttpRequest::new(super::url(context, "/"))).await?;
        ensure!(index.status == 200, "首页状态码应为 200，实际 {}", index.status);
        ensure!(
            index.mime_type().as_deref() == Some("text/html"),
            "首页 content-type 应为 html，实际 {:?}",
            index.content_type()
        );
        let html = index.string();
        ensure!(html.contains("id=\"root\""), "首页缺少挂载节点 #root");

        let asset = asset_path(&html)?;
        let response = super::send(HttpMethod::Get, HttpRequest::new(super::url(context, &asset))).await?;
        ensure!(
            response.status == 200,
            "构建产物 {asset} 状态码应为 200，实际 {}",
            response.status
        );
        ensure!(
            response.mime_type().as_deref() == Some("text/javascript"),
            "构建产物 {asset} content-type 应为 javascript，实际 {:?}",
            response.content_type()
        );
        ensure!(!response.is_empty(), "构建产物 {asset} 响应体为空");
        ensure!(
            response
                .header("cache-control")
                .is_some_and(|value| value.contains("immutable")),
            "构建产物 {asset} 应带长期缓存头，实际 {:?}",
            response.header("cache-control")
        );

        let missing = super::send(
            HttpMethod::Get,
            HttpRequest::new(super::url(context, "/assets/missing.js")),
        )
        .await?;
        ensure!(missing.status == 404, "缺失资源状态码应为 404，实际 {}", missing.status);
        // 缺失的资源按 D-027 返回 404 文本，不能回落成 index.html。
        ensure!(
            missing.mime_type().as_deref() == Some("text/plain"),
            "缺失资源应返回文本错误，实际 {:?}",
            missing.content_type()
        );

        Ok(vec![
            format!("首页 {} 字节，content-type {:?}", html.len(), index.content_type()),
            format!("构建产物 {asset} {} 字节", response.body.len()),
            format!("缺失资源返回 {}", missing.status),
        ])
    })
}

/// 从首页 HTML 里取一个脚本产物路径：Vite 会注入 `src="/assets/index-<hash>.js"`。
fn asset_path(html: &str) -> Result<String> {
    const PREFIX: &str = "/assets/";

    let mut rest = html;
    while let Some(index) = rest.find(PREFIX) {
        let candidate = &rest[index..];
        let end = candidate.find('"').unwrap_or(candidate.len());
        let path = &candidate[..end];
        if path.ends_with(".js") {
            return Ok(path.to_string());
        }
        rest = &candidate[end..];
    }
    bail!("首页没有引用 /assets/ 下的脚本产物（先执行 make ui-build）")
}
