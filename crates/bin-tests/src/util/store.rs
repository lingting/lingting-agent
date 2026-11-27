//! 用例侧对「用例自己那条服务端数据库」的访问：接口还没暴露的事实从这里核对。
//!
//! 目前只有两类用途：
//!
//! - **只读回读**：检查点序列与 `PENDING` 审计行还没有查询接口（`api/run/history` 随 P6A-013
//!   落地、`PENDING` 按 D-P6-008 不进查询结果），只能从库内行看；
//! - **注入**：例如在 `run_checkpoints` 上临时建一个让插入失败的触发器（`resume.checkpoint`
//!   的写失败路径）。
//!
//! 回读一律走产品实现（`Database::open` + `Repository`），不复刻 SQL 投影与解码；迁移由
//! `Database::open` 完成，与产品启动路径同源。**不读日志与事件文件**。

use std::sync::Arc;
use std::time::Duration;

use agent_core::session::RunCheckpoint;
use agent_tool::ToolCallRecord;
use anyhow::{Context, Result};
use lib_db::{DATABASE_FILE, Database, DbConfig};
use service_core::repository::Repository;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::util::context::TestContext;

/// 打开用例服务端数据库的只读回读入口。
pub(crate) async fn repository(context: &TestContext) -> Result<Repository> {
    let database = Database::open(database_path(context), DbConfig::default())
        .await
        .context("打开用例服务端数据库失败")?;
    Ok(Repository::new(Arc::new(database)))
}

/// 裸连接池：只给注入用（触发器这类建表之外的写入，`Repository` 不承载）。
///
/// 与服务端同一个 WAL 文件：读不阻塞写，写锁只用 `busy_timeout` 等待。
pub(crate) async fn pool(context: &TestContext) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(database_path(context))
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(5));
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .with_context(|| format!("打开用例服务端数据库失败：{}", database_path(context).display()))
}

/// 某 Run 的检查点，按序号升序。
pub(crate) async fn checkpoints(context: &TestContext, run_id: &str) -> Result<Vec<RunCheckpoint>> {
    repository(context)
        .await?
        .checkpoints_of_run(run_id)
        .await
        .with_context(|| format!("读取检查点失败：{run_id}"))
}

/// 某 Run 的审计记录，**含 `PENDING` 行**（查询接口不返回它，见 D-P6-008）。
pub(crate) async fn tool_calls(context: &TestContext, run_id: &str) -> Result<Vec<ToolCallRecord>> {
    repository(context)
        .await?
        .tool_calls(run_id)
        .await
        .with_context(|| format!("读取工具审计失败：{run_id}"))
}

/// 数据库文件位置：`<AGENT_HOME>/data/agent.db`。
fn database_path(context: &TestContext) -> std::path::PathBuf {
    context.home().join("data").join(DATABASE_FILE)
}
