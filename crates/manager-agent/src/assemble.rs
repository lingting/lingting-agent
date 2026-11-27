//! 启动装配：应用目录 → 数据库 → Repository → Service → 运行时状态。
//!
//! 装配顺序固定：先迁库再自愈残留 Run，最后才允许接入请求；任一步失败都返回可读错误，
//! 由启动方（`lib-bin-web`）转成非 0 退出码（P2-013）。
//!
//! 工具与 Skill 的顺序同样固定（P3-014 / P4-011 / P5-010）：内置工具注册表 → Skill 发现与
//! `skill_load` 注册 → MCP 接入 → Context 组装器（`AGENTS.md` + Skill 索引）交给运行时。Skill
//! 必须先于 MCP：`skill_load` 先入注册表，MCP 的工具随后进来，「缺工具」标记因此按运行期现状
//! 判定（D-P5-009）。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agent_context::ContextBuilder;
use agent_mcp::{FileAccessRules, McpManager, McpServerSpec, McpTransport};
use agent_route::ProviderService;
use agent_tool::builtin::{BuiltinOptions, register_all};
use agent_tool::{ToolConcurrency, ToolFileAccess, ToolLimits, ToolPermission, ToolPolicy, ToolRegistry};
use anyhow::{Context, Result};
use lib_core::time;
use lib_db::{DATABASE_FILE, Database, DbConfig};
use service_core::repository::Repository;
use service_core::session::SessionService;
use types_core::config::{
    AgentConfig, McpConcurrencyKind, McpConfig, McpFileAccessKind, McpPermissionKind, McpServerConfig,
    McpTransportConfig, ProviderConfig, SkillsConfig, ToolConfig, ToolPermissionKind,
};

use crate::holder::AgentHolder;
use crate::runtime::{JournalConfig, RuntimeState};
use crate::skill::build_skills;

/// 启动自愈的中断原因：执行中的 Run 落 `PAUSED`（可恢复中间态，D-P6-004）。
///
/// 文案是 `runs.interrupted_reason` 的唯一来源，恢复入口据此给出「上次执行被中断：…」
/// 的简短原因（D-P6-007 的白名单只认这一句）。旧的终态文案「服务重启，执行中的 Run 已终止」
/// 留给「判定不可恢复」的终态路径使用，当前没有调用点。
const INTERRUPTED_RUNNING: &str = "服务重启，执行中的 Run 已中断";
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
    /// MCP 配置（`mcp.toml`）；由启动方从独立配置文件装载后透传（P4-011）。
    pub mcp: McpConfig,
    /// Skill 配置（`skills.toml`）；由启动方从独立配置文件装载后透传（P5-010）。
    pub skills: SkillsConfig,
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
        .with_orphan_healing(INTERRUPTED_RUNNING, HEAL_QUEUED, time::now_millis())
        .await
        .context("启动自愈失败")?;
    if healed > 0 {
        tracing::warn!(healed, "启动自愈：残留 Run 已处理（执行中的转为可恢复中断态）");
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
    // 配置冲突可见但不阻断启动（D-P5-010）：执行器的 `max_result_chars` 是最后一道闸，
    // 正常配置下 `max_inline_chars < max_result_chars`，不会触发工具结果截断。
    if options.skills.enabled && options.skills.max_inline_chars > tool_limits.max_result_chars {
        tracing::warn!(
            max_inline_chars = options.skills.max_inline_chars,
            max_result_chars = tool_limits.max_result_chars,
            "skills.toml 的 max_inline_chars 大于 [tools] max_result_chars：skill_load 的结果会被执行器截断"
        );
    }
    // Skill 在 MCP 之前接入：`skill_load` 先入注册表，MCP 的工具随后进来，缺工具标记因此按运行期
    // 现状判定（D-P5-009 / P5-010 的装配顺序）。
    let skills = build_skills(&options.skills, &home, &tools).context("装配 Skill 失败")?;
    // MCP 在工具注册表建好之后接入：工具经同一个注册表暴露给模型（P4-011）。
    let mcp = build_mcp(&options.mcp, &home, Arc::clone(&tools)).await;
    // Context 组装器由装配层造好给运行时（D-P5-016）：索引与 `AGENTS.md` 在这里合成同一份来源。
    let builder = match skills.source() {
        Some(source) => ContextBuilder::new(home.clone()).with_skills(Arc::clone(source)),
        None => ContextBuilder::new(home.clone()),
    };
    Ok(AgentHolder::new(
        session,
        runtime,
        provider,
        options.agent,
        home,
        tools,
        tool_limits,
        mcp,
        skills,
        builder,
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

/// 建 MCP 门面并接入注册表：配置翻译、并发连接、逐 server 一行装配日志。
///
/// 连接失败只记状态，不返回错误：所有 server 都不可达时服务照常启动（P4-011）。
/// 日志里不出现命令行与请求头（判据 E-3）。
async fn build_mcp(config: &McpConfig, home: &Path, registry: Arc<ToolRegistry>) -> Arc<McpManager> {
    let manager = Arc::new(McpManager::new(mcp_config(config, home), registry));
    manager.connect_enabled_all().await;
    for server in manager.snapshot() {
        if server.status == agent_mcp::McpStatusKind::Failed {
            tracing::warn!(
                id = %server.id,
                transport = %server.transport,
                status = ?server.status,
                tools = server.tool_count,
                "MCP 服务器不可用"
            );
        } else {
            tracing::info!(
                id = %server.id,
                transport = %server.transport,
                status = ?server.status,
                tools = server.tool_count,
                "MCP 服务器状态"
            );
        }
    }
    manager
}

/// `mcp.toml` 的顶层配置 → `agent-mcp` 的连接配置（两套类型只在这一层相遇，D-P4-001）。
fn mcp_config(config: &McpConfig, home: &Path) -> agent_mcp::McpConfig {
    agent_mcp::McpConfig {
        reconnect_initial_backoff: Duration::from_millis(config.reconnect_initial_backoff_ms),
        reconnect_max_backoff: Duration::from_millis(config.reconnect_max_backoff_ms),
        servers: config
            .servers
            .iter()
            .map(|server| mcp_server(server, config, home))
            .collect(),
    }
}

/// 单个 server 的配置翻译：`cwd` 相对应用根目录解析，枚举逐项对应。
fn mcp_server(server: &McpServerConfig, config: &McpConfig, home: &Path) -> McpServerSpec {
    let concurrency = server
        .concurrency
        .map_or_else(|| concurrency_of(config.concurrency), concurrency_of);
    McpServerSpec {
        id: server.id.clone(),
        enabled: server.enabled,
        transport: match &server.transport {
            McpTransportConfig::Stdio {
                command,
                args,
                env,
                cwd,
            } => McpTransport::Stdio {
                command: command.clone(),
                args: args.clone(),
                env: env.clone().into_iter().collect(),
                cwd: resolve_cwd(cwd, home),
            },
            McpTransportConfig::Http { url, headers } => McpTransport::Http {
                url: url.clone(),
                headers: headers.clone().into_iter().collect(),
            },
        },
        connect_timeout: Duration::from_millis(config.connect_timeout_ms),
        permission: mcp_permission_of(server.permission),
        concurrency,
        file_access: FileAccessRules::new(
            server
                .file_access
                .iter()
                .map(|(name, kind)| (name.clone(), file_access_of(*kind))),
        ),
    }
}

/// 工作目录：空串表示不切换；相对路径按应用根目录（`config.toml` 所在目录）解析。
fn resolve_cwd(cwd: &str, home: &Path) -> Option<PathBuf> {
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return None;
    }
    let path = Path::new(cwd);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        home.join(path)
    })
}

/// 配置侧的 MCP 并发取值 → 工具层的并发语义。
fn concurrency_of(kind: McpConcurrencyKind) -> ToolConcurrency {
    match kind {
        McpConcurrencyKind::Parallel => ToolConcurrency::Parallel,
        McpConcurrencyKind::Exclusive => ToolConcurrency::Exclusive,
    }
}

/// 配置侧的 MCP 权限取值 → 工具层的权限判定值。
fn mcp_permission_of(kind: McpPermissionKind) -> ToolPermission {
    match kind {
        McpPermissionKind::Allow => ToolPermission::Allow,
        McpPermissionKind::Deny => ToolPermission::Deny,
        McpPermissionKind::Confirm => ToolPermission::Confirm,
    }
}

/// 配置侧的 MCP 文件操作声明 → 工具自述的声明值。
fn file_access_of(kind: McpFileAccessKind) -> ToolFileAccess {
    match kind {
        McpFileAccessKind::None => ToolFileAccess::None,
        McpFileAccessKind::Read => ToolFileAccess::Read,
        McpFileAccessKind::Write => ToolFileAccess::Write,
        McpFileAccessKind::ReadWrite => ToolFileAccess::ReadWrite,
    }
}

/// 配置侧的权限取值 → 工具层的判定值：两处枚举刻意分开，配置类型不进 `agent-tool`。
fn permission_of(kind: ToolPermissionKind) -> ToolPermission {
    match kind {
        ToolPermissionKind::Allow => ToolPermission::Allow,
        ToolPermissionKind::Deny => ToolPermission::Deny,
        ToolPermissionKind::Confirm => ToolPermission::Confirm,
    }
}
