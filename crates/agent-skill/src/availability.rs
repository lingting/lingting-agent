//! 工具可用性判定与降级文案的**唯一产点**（P5-006 / D-P5-009）。
//!
//! 判定时机是渲染索引与加载正文时，**不是发现时**：MCP 在 Skill 之后才注册工具，发现期判定会
//! 把「连接还没完成」误判成「工具不存在」，而且之后不会自愈。
//!
//! 两处文案都在这里：索引行的后缀 `（缺少工具：a、b）` 与加载结果末尾的「缺少的工具」小节。
//! 缺工具**不阻断**加载（Skill 里往往只有部分步骤依赖该工具），但必须把约束显式交给模型。

/// 工具是否可用：只认名字，不依赖 `agent-tool` 的实现细节。
///
/// 实现由装配层给出（按工具注册表的现状判定，权限 `deny` 也算不可用）；本 Crate 不持有注册表。
pub trait ToolAvailability: Send + Sync {
    /// 名字对应的工具此刻是否可用。
    fn available(&self, name: &str) -> bool;
}

/// 索引行里缺失清单的前缀。
pub const INDEX_MISSING_PREFIX: &str = "（缺少工具：";
/// 索引行里缺失清单的后缀。
pub const INDEX_MISSING_SUFFIX: &str = "）";
/// 加载结果里缺失小节的标题。
pub const MISSING_HEADING: &str = "## 缺少的工具";
/// 加载结果里缺失小节末尾的约束语。
pub const MISSING_CONSTRAINT: &str = "涉及这些工具的步骤必须停止，并明确告知用户无法执行。";
/// 多个工具名的分隔符。
const LIST_SEPARATOR: &str = "、";

/// 声明里此刻不可用的工具名，保持 `requires_tools` 的原有顺序。
pub fn missing_tools(requires: &[String], availability: &dyn ToolAvailability) -> Vec<String> {
    requires
        .iter()
        .filter(|name| !availability.available(name))
        .cloned()
        .collect()
}

/// 索引行的后缀：没有缺失时是空串（索引行格式因此只由这里决定）。
pub fn index_suffix(missing: &[String]) -> String {
    if missing.is_empty() {
        return String::new();
    }
    format!(
        "{INDEX_MISSING_PREFIX}{}{INDEX_MISSING_SUFFIX}",
        missing.join(LIST_SEPARATOR)
    )
}

/// 加载结果末尾的「缺少的工具」小节：没有缺失时没有这一节。
pub fn missing_section(missing: &[String]) -> Option<String> {
    if missing.is_empty() {
        return None;
    }
    let mut text = String::from(MISSING_HEADING);
    for name in missing {
        text.push_str("\n- ");
        text.push_str(name);
    }
    text.push_str("\n\n");
    text.push_str(MISSING_CONSTRAINT);
    Some(text)
}
