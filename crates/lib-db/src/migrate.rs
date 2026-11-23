//! 迁移 runner：迁移账本 + 内嵌脚本，按版本升序执行。
//!
//! 不用 `sqlx::migrate!`（D-P2-003 不启用 `macros`）：账本表 `schema_migrations` 是唯一事实源，
//! 不使用 `PRAGMA user_version`（D-P2-030）。每个脚本走 [`Database::with_transaction`]：
//! 成功才写账本，失败整条回滚，因此连续启动是可重复执行的。
//!
//! 语句逐条执行，不用 `sqlx::raw_sql`：后者返回的 future 在 `&mut SqliteConnection` 上
//! 无法满足 `Send`（`Executor` 实现要求引用与 `Executor` 生命周期相同，高阶推导失败），
//! 会让整个迁移 future 不实现 `Send`，进而无法在异步服务里使用（D-P2-033）。

use sqlx::SqlitePool;

use crate::{Database, DbError};

/// 一个迁移脚本：版本号、名称与编进二进制的 SQL。
struct Migration {
    /// 版本号，按升序执行。
    version: i64,
    /// 名称，只用于错误与日志。
    name: &'static str,
    /// 脚本内容。
    sql: &'static str,
}

/// 迁移清单：追加新版本时只在这里追加一条，不改动已发布的脚本。
const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "init",
    sql: include_str!("../migrations/0001_init.sql"),
}];

/// 迁移账本表：建表语句本身也必须是幂等的（每次启动都会执行）。
const CREATE_LEDGER: &str = "\
CREATE TABLE IF NOT EXISTS schema_migrations (
    version       INTEGER PRIMARY KEY,
    applied_at_ms INTEGER NOT NULL
)";

/// 执行全部未应用的迁移。
pub(crate) async fn run(database: &Database) -> Result<(), DbError> {
    sqlx::query(CREATE_LEDGER).execute(database.pool()).await?;
    for migration in MIGRATIONS {
        if applied(database.pool(), migration.version).await? {
            continue;
        }
        apply(database, migration).await?;
    }
    Ok(())
}

/// 该版本是否已应用。
async fn applied(pool: &SqlitePool, version: i64) -> Result<bool, DbError> {
    let found: Option<i64> = sqlx::query_scalar("SELECT version FROM schema_migrations WHERE version = ?")
        .bind(version)
        .fetch_optional(pool)
        .await?;
    Ok(found.is_some())
}

/// 在事务里执行一个脚本并记账；任一步失败整条回滚。
async fn apply(database: &Database, migration: &Migration) -> Result<(), DbError> {
    let mut transaction = database.begin().await?;
    for statement in statements(migration.sql) {
        sqlx::query(statement)
            .execute(transaction.connection())
            .await
            .map_err(|error| migrated(migration, error))?;
    }
    sqlx::query("INSERT INTO schema_migrations (version, applied_at_ms) VALUES (?, ?)")
        .bind(migration.version)
        .bind(lib_core::time::now_millis())
        .execute(transaction.connection())
        .await
        .map_err(|error| migrated(migration, error))?;
    transaction.commit().await
}

/// 把底层错误收敛成「哪个迁移失败」。
fn migrated(migration: &Migration, source: sqlx::Error) -> DbError {
    crate::observability::record_error(&source);
    DbError::Migrate {
        version: migration.version,
        name: migration.name,
        source,
    }
}

/// 把脚本拆成单条语句。
///
/// 约定（与 `migrations/*.sql` 一致）：用行尾 `;` 分隔语句，注释只用 `--`，
/// 不在语句里出现分号；纯注释片段与空片段一律跳过。
fn statements(sql: &str) -> impl Iterator<Item = &str> {
    sql.split(';').map(str::trim).filter(|statement| has_sql(statement))
}

/// 片段里是否有真正的 SQL（排除空片段与纯 `--` 注释）。
fn has_sql(statement: &str) -> bool {
    statement
        .lines()
        .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with("--"))
}
