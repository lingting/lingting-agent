//! 工具发现：把 `tools/list` 变成稳定的、可复算的模型可见清单（D-P4-005 / D-P4-010 / D-P4-012）。
//!
//! 两步分开，便于核对：
//!
//! 1. [`fetch`]：拉取并自动翻页，直到 `nextCursor` 为空，结果按**原名升序**；
//! 2. [`resolve`]：按固定顺序（调用方保证「server 按 id 升序」，这里保证「工具按原名升序」）
//!    生成可见名、消解冲突，并把配置里的 `file_access` / 并发声明落成工具自述。
//!
//! 发现失败时状态仍是 `READY`（连接是好的），调用方保留上一次的快照并记 `lastError`。

use std::collections::BTreeSet;

use agent_tool::{ToolConcurrency, ToolFileAccess, ToolRisk};
use rmcp::model::{PaginatedRequestParams, Tool as RemoteTool};
use rmcp::service::{Peer, RoleClient};
use serde_json::Value;

use crate::config::McpServerSpec;
use crate::error::McpError;
use crate::naming;

/// server 报告的一个工具（只保留本项目用到的字段）。
#[derive(Debug, Clone, PartialEq)]
pub struct RawTool {
    /// server 里的工具原名。
    pub name: String,
    /// 工具说明（server 没给就是空串）。
    pub description: String,
    /// 原样透传的 `inputSchema`。
    pub input_schema: Value,
    /// `annotations.readOnlyHint == true`。
    pub read_only: bool,
}

impl RawTool {
    /// 从 rmcp 的工具模型取值：`inputSchema` 不剥键、不改写（D-P4-003）。
    fn of(tool: RemoteTool) -> Self {
        let read_only = tool
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.read_only_hint)
            .unwrap_or(false);
        Self {
            name: tool.name.to_string(),
            description: tool
                .description
                .map(|description| description.to_string())
                .unwrap_or_default(),
            input_schema: Value::Object(tool.input_schema.as_ref().clone()),
            read_only,
        }
    }
}

/// 一个已解析的 MCP 工具：注册到 `agent-tool` 所需要的全部自述。
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolSpec {
    /// 所属 server 的配置 `id`。
    pub server_id: String,
    /// 模型可见名（`mcp__<server>__<tool>`，冲突时带哈希后缀）。
    pub tool_name: String,
    /// server 里的原名（`tools/call` 用它发请求）。
    pub original_name: String,
    /// 工具说明。
    pub description: String,
    /// 原样透传的参数 JSON Schema。
    pub schema: Value,
    /// 风险等级：`readOnlyHint` 决定，缺省按 `Write` 处理。
    pub risk: ToolRisk,
    /// 本地文件操作声明：由 `mcp.toml` 的 `servers[].file_access` 解析（缺省 `None`）。
    pub file_access: ToolFileAccess,
    /// 并发语义：写声明恒 `Exclusive`，否则取 server `concurrency`。
    pub concurrency: ToolConcurrency,
}

impl McpToolSpec {
    /// 由原样字段与配置声明生成一条工具快照（不处理命名冲突）。
    fn build(spec: &McpServerSpec, tool: RawTool, tool_name: String) -> Self {
        let file_access = spec.file_access.access_of(&tool.name);
        // 声明了写操作就恒串行：配置不能把「会写」的工具放松成并行（D-P4-006）。
        let concurrency = if file_access.default_concurrency() == ToolConcurrency::Exclusive {
            ToolConcurrency::Exclusive
        } else {
            spec.concurrency
        };
        Self {
            server_id: spec.id.clone(),
            tool_name,
            original_name: tool.name,
            description: tool.description,
            schema: tool.input_schema,
            risk: if tool.read_only {
                ToolRisk::ReadOnly
            } else {
                ToolRisk::Write
            },
            file_access,
            concurrency,
        }
    }
}

/// 拉取该 server 的全部工具：自动翻页，结果按原名升序。
pub(crate) async fn fetch(peer: &Peer<RoleClient>, server_id: &str) -> Result<Vec<RawTool>, McpError> {
    let mut tools = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let params = cursor
            .take()
            .map(|cursor| PaginatedRequestParams::default().with_cursor(Some(cursor)));
        let page = peer
            .list_tools(params)
            .await
            .map_err(|error| McpError::from_service(server_id, None, &error))?;
        tools.extend(page.tools.into_iter().map(RawTool::of));
        match page.next_cursor {
            // 空游标等价于「没有下一页」：避免 server 用空串终止时无限翻页。
            Some(next) if !next.is_empty() => cursor = Some(next),
            _ => break,
        }
    }
    // 单次发现按原名升序：冲突消解的确定性从这里开始。
    tools.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(tools)
}

/// 把原样工具解析成快照：`taken` 是「已经被占用的名字」（内置工具与先发现的 server）。
///
/// 同一批内按原名升序处理；撞名（同批互相撞或撞 `taken`）时后到者追加 `_<8 位哈希>`，
/// 先注册者的名字不被动过。
pub fn resolve(spec: &McpServerSpec, mut raw: Vec<RawTool>, taken: &BTreeSet<String>) -> Vec<McpToolSpec> {
    raw.sort_by(|left, right| left.name.cmp(&right.name));
    let mut taken = taken.clone();
    let mut specs = Vec::with_capacity(raw.len());
    for tool in raw {
        let base = naming::visible_name(&spec.id, &tool.name);
        let mut name = base.clone();
        let mut round = 0usize;
        // 哈希后缀理论上一次就够；循环只是为了「绝不注册两个同名工具」。
        while taken.contains(&name) && round < MAX_DISAMBIGUATE_ROUNDS {
            round += 1;
            name = naming::disambiguate(&base, &format!("{}#{round}", tool.name));
        }
        taken.insert(name.clone());
        specs.push(McpToolSpec::build(spec, tool, name));
    }
    specs
}

/// 冲突消解的尝试上限：同批工具数不会超过它，超过说明 server 给了重复原名。
const MAX_DISAMBIGUATE_ROUNDS: usize = 32;
