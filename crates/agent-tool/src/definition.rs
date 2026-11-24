//! 工具定义与自述声明：发给模型看的部分，以及「这个工具会怎么动本地文件」。
//!
//! 两者刻意分离：`ToolFileAccess` 是给运行时与审计看的自述，不进入发给模型的定义
//! （`LlmToolDefinition` 只有 name / description / parameters，D-P3-021）。

use framework_proc_auto::{auto_enum, auto_type};

use crate::schema::ToolSchema;
use crate::tool::Tool;

/// 工具的风险等级：描述「这个工具危险不危险」，不区分操作的是本地文件还是网络。
#[auto_enum]
pub enum ToolRisk {
    /// 只读能力（计算、查询、读本地文件）。
    ReadOnly,
    /// 会主动向外部发起请求。
    Network,
    /// 会写本地文件。
    Write,
}

/// 本地文件操作声明：`None`（不操作）/ `Read`（只读）/ `Write`（只写）/ `ReadWrite`（读写）。
///
/// 这是工具的自述，编译器不校验（D-P3-021）。它只有两个用途：推导并发默认值、进定义与审计。
/// 权限判定不看它（权限只由 `[tools.permissions]` 与 [`ToolRisk`] 决定）。
#[auto_enum]
pub enum ToolFileAccess {
    /// 不操作本地文件。
    None,
    /// 只读本地文件。
    Read,
    /// 只写本地文件。
    Write,
    /// 读写本地文件。
    ReadWrite,
}

impl ToolFileAccess {
    /// 由声明推导并发默认值：写类工具串行，其余可并行（D-P3-021）。
    pub fn default_concurrency(self) -> ToolConcurrency {
        match self {
            Self::None | Self::Read => ToolConcurrency::Parallel,
            Self::Write | Self::ReadWrite => ToolConcurrency::Exclusive,
        }
    }
}

/// 一轮内同一工具的多次调用能否并发：`Parallel` 可并发，`Exclusive` 独占（D-P3-006）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolConcurrency {
    /// 可与同轮其它调用并发执行。
    Parallel,
    /// 独占执行：把这一轮切成段，段间串行。
    Exclusive,
}

/// 一个工具的稳定定义：模型可见部分 + 运行期与审计需要的自述。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDefinition {
    /// 工具名（模型可见的稳定标识）。
    pub name: String,
    /// 工具说明。
    pub description: String,
    /// 面向模型的参数 JSON Schema。
    pub schema: ToolSchema,
    /// 风险等级。
    pub risk: ToolRisk,
    /// 本地文件操作声明。
    pub file_access: ToolFileAccess,
}

impl ToolDefinition {
    /// 取工具的当前定义。
    pub fn of(tool: &dyn Tool) -> Self {
        Self {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            schema: tool.schema().clone(),
            risk: tool.risk(),
            file_access: tool.file_access(),
        }
    }
}

/// 工具定义的契约快照：让 SDK 与前端看到「有哪些工具、参数长什么样」。
///
/// 参数 schema 以 JSON 文本承载：它本身就是一段 JSON，前端直接解析即可，
/// 不必为任意 JSON Schema 再生成一套类型。
#[auto_type(default = false)]
pub struct ToolDefinitionVO {
    /// 工具名（模型可见的稳定标识）。
    pub name: String,
    /// 工具说明。
    pub description: String,
    /// 参数 JSON Schema 的文本形式，与发给模型的定义同源。
    pub parameters: String,
    /// 风险等级。
    pub risk: ToolRisk,
    /// 本地文件操作声明。
    pub file_access: ToolFileAccess,
}

impl From<&ToolDefinition> for ToolDefinitionVO {
    fn from(definition: &ToolDefinition) -> Self {
        Self {
            name: definition.name.clone(),
            description: definition.description.clone(),
            parameters: definition.schema.to_json_string(),
            risk: definition.risk,
            file_access: definition.file_access,
        }
    }
}
