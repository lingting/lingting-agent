//! `McpTool`：把一条已解析的工具快照适配成 `agent_tool::Tool`。
//!
//! 适配器只持有「调用能力」（`Arc<McpClient>`）与生成好的自述，不认识 manager、不知道重连；
//! 参数校验、权限、超时、取消、截断、审计全部由 P3 的执行器负责（同构而不旁路）。

use std::sync::Arc;

use agent_tool::{
    Tool, ToolConcurrency, ToolContext, ToolError, ToolFileAccess, ToolOutput, ToolRisk, ToolSchema, schema,
};
use async_trait::async_trait;
use serde_json::Value;

use crate::client::McpClient;
use crate::discover::McpToolSpec;

/// MCP 工具在运行时眼里的样子：一个普通 `Tool`。
pub struct McpTool {
    client: Arc<McpClient>,
    name: String,
    description: String,
    schema: ToolSchema,
    risk: ToolRisk,
    file_access: ToolFileAccess,
    concurrency: ToolConcurrency,
    original_name: String,
}

impl McpTool {
    /// 由发现快照构造：名字、说明、schema、风险与文件操作声明都已定好。
    pub fn new(client: Arc<McpClient>, spec: McpToolSpec) -> Self {
        Self {
            client,
            name: spec.tool_name,
            description: spec.description,
            schema: schema::from_value(spec.schema),
            risk: spec.risk,
            file_access: spec.file_access,
            concurrency: spec.concurrency,
            original_name: spec.original_name,
        }
    }

    /// 所属 server 的配置 `id`（审计与排查用）。
    pub fn server_id(&self) -> &str {
        self.client.id()
    }

    /// server 里的工具原名（`tools/call` 的 `name`）。
    pub fn original_name(&self) -> &str {
        &self.original_name
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> &ToolSchema {
        &self.schema
    }

    fn risk(&self) -> ToolRisk {
        self.risk
    }

    fn file_access(&self) -> ToolFileAccess {
        self.file_access
    }

    fn concurrency(&self) -> ToolConcurrency {
        self.concurrency
    }

    async fn call(&self, ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        // `ctx` 只用来写 trace 字段：参数可能带敏感内容，不进日志。
        tracing::debug!(
            server = %self.client.id(),
            tool = %self.original_name,
            session = %ctx.session_id,
            run = %ctx.run_id,
            call = %ctx.tool_call_id,
            "MCP 工具调用"
        );
        self.client
            .call_tool(&self.original_name, args)
            .await
            .map_err(ToolError::from)
    }
}
