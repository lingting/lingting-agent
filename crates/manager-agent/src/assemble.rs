//! 启动装配：应用目录 → 数据库 → Repository → Service → 运行时状态。
//!
//! 装配顺序固定：先迁库再自愈残留 Run，最后才允许接入请求；任一步失败都返回可读错误，
//! 由启动方（`lib-bin-web`）转成非 0 退出码（P2-013）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_route::ProviderService;
use agent_tool::builtin::{BuiltinOptions, register_all};
use agent_tool::{ToolLimits, ToolPermission, ToolPolicy, ToolRegistry};
use anyhow::{Context, Result};
use lib_core::time;
use lib_db::{DATABASE_FILE, Database, DbConfig};
use service_core::repository::Repository;
use service_core::session::SessionService;
use types_core::config::{AgentConfig, ProviderConfig, ToolConfig, ToolPermissionKind};

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
    /// Provider 配置；`None` 表示 `config.toml` 没有 `[provider]` 段（服务可启动，调用模型才失败）。
    pub provider: Option<ProviderConfig>,
    /// Agent 运行参数；由启动方从 `config.toml` 解析后透传。
    pub agent: AgentConfig,
    /// 工具执行参数与权限（`[tools]`）；由启动方从 `config.toml` 解析后透传（P3-005）。
    pub tools: ToolConfig,
    /// 数据库连接参数。
    pub database: DbConfig,
    /// 应用根目录；`None` 时按应用目录推导（用例传自己的临时目录）。
    pub home: Option<PathBuf>,
}

/// 按应用目录装配：`<应用根>/data/agent.db`，事件文件在同级的 `data/events/`（D-P2-004）。
pub async fn assemble(options: AssemblyOptions) -> Result<AgentHolder> {
    let directories = lib_core::directorys::directorys().context("获取应用目录失败")?;
    let options = AssemblyOptions {
        home: Some(directory_root(&directories)),
        ..options
    };
    assemble_with(directories.data.join(DATABASE_FILE), options).await
}

/// 应用根目录：`config.toml` 所在目录（`AGENTS.md` 与配置都在这一层）。
fn directory_root(directories: &lib_core::directorys::AppDirectories) -> PathBuf {
    directories
        .config_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| directories.data.clone())
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

    let provider = ProviderService::new(options.provider);
    if !provider.configured() {
        tracing::info!("未配置 Provider（config.toml 缺 [provider] 段）：服务可启动，调用模型时才会失败");
    }
    let session = Arc::new(SessionService::new(repository));
    let runtime = RuntimeState::new(Arc::clone(&session)).with_journal_config(options.journal);
    let runtime = match options.events_root {
        Some(root) => runtime.with_events_root(root),
        None => runtime,
    };
    let home = match options.home {
        Some(home) => home,
        None => {
            let directories = lib_core::directorys::directorys().context("获取应用目录失败")?;
            directory_root(&directories)
        }
    };
    let (tools, tool_limits) = build_tools(&options.tools)?;
    Ok(AgentHolder::new(
        session,
        runtime,
        provider,
        options.agent,
        home,
        tools,
        tool_limits,
    ))
}

/// 按 `[tools]` 建注册表与执行参数：权限判定表、并发/超时上限，然后注册五个内置工具。
///
/// 权限只影响「发给模型的清单」：`deny` 的工具仍然注册（执行期回 `DENIED`），`http_request`
/// 是 `Network` 风险、默认 `deny`，不显式 `allow` 就不进清单（D-P3-005）。
fn build_tools(config: &ToolConfig) -> Result<(Arc<ToolRegistry>, ToolLimits)> {
    let policy = ToolPolicy::new(
        config
            .permissions
            .iter()
            .map(|(name, kind)| (name.clone(), permission_of(*kind))),
    );
    let mut registry = ToolRegistry::new(policy);
    let builtin = BuiltinOptions {
        file_allow_dirs: config.file.allow_dirs.clone(),
        file_max_bytes: config.file.max_bytes,
        http_allow_hosts: config.http.allow_hosts.clone(),
        http_max_bytes: config.http.max_bytes,
    };
    register_all(&mut registry, &builtin).context("注册内置工具失败")?;
    let limits = ToolLimits {
        timeout_ms: config.timeout_ms,
        max_result_chars: config.max_result_chars,
        max_parallel_calls: config.max_parallel_calls,
        max_retries: config.max_retries,
    };
    Ok((Arc::new(registry), limits))
}

/// 配置侧的权限取值 → 工具层的判定值：两处枚举刻意分开，配置类型不进 `agent-tool`。
fn permission_of(kind: ToolPermissionKind) -> ToolPermission {
    match kind {
        ToolPermissionKind::Allow => ToolPermission::Allow,
        ToolPermissionKind::Deny => ToolPermission::Deny,
        ToolPermissionKind::Confirm => ToolPermission::Confirm,
    }
}
