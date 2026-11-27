//! Skill 的装配侧适配：发现、`skill_load` 注册、工具可用性判定与门面（P5-006 / P5-010）。
//!
//! 本模块只做「翻译 + 发现 + 注册 + 日志 + 转发」：`skills.toml` 的上限翻译成 `agent-skill` 的
//! `SkillLimits`，目录扫描交给 `agent_skill::discover`，其余领域逻辑都在 `agent-skill` 内完成
//! （判据 A-4）。
//!
//! 判定口径只有一条：**此刻能不能真的执行**。未注册的工具算不可用（MCP 还没连上、或 server 的
//! 工具快照里没有它），权限为 `deny` 的工具也算不可用——它不会出现在模型的工具清单里。
//! 判定发生在渲染索引与加载正文时（不是发现时），因此 MCP 在运行期连上之后，缺失标记会自动消失，
//! 不需要重载 Skill。
//!
//! 管理面（P5-012）：`skill_list` / `skill_get` / `skill_update` 三个门面自由函数只做转发，
//! 清单、详情与动作口径在 `agent-skill` 的 `manager` 模块；只有 `RELOAD` 的重扫发生在这里
//! （扫描需要应用根目录，而它归装配层持有，D-P5-008）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_skill::metrics::SkillStateCounts;
use agent_skill::{
    SkillAction, SkillDetailVO, SkillGetParams, SkillIndexSource, SkillLimits, SkillLoad, SkillManagerError,
    SkillRegistry, SkillUpdatePO, SkillVO, ToolAvailability,
};
use agent_tool::{ToolPermission, ToolRegistry};
use anyhow::{Context, Result};
use types_core::config::SkillsConfig;

/// 一次装配的 Skill 部件：注册表、可用性判定器与索引进 Context 的入口。
pub struct SkillManager {
    /// `skills.toml` 的 `enabled`：为假时不发现、不注册、不注入（等价 P2 行为）。
    enabled: bool,
    /// 快照 + 启用状态 + 上限的唯一入口。
    registry: Arc<SkillRegistry>,
    /// 工具可用性判定器（索引渲染与 `skill_load` 共用同一份）。
    availability: Arc<dyn ToolAvailability>,
    /// 索引进系统提示的入口；`enabled = false` 时为空。
    source: Option<Arc<SkillIndexSource>>,
    /// 应用根目录：`RELOAD` 重扫时作为相对路径的基准。
    home: PathBuf,
    /// 发现根目录（`skills.toml` 的 `root`）：相对应用根目录解析，绝对路径按原样。
    root: PathBuf,
}

impl SkillManager {
    /// 注册表：清单、加载、启停都从这里取。
    pub fn registry(&self) -> &Arc<SkillRegistry> {
        &self.registry
    }

    /// 工具可用性判定器。
    pub fn availability(&self) -> &Arc<dyn ToolAvailability> {
        &self.availability
    }

    /// Skill 功能是否启用（`skills.toml` 的 `enabled`）。
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 索引来源：`None` 表示不注入（`enabled = false`）。
    pub fn source(&self) -> Option<&Arc<SkillIndexSource>> {
        self.source.as_ref()
    }

    /// 清单：全部条目（含 `INVALID` 与已禁用项），按 `name` 升序。
    pub fn list(&self) -> Vec<SkillVO> {
        agent_skill::manager::list(&self.registry, self.availability.as_ref())
    }

    /// 详情：清单字段 + 正文 + 引用清单；未知名字是 [`SkillManagerError::NotFound`]。
    pub fn detail(&self, name: &str) -> std::result::Result<SkillDetailVO, SkillManagerError> {
        agent_skill::manager::detail(&self.registry, self.availability.as_ref(), name)
    }

    /// 执行一个管理动作，返回动作完成后的整张清单（D-P5-015）。
    ///
    /// 参数级校验先于查找（`agent-skill` 的 `action_name`）；`ENABLE` / `DISABLE` 落到注册表的启用状态，
    /// `RELOAD` 重扫目录并整体替换快照——`disabled` 名单由注册表保留（D-P5-008）。
    pub fn update(&self, po: &SkillUpdatePO) -> std::result::Result<Vec<SkillVO>, SkillManagerError> {
        let name = agent_skill::manager::action_name(po.action, po.name.as_deref())?;
        match (po.action, name) {
            (SkillAction::Reload, _) => {
                self.reload();
                Ok(self.list())
            }
            (SkillAction::Enable, Some(name)) => self.apply(name, true),
            (SkillAction::Disable, Some(name)) => self.apply(name, false),
            // `action_name` 已保证 `ENABLE` / `DISABLE` 一定带着名字。
            (SkillAction::Enable | SkillAction::Disable, None) => Err(SkillManagerError::MissingName),
        }
    }

    /// 重扫发现目录并整体替换快照（`RELOAD`）；`skills.toml` 的 `enabled = false` 时是空操作。
    ///
    /// 日志只记计数与状态，不记正文与 frontmatter 全文（D-P5-018）。
    pub fn reload(&self) {
        if !self.enabled {
            return;
        }
        let snapshot = agent_skill::discover(&self.home, &self.root, self.registry.limits());
        let ready = snapshot.entries().iter().filter(|entry| entry.is_ready()).count();
        let invalid = snapshot.len() - ready;
        self.registry.reload(snapshot);
        tracing::info!(discovered = ready + invalid, ready, invalid, "Skill 重载完成");
    }

    /// 取快照时刻的 Skill 现状（`metrics` 的 `skills` 段用，不做累加）。
    pub fn state_counts(&self) -> SkillStateCounts {
        SkillStateCounts {
            discovered: count(self.registry.entry_count()),
            invalid: count(self.registry.invalid_count()),
            enabled: count(self.registry.enabled_count()),
        }
    }

    /// 切换一条 Skill 的启用状态：名字不存在（含功能未启用）一律 [`SkillManagerError::NotFound`]。
    fn apply(&self, name: &str, enabled: bool) -> std::result::Result<Vec<SkillVO>, SkillManagerError> {
        self.registry
            .set_enabled(name, enabled)
            .map_err(|_| SkillManagerError::NotFound)?;
        Ok(self.list())
    }
}

/// 全部 Skill 的清单（`api/skill/list`）：未装配时为空清单，不报错。
pub fn skill_list() -> Vec<SkillVO> {
    crate::holder::holder().map_or_else(Vec::new, |installed| installed.skills().list())
}

/// 一条 Skill 的详情（`api/skill/get`）：未知名字（含未装配）是 [`SkillManagerError::NotFound`]。
pub fn skill_get(params: &SkillGetParams) -> Result<SkillDetailVO> {
    let Some(installed) = crate::holder::holder() else {
        return Err(anyhow::Error::new(SkillManagerError::NotFound));
    };
    installed.skills().detail(&params.name).map_err(anyhow::Error::new)
}

/// 执行一个管理动作并返回整张清单（`api/skill/update`）。
///
/// 未装配时：`RELOAD` 是空操作（返回空清单），`ENABLE` / `DISABLE` 按「技能不存在」拒绝；两种情况都
/// 先过一遍参数级校验，文案与装配后完全一致。
pub fn skill_update(po: &SkillUpdatePO) -> Result<Vec<SkillVO>> {
    let Some(installed) = crate::holder::holder() else {
        agent_skill::manager::action_name(po.action, po.name.as_deref()).map_err(anyhow::Error::new)?;
        return match po.action {
            SkillAction::Reload => Ok(Vec::new()),
            SkillAction::Enable | SkillAction::Disable => Err(anyhow::Error::new(SkillManagerError::NotFound)),
        };
    };
    installed.skills().update(po).map_err(anyhow::Error::new)
}

/// `usize` → `u64`：饱和，不 panic。
fn count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// 装配 Skill：发现一次快照、落启动期禁用名单、注册 `skill_load`，并逐条记装配日志。
///
/// 装配顺序由调用方保证（`assemble_with`）：本函数必须在 MCP 接入之前调用——`skill_load` 先入
/// 注册表，MCP 的工具随后进来，「缺工具」因此天然按运行期现状判定（D-P5-009）。
///
/// 日志只记相对目录、名字与状态，不记正文与 frontmatter 全文（D-P5-018）。
pub fn build_skills(config: &SkillsConfig, home: &Path, tools: &Arc<ToolRegistry>) -> Result<Arc<SkillManager>> {
    let limits = limits(config);
    let registry = Arc::new(SkillRegistry::new(limits));
    let availability: Arc<dyn ToolAvailability> = Arc::new(RegistryAvailability::new(Arc::clone(tools)));

    if !config.enabled {
        tracing::info!("skills.toml 的 enabled = false：不发现、不注册、不注入 Skill");
        return Ok(Arc::new(SkillManager {
            enabled: false,
            registry,
            availability,
            source: None,
            home: home.to_path_buf(),
            root: config.root.clone(),
        }));
    }

    let snapshot = agent_skill::discover(home, &config.root, &limits);
    let ready = snapshot.entries().iter().filter(|entry| entry.is_ready()).count();
    let invalid = snapshot.len() - ready;
    registry.reload(snapshot);
    // 启动期禁用名单：只作用于此刻存在的名字；`RELOAD` 之后由 `reload` 保留（D-P5-008）。
    for name in &config.disabled {
        // 名字不在快照里时 `set_enabled` 返回 `UnknownSkill`：名字已被删掉，无需记录。
        if registry.set_enabled(name, false).is_ok() {
            tracing::info!(skill = %name, "Skill 已按 skills.toml 禁用");
        }
    }
    log_entries(&registry);

    tools
        .register(Arc::new(SkillLoad::new(
            Arc::clone(&registry),
            Arc::clone(&availability),
        )))
        .context("注册 skill_load 工具失败")?;

    let source = Arc::new(SkillIndexSource::new(Arc::clone(&registry), Arc::clone(&availability)));
    // 启用数由 `index` 的口径给出（`READY && enabled`），与清单接口同源。
    tracing::info!(
        discovered = ready + invalid,
        ready,
        invalid,
        enabled = registry.enabled_count(),
        "Skill 装配完成"
    );
    Ok(Arc::new(SkillManager {
        enabled: true,
        registry,
        availability,
        source: Some(source),
        home: home.to_path_buf(),
        root: config.root.clone(),
    }))
}

/// 逐条记装配结果：坏的也进清单，但必须让启动日志里看得到原因（判据 E-1）。
///
/// 只记相对目录、名字、状态与计数，不记正文与 frontmatter 全文（D-P5-018）。
fn log_entries(registry: &SkillRegistry) {
    for entry in registry.snapshot().entries() {
        match entry.error() {
            Some(error) => tracing::warn!(
                skill = %entry.name,
                directory = %entry.directory,
                error = %error,
                "Skill 不可用"
            ),
            None => tracing::info!(
                skill = %entry.name,
                directory = %entry.directory,
                chars = entry.body_chars,
                sections = entry.sections.len(),
                "Skill 已发现"
            ),
        }
    }
}

/// `skills.toml` 的四个上限 → `agent-skill` 自己的上限类型（两套类型只在这一层相遇）。
fn limits(config: &SkillsConfig) -> SkillLimits {
    SkillLimits {
        max_index_chars: config.max_index_chars,
        max_description_chars: config.max_description_chars,
        max_skill_bytes: config.max_skill_bytes,
        max_inline_chars: config.max_inline_chars,
    }
}

/// 按工具注册表的现状回答「这个工具可用吗」。
pub struct RegistryAvailability(Arc<ToolRegistry>);

impl RegistryAvailability {
    /// 用装配期建好的注册表建一个判定器。
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self(registry)
    }
}

impl ToolAvailability for RegistryAvailability {
    fn available(&self, name: &str) -> bool {
        // `permission_of` 只对已注册的工具返回 `Some`，因此这一句同时覆盖「未注册」与「被拒绝」。
        matches!(self.0.permission_of(name), Some(permission) if permission != ToolPermission::Deny)
    }
}
