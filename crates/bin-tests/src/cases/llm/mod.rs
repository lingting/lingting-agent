//! LLM 领域用例：配置装载与密钥安全（P2-017），以及内置 Mock LLM Server 的自身可用性（P2-018）。
//!
//! [`mock`] 同时是对外复用的测试设施：P2-026 / P2-031 的 `chat.*` / `run.*` 用例通过它提供
//! 服务端要访问的 Mock LLM——服务端 `config.toml` 的 `[provider] base_url` 指向
//! [`mock::MockServer::base_url`] 即可。

mod canary;
mod config;
pub(crate) mod mock;
mod security;

use crate::case::{Case, CaseKind};

/// LLM 领域用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "llm.canary",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: canary::canary,
        },
        Case {
            name: "llm.config",
            kind: CaseKind::Shared,
            prep: &[],
            run: config::config,
        },
        Case {
            name: "llm.mock",
            kind: CaseKind::Shared,
            prep: &[],
            run: mock::case::mock,
        },
        Case {
            name: "llm.security",
            kind: CaseKind::Shared,
            prep: &[],
            run: security::security,
        },
    ]
}
