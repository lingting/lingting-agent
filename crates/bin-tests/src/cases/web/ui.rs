//! `web.ui`：前端用例（`pnpm test` → `vp test run`）全部通过。

use crate::case::CaseFuture;
use crate::context::TestContext;
use crate::frontend;

/// 在 `lingting-agent-ui` 里跑前端用例；失败时把 pnpm 的输出原样留在控制台。
pub(super) fn ui(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        frontend::test().await?;
        Ok(vec!["前端用例通过（pnpm test → vp test run）".to_string()])
    })
}
