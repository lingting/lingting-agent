//! Skill 配置（`skills.toml`，P5-002 / D-P5-007）。
//!
//! Skill 的配置**独立成文件**（应用根目录下的 `skills.toml`），不写进 `config.toml`：与 `mcp.toml`
//! 同构，一个能力一个文件，改一个能力的配置不牵动主配置的重启节奏。文件内容就是这一段配置本身：
//!
//! ```toml
//! enabled = true
//! root = "skills"
//! disabled = ["log-triage"]
//! max_index_chars = 4000
//! max_description_chars = 200
//! max_skill_bytes = 65536
//! max_inline_chars = 6000
//! ```
//!
//! 所有字段有缺省值；文件缺失等价于「全部缺省」（Skill 功能照常启用），文件存在但内容为空则是
//! 启动期硬错误（不静默降级）。
//!
//! 装载与校验在 `lib-bin-cli::config`：空 `root`、`0` 上限、`max_description_chars >
//! max_index_chars`、`disabled` 的空名字与非法名字、`enabled` 类型错都是启动期硬错误（退出码 2），
//! 错误文案给出字段名。
//!
//! 启停状态只在内存（`disabled` 名单是运维开关，不是业务数据）：本阶段**不做数据库迁移**。

use std::path::PathBuf;

use serde::Deserialize;

/// 索引字符数上限的缺省值：索引（名字 + 描述 + 依赖工具状态）进系统提示的预算。
pub const DEFAULT_MAX_INDEX_CHARS: usize = 4_000;
/// 单条描述字符数上限的缺省值：超出即截断（不报错）。
pub const DEFAULT_MAX_DESCRIPTION_CHARS: usize = 200;
/// 单个 `SKILL.md` 字节上限的缺省值（64 KiB）。
pub const DEFAULT_MAX_SKILL_BYTES: u64 = 65_536;
/// 单次 `skill_load` 返回正文字符数上限的缺省值。
pub const DEFAULT_MAX_INLINE_CHARS: usize = 6_000;
/// 发现根目录的缺省值（相对应用根目录）。
pub const DEFAULT_ROOT: &str = "skills";

/// `skills.toml` 的顶层配置。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SkillsConfig {
    /// Skill 功能总开关：`false` 时不发现、不注入、不注册工具，`api/skill/list` 返回空清单。
    pub enabled: bool,
    /// 发现根目录：相对应用根目录解析，绝对路径按原样。
    pub root: PathBuf,
    /// 启动即禁用的名字清单；`RELOAD` 之后保留（只保留仍然存在的名字）。
    pub disabled: Vec<String>,
    /// 索引（进系统提示的那一段）字符数上限。
    pub max_index_chars: usize,
    /// 单条描述在索引里的字符数上限。
    pub max_description_chars: usize,
    /// 单个 `SKILL.md` 的字节上限；超出的 Skill 判 `INVALID`。
    pub max_skill_bytes: u64,
    /// 单次 `skill_load` 返回正文字符数上限。
    pub max_inline_chars: usize,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            root: PathBuf::from(DEFAULT_ROOT),
            disabled: Vec::new(),
            max_index_chars: DEFAULT_MAX_INDEX_CHARS,
            max_description_chars: DEFAULT_MAX_DESCRIPTION_CHARS,
            max_skill_bytes: DEFAULT_MAX_SKILL_BYTES,
            max_inline_chars: DEFAULT_MAX_INLINE_CHARS,
        }
    }
}
