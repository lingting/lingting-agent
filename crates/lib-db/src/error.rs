//! 数据库错误：对外只暴露语义（打开 / 迁移 / 查询 / 事务），不泄漏连接串与 SQL。

use std::fmt::{Display, Formatter};
use std::path::PathBuf;

/// 数据库错误。
#[derive(Debug)]
pub enum DbError {
    /// 打开数据库或建连失败。
    Open {
        /// 数据库文件路径。
        path: PathBuf,
        /// 底层错误。
        source: sqlx::Error,
    },
    /// 迁移脚本执行失败；已回滚，`schema_migrations` 不推进。
    Migrate {
        /// 迁移版本号。
        version: i64,
        /// 迁移名称。
        name: &'static str,
        /// 底层错误。
        source: sqlx::Error,
    },
    /// 查询或写入失败。
    Query(sqlx::Error),
    /// 事务开启、提交或回滚失败。
    Transaction(sqlx::Error),
}

impl Display for DbError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open { path, source } => write!(formatter, "打开数据库失败（{}）：{source}", path.display()),
            Self::Migrate { version, name, source } => {
                write!(formatter, "执行迁移失败（{version:04}-{name}）：{source}")
            }
            Self::Query(source) => write!(formatter, "数据库查询失败：{source}"),
            Self::Transaction(source) => write!(formatter, "数据库事务失败：{source}"),
        }
    }
}

impl std::error::Error for DbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open { source, .. } | Self::Query(source) | Self::Transaction(source) => Some(source),
            Self::Migrate { source, .. } => Some(source),
        }
    }
}

impl DbError {
    /// 事务开启 / 提交 / 回滚失败：顺带记一次底层错误，写锁争用计数由观测层判定。
    pub(crate) fn transaction(source: sqlx::Error) -> Self {
        crate::observability::record_error(&source);
        Self::Transaction(source)
    }
}

impl From<sqlx::Error> for DbError {
    fn from(error: sqlx::Error) -> Self {
        crate::observability::record_error(&error);
        Self::Query(error)
    }
}
