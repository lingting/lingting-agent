//! `web.e2e`：真实浏览器端到端用例（Playwright）。

use crate::case::CaseFuture;
use crate::context::TestContext;
use crate::frontend;

/// 用本用例的隔离服务端跑 Playwright：服务端地址经 `WEBUI_BASE_URL` 传给前端配置。
///
/// 关服用例会把服务端停掉，因此这里必须是隔离实例，不能复用共享服务端。
pub(super) fn e2e(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        frontend::e2e(context.base_url()).await?;
        Ok(vec!["端到端用例通过（playwright test）".to_string()])
    })
}
