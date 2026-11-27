//! `skill.web`（P5-015 的取证）：抽屉清单、详情、开关与重载四条路径的前端用例。
//!
//! 与 `tool.web` 同形：只跑技能抽屉的用例文件，其余前端用例由 `web.ui` 统一跑。

use super::case;
use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;
use crate::util::frontend;

/// 抽屉用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.web", CaseKind::Shared, web)]
}

/// 在 `lingting-agent-ui` 里跑 `tests/skill-drawer.test.tsx`。
fn web(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        frontend::skill_tests().await?;
        Ok(vec!["前端技能抽屉用例通过（skill-drawer.test.tsx）".to_string()])
    })
}
