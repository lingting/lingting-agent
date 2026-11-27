//! Skill 管理面的契约快照（P5-012 / D-P5-015）：清单、详情与动作入参。
//!
//! 类型放在 `agent-skill`，由 `manager-agent` 的门面函数产出、`lib-web` 的 handler 直接返回
//! （与 `McpServerVO` 同一条路径）：接口层不再维护第二套 DTO（`AGENTS.md` 第 19 条）。
//!
//! 边界：`SkillVO` 由注册表的清单视图产出；`SkillDetailVO` 在它之上补正文与引用清单；
//! 动作与查询入参（`SkillUpdatePO` / `SkillGetParams`）也定义在这里，接口层只解包转发。
//!
//! 字段命名与 wire 形态由 `auto_type` / `auto_enum` 决定：`camelCase` 字段名、`Option` 字段可省略、
//! 枚举取 `SCREAMING_SNAKE_CASE`。

use framework_proc_auto::{auto_enum, auto_type};

/// Skill 的可用性状态：取值与注册表的 `SkillStatus` 一一对应。
///
/// `INVALID` 的条目**不进索引、不可加载**，但一定出现在清单里并带着 `error`（判据 E-1）。
#[auto_enum]
pub enum SkillStatusKind {
    /// 解析成功：进索引、可加载。
    Ready,
    /// 解析失败或同名冲突：进清单、带原因，不可加载。
    Invalid,
}

/// 管理接口的三个动作（`api/skill/update` 的 `action`）。
#[auto_enum]
pub enum SkillAction {
    /// 启用一条 Skill（`name` 必填）。
    Enable,
    /// 禁用一条 Skill（`name` 必填）。
    Disable,
    /// 重扫发现目录并整体替换快照（`name` 必须缺省）。
    Reload,
}

/// 一条 Skill 的清单快照（`api/skill/list` 的元素）。
///
/// 含 `INVALID` 与已禁用项：清单是「目录里有什么」的全集，禁用只影响索引与加载。
#[auto_type(default = false, clone = true)]
pub struct SkillVO {
    /// 逻辑标识（frontmatter 的 `name`；`INVALID` 条目回落到目录名，只为定位用）。
    pub name: String,
    /// frontmatter 的描述（`INVALID` 条目为空串）。
    pub description: String,
    /// 可用性状态。
    pub status: SkillStatusKind,
    /// 进程内的启用标记（`skills.toml` 的 `disabled` 名单与接口动作的结果）。
    pub enabled: bool,
    /// frontmatter 声明的工具（原样顺序）。
    pub requires_tools: Vec<String>,
    /// 声明里此刻不可用的工具（缺工具不阻断加载，只做提示）。
    pub missing_tools: Vec<String>,
    /// 相对应用根目录的目录路径（`skills/<dir>`）。
    pub directory: String,
    /// `references/` 下的引用清单条数。
    pub reference_count: u32,
    /// `INVALID` 的可定位原因；`READY` 时为空。
    pub error: Option<String>,
}

/// 一条 Skill 的详情（`api/skill/get`）：清单字段 + 正文 + 引用清单。
///
/// 字段与 [`SkillVO`] 逐个相同（同一次读取，不做两次查询），因此详情与清单不会出现「元数据与正文
/// 不一致」的窗口。禁用项与 `INVALID` 项的 `body` 为空串，原因仍由 `error` 给出。
#[auto_type(default = false, clone = true)]
pub struct SkillDetailVO {
    /// 逻辑标识。
    pub name: String,
    /// frontmatter 的描述。
    pub description: String,
    /// 可用性状态。
    pub status: SkillStatusKind,
    /// 进程内的启用标记。
    pub enabled: bool,
    /// frontmatter 声明的工具。
    pub requires_tools: Vec<String>,
    /// 声明里此刻不可用的工具。
    pub missing_tools: Vec<String>,
    /// 相对应用根目录的目录路径。
    pub directory: String,
    /// `references/` 下的引用清单条数。
    pub reference_count: u32,
    /// `INVALID` 的可定位原因。
    pub error: Option<String>,
    /// 正文原样文本（禁用或 `INVALID` 时为空串）。
    pub body: String,
    /// 引用清单（相对应用根目录的路径），读取交给 `file_read`。
    pub references: Vec<String>,
}

/// 详情的查询参数（`api/skill/get`）：`name` 必填。
///
/// 未知 `name` 是 400 `技能不存在`，不是 404（与 MCP 的 `serverId` 口径一致）。
#[auto_type]
pub struct SkillGetParams {
    /// 目标 Skill 的逻辑标识。
    pub name: String,
}

/// 管理动作的请求体（`api/skill/update`）：`name` + `action`。
#[auto_type(default = false)]
pub struct SkillUpdatePO {
    /// 目标 Skill 的逻辑标识：`ENABLE` / `DISABLE` 必填，`RELOAD` 必须缺省。
    pub name: Option<String>,
    /// 要执行的动作。
    pub action: SkillAction,
}
