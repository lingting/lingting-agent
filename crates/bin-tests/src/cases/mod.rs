//! 用例注册：新增领域时在这里汇总。

mod chat;
mod cli;
mod client;
mod disconnect;
mod journal;
mod llm;
mod occupancy;
mod persist;
mod probe;
mod rotation;
mod run;
mod runtime;
mod sdk;
mod session;
mod shutdown;
mod stress;
mod tui;
mod web;

use crate::case::Case;

/// 全部用例，按名称排序。
pub fn all() -> Vec<Case> {
    let mut cases = Vec::new();
    cases.extend(chat::cases());
    cases.extend(cli::cases());
    cases.extend(client::cases());
    cases.extend(disconnect::cases());
    cases.extend(journal::cases());
    cases.extend(llm::cases());
    cases.extend(occupancy::cases());
    cases.extend(persist::cases());
    cases.extend(probe::cases());
    cases.extend(rotation::cases());
    cases.extend(run::cases());
    cases.extend(shutdown::cases());
    cases.extend(stress::cases());
    cases.extend(runtime::cases());
    cases.extend(sdk::cases());
    cases.extend(session::cases());
    cases.extend(tui::cases());
    cases.extend(web::cases());
    cases.sort_by_key(|case| case.name);
    cases
}
