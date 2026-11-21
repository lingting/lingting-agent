//! 事务句柄：由 [`Database::begin`] 开启，`commit` 显式提交，未提交即回滚。
//!
//! 事务的开启、提交与回滚只由本模块负责，业务侧只允许通过 `with_xxx` 方法取得事务；
//! 语句执行期间的任何 `?` 提前返回都会让句柄被丢弃，`sqlx::Transaction` 的 `Drop`
//! 会发起回滚，因此不会留下半个事务（D-P2-034）。

use sqlx::SqliteConnection;

use crate::error::DbError;
use crate::pool::Database;

/// 事务句柄：包装 `sqlx` 的事务，保证「提交或回滚」只有两个出口。
pub struct Transaction {
    /// `None` 表示已经提交或回滚；`Drop` 时仍是 `Some` 就会由 `sqlx` 回滚。
    inner: Option<sqlx::Transaction<'static, sqlx::Sqlite>>,
}

impl Database {
    /// 开启事务：**事务边界只在这里产生**，业务代码不得直接调用 `sqlx` 的 `begin`。
    pub async fn begin(&self) -> Result<Transaction, DbError> {
        // 取连接等待是「数据库是不是瓶颈」的唯一直接证据（P2.1-003）：
        // 计时把手在未启用 `metrics` 特性时是零成本占位。
        let timer = crate::observability::AcquireTimer::start();
        let transaction = self.pool().begin().await.map_err(DbError::Transaction)?;
        timer.finish();
        Ok(Transaction {
            inner: Some(transaction),
        })
    }
}

impl Transaction {
    /// 事务内的连接：只用于本事务范围内的语句。
    pub fn connection(&mut self) -> &mut SqliteConnection {
        self.inner
            .as_mut()
            .expect("事务已结束：commit / rollback 之后不能再执行语句")
    }

    /// 提交事务。
    pub async fn commit(mut self) -> Result<(), DbError> {
        let transaction = self.inner.take().expect("事务已结束：commit / rollback 只能调用一次");
        transaction.commit().await.map_err(DbError::Transaction)
    }

    /// 显式回滚；调用方明确知道要放弃本次写入时使用（其余情况直接 `?` 返回即可）。
    pub async fn rollback(mut self) -> Result<(), DbError> {
        let transaction = self.inner.take().expect("事务已结束：commit / rollback 只能调用一次");
        transaction.rollback().await.map_err(DbError::Transaction)
    }
}
