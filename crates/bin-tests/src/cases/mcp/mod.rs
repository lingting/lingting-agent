//! MCP 用例（P4-018 起）：配置装载、工具发现、调用、生命周期与接口。
//!
//! 断言口径与 P3 一致：数据只来自接口响应、客户端事件与 `metrics` 快照，**不读日志**。
//! 当前批次只有进程内可核对的部分（`mcp.config`），连接与调用用例随 P4-017 的测试替身接入。

mod config;

use crate::case::{Case, CaseFn, CaseKind};

/// MCP 用例；排序由上层统一处理。
pub(crate) fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    cases.extend(config::cases());
    cases
}

/// 声明一个用例：MCP 用例都不需要额外的前置产物。
pub(super) fn case(name: &'static str, kind: CaseKind, run: CaseFn) -> Case {
    Case {
        name,
        kind,
        prep: &[],
        run,
    }
}
