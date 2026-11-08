//! WebUI 领域用例：浏览器访问服务端时可观测的 HTTP 行为。
//!
//! 这里只断言服务端侧（静态托管、SPA 回落、CORS、SSE 帧与错误体）。界面渲染与交互由
//! 前端测试与 Playwright 用例覆盖，见 `.plan/p1-016-webui-tests.md`。

mod assets;

use crate::case::{Case, CaseKind, Prep};
use crate::context::TestContext;
use anyhow::{Context, Result};
use framework_http::{HttpClient, HttpMethod, HttpRequest, HttpResponse};

/// 静态托管类用例的前置要求：服务端读的是 `lingting-agent-ui/dist`。
pub(super) const WEB_UI: &[Prep] = &[Prep::WebUiBuild];

/// WebUI 领域用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![Case {
        name: "web.assets",
        kind: CaseKind::Shared,
        prep: WEB_UI,
        run: assets::assets,
    }]
}

/// 拼出服务的绝对地址：静态资源与预检不属于生成的 SDK 接口，只能按 path 直连。
pub(super) fn url(context: &TestContext, path: &str) -> String {
    format!(
        "{}/{}",
        context.base_url().trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// 发一次原始请求；失败时带上请求地址，便于定位。
pub(super) async fn send(method: HttpMethod, request: HttpRequest) -> Result<HttpResponse> {
    let url = request.url.clone();
    HttpClient::new()
        .request(method, request)
        .await
        .with_context(|| format!("请求失败：{url}"))
}
