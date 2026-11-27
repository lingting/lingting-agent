//! 数据访问层：会话 / 消息 / Run 的全部 SQL 与行映射。
//!
//! 只有本模块出现 SQL（`docs/workspace.md` 4.3、P2 验收 S-8）：Service 只调用方法，不拼 SQL。
//! 约定：
//!
//! - **事务边界只由 `lib-db` 产生**：需要多语句原子性的操作一律命名为 `with_xxx`，
//!   内部经 [`Database::begin`] 取得事务并显式 `commit`；`*_in` 私有 helper 只接收已开启的
//!   事务，自己绝不开事务（D-P2-034）。任何 `?` 提前返回都会让事务句柄被丢弃并回滚。
//! - 终态写入一律是**条件更新**（`WHERE status IN (...)`），受影响 0 行表示「已终态」，
//!   由调用方决定语义，不在这里报错（D-P2-030）。
//! - 枚举列存 serde wire 值，读回时用 `strum` 的 `FromStr`（`auto_enum` 已派生）；
//!   不使用 `sqlx` 的 `derive` / `FromRow`（D-P2-003）。
//! - 消息顺序由 `seq` 保证，分配放在 `INSERT` 的子查询里，与写入同一条语句完成（D-P2-005）。

mod checkpoint;
mod message;
mod run;
mod session;
mod tool;

pub use checkpoint::CheckpointError;

use std::sync::Arc;

use lib_db::{Database, DbError, Transaction};
use sqlx::SqlitePool;

/// 数据访问层入口；一个进程一个实例，内部共享同一个数据库。
pub struct Repository {
    /// 数据库：事务入口与连接池都从这里取。
    database: Arc<Database>,
}

impl Repository {
    /// 用已打开的数据库构造 Repository（迁移由 `Database::open` 完成）。
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    /// 连接池句柄：只用于单语句查询。
    fn pool(&self) -> &SqlitePool {
        self.database.pool()
    }

    /// 开启事务；只允许本模块的 `with_xxx` 方法调用。
    async fn begin(&self) -> Result<Transaction, DbError> {
        self.database.begin().await
    }
}

/// 把字符串列解析成枚举；失败按列解码错误返回，不 panic。
fn decode_enum<T>(column: &str, value: &str) -> Result<T, DbError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value.parse().map_err(|error| {
        DbError::Query(sqlx::Error::ColumnDecode {
            index: column.to_string(),
            source: format!("列 `{column}` 的取值无法识别：{error}").into(),
        })
    })
}
