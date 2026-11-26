//! Skill 用例（P5）：配置装载、`SKILL.md` 解析与发现。
//!
//! 断言口径：数据只来自进程内的公开契约（`lib_bin_cli::config::parse_skills`、`agent_skill::parse`）
//! 与服务端子进程的退出码；**不读日志**。解析与发现的用例在 P5-016 落地，本批次只有 `skill.config`。

mod config;

use crate::case::{Case, CaseFn, CaseKind};

/// Skill 用例；排序由上层统一处理。
pub(crate) fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    cases.extend(config::cases());
    cases
}

/// 声明一个用例：Skill 用例都不需要额外的前置产物。
pub(super) fn case(name: &'static str, kind: CaseKind, run: CaseFn) -> Case {
    Case {
        name,
        kind,
        prep: &[],
        run,
    }
}
