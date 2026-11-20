//! 启动装配：应用目录 → 数据库 → Repository → Service → 运行时状态。
//!
//! 装配顺序固定：先迁库再自愈残留 Run，最后才允许接入请求；任一步失败都返回可读错误，
//! 由启动方（`lib-bin-web`）转成非 0 退出码（P2-013）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use lib_core::time;
use lib_db::{DATABASE_FILE, Database, DbConfig};
use service_core::repository::Repository;
use service_core::session::SessionService;

use crate::holder::AgentHolder;
use crate::runtime::{JournalConfig, RuntimeState};

/// 启动自愈的原因文本：执行中的 Run 落 `FAILED`（无法确认是否已产生结果）。
const HEAL_RUNNING: &str = "服务重启，执行中的 Run 已终止";
/// 启动自愈的原因文本：排队中的 Run 落 `CANCELLED`（占用不恢复，D-P1.1-007）。
const HEAL_QUEUED: &str = "服务重启，排队中的 Run 已取消";

/// 装配参数。
#[derive(Debug, Clone, Default)]
pub struct AssemblyOptions {
    /// 事件文件根目录；`None` 时按应用目录推导（生产路径，用例传临时目录）。
    pub events_root: Option<PathBuf>,
    /// 事件文件轮转配置；由启动方从 `config.toml` 解析后透传。
    pub journal: JournalConfig,
    /// 数据库连接参数。
    pub database: DbConfig,
}

/// 按应用目录装配：`<应用根>/data/agent.db`，事件文件在同级的 `data/events/`（D-P2-004）。
pub async fn assemble(options: AssemblyOptions) -> Result<AgentHolder> {
    let directories = lib_core::directorys::directorys().context("获取应用目录失败")?;
    assemble_with(directories.data.join(DATABASE_FILE), options).await
}

/// 指定数据库文件装配：生产路径由 [`assemble`] 传入应用目录下的 `agent.db`，
/// 用例传入自己的临时目录（`AGENT_HOME` 之外的隔离，不发散到真实应用目录）。
pub async fn assemble_with(database_path: impl AsRef<Path>, options: AssemblyOptions) -> Result<AgentHolder> {
    let database_path = database_path.as_ref();
    let database = Arc::new(
        Database::open(database_path, options.database)
            .await
            .with_context(|| format!("打开数据库失败：{}", database_path.display()))?,
    );
    tracing::info!(path = %database_path.display(), "数据库已就绪");

    let repository = Arc::new(Repository::new(Arc::clone(&database)));
    let healed = repository
        .with_orphan_healing(HEAL_RUNNING, HEAL_QUEUED, time::now_millis())
        .await
        .context("启动自愈失败")?;
    if healed > 0 {
        tracing::warn!(healed, "启动自愈：残留 Run 已落成终态");
    }

    let session = Arc::new(SessionService::new(repository));
    let runtime = RuntimeState::new(Arc::clone(&session)).with_journal_config(options.journal);
    let runtime = match options.events_root {
        Some(root) => runtime.with_events_root(root),
        None => runtime,
    };
    Ok(AgentHolder::new(session, runtime))
}
