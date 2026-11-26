//! Skills（P5）：`SKILL.md` 解析、章节切分、引用扫描与 `skill_load` 适配。
//!
//! 本 Crate 是「任务知识」的唯一来源：Skill 告诉模型**怎么做**（步骤、约束、参考材料），
//! 不提供任何执行能力——所有动作仍经 P3 / P4 的工具与权限体系。
//!
//! 依赖方向：`agent-skill -> agent-tool / agent-context-core / agent-core / lib-core`。
//!
//! 边界：
//!
//! - 本 Crate 只产出知识：元数据、正文、章节、引用清单。唯一的工具形态是 `skill_load` 这一个
//!   只读检索工具，它**不执行** Skill 声明的任何工具（D-P5-001）。
//! - 不依赖 `types-core`：`skills.toml` 由装配层读成上限参数再喂进来（与 `agent-tool`
//!   处理 `[tools]`、`agent-mcp` 处理 `mcp.toml` 的口径一致，D-P5-007）。
//! - 不依赖 `agent-runtime` / `agent-route` / `agent-context`：索引进系统提示、正文按需加载都
//!   由上层调用本 Crate 的纯函数拼装，本 Crate 不认识 Agent Loop 的时序与事件。
//! - 不依赖 `service-core` / `lib-db` / `lib-web*` / `bin-*`：本 Crate 不认识持久化与接口。
//!
//! 模块分工（一个概念一个落点）：
//!
//! - [`skill`]：解析入口（`SKILL.md` → [`SkillFile`]）与名字规则；
//! - [`frontmatter`]：YAML frontmatter 的字段表与校验（未知字段忽略，D-P5-004）；
//! - [`section`]：ATX 标题的章节切分（自研按行扫描，不引入 Markdown crate，D-P5-017）；
//! - [`reference`]：`references/` 的引用清单扫描（不跟随符号链接、无 `..`）；
//! - [`error`]：固定文案表，唯一构造点（D-P5-018）。

mod error;
mod frontmatter;
mod reference;
mod section;
mod skill;

pub use error::SkillError;
pub use reference::{MAX_REFERENCES, scan_references};
pub use section::SkillSection;
pub use skill::{SkillFile, SkillMetadata, parse, validate_name};
