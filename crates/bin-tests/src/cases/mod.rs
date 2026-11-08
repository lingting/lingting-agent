//! 用例注册：新增领域时在这里汇总。

mod probe;
mod session;
mod tui;
mod web;

use crate::case::Case;

/// 全部用例，按名称排序。
pub fn all() -> Vec<Case> {
    let mut cases = Vec::new();
    cases.extend(probe::cases());
    cases.extend(session::cases());
    cases.extend(tui::cases());
    cases.extend(web::cases());
    cases.sort_by_key(|case| case.name);
    cases
}
