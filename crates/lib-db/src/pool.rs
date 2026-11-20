//! 连接池：打开数据库、执行迁移与事务入口。

use std::path::Path;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

use crate::config::DbConfig;
use crate::error::DbError;
use crate::migrate;

/// 已打开的数据库：连接池 + 迁移完成后的可用状态。
pub struct Database {
    /// 连接池；只在本 Crate 与事务句柄之间传递。
    pool: SqlitePool,
}

impl Database {
    /// 打开数据库并完成迁移。
    ///
    /// `journal_mode=WAL`、`busy_timeout`、`foreign_keys=ON` 由连接选项施加到每条连接（D-P2-004）；
    /// 文件不存在时创建。目录必须已存在：目录归属应用装配方（`lib_core::directorys`）。
    pub async fn open(path: impl AsRef<Path>, config: DbConfig) -> Result<Self, DbError> {
        let path = path.as_ref();
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_millis(config.busy_timeout_ms))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(config.max_connections.max(1))
            .acquire_timeout(config.acquire_timeout())
            .connect_with(options)
            .await
            .map_err(|error| DbError::Open {
                path: path.to_path_buf(),
                source: error,
            })?;

        let database = Self { pool };
        database.migrate().await?;
        Ok(database)
    }

    /// 执行内嵌迁移：已应用的版本跳过，未应用的在独立事务里执行并记账（D-P2-030）。
    pub async fn migrate(&self) -> Result<(), DbError> {
        migrate::run(self).await
    }

    /// 连接池句柄：只用于单语句查询与 [`Self::begin`]。
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// 关闭连接池，等待在途语句结束。
    pub async fn close(self) {
        self.pool.close().await;
    }
}
