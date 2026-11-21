//! SQLite 基础设施：连接池、迁移、事务句柄与统一错误。
//!
//! 本 Crate 只提供数据库能力，**不含业务 SQL**：表结构的创建与版本推进在 `migrate` 里，
//! 业务查询一律写在各自 Service Crate 的 `repository` 内（`docs/workspace.md` 4.3）。
//!
//! 依赖方向：`lib-db -> lib-core`，不依赖 `types-core` / `service-core` / `agent-*`。
//!
//! 迁移不用 `sqlx::migrate!`：该宏要求 `macros` 特性（D-P2-003 不启用），
//! 改为自建 runner（D-P2-030），脚本用 `include_str!` 编进二进制，不依赖运行时文件树。
//!
//! 事务边界只由 [`Database::begin`] 产生，提交 / 回滚只由 [`Transaction`] 产生；
//! 业务侧一律通过 `with_xxx` 方法使用事务（D-P2-034）。

mod config;
mod error;
mod migrate;
pub mod observability;
mod pool;
mod transaction;

pub use config::DbConfig;
pub use error::DbError;
pub use pool::Database;
pub use transaction::Transaction;

/// 数据库文件名，位于应用数据目录下（D-P2-004）。
pub const DATABASE_FILE: &str = "agent.db";
