//! 通用核心业务服务：Settings / Provider / Model 与业务入口。
//!
//! P1 不引入 SQLite（D-017）：会话业务的存储是内存 Mock，结构上已经按
//! `service` / `repository` / `holder` 分层，P2 只替换存储实现，不动契约。

pub mod holder;
pub mod session;
