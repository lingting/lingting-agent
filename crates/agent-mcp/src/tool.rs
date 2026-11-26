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
        // 执行器的超时会把本 future 直接 drop：守卫据此区分「超时」与「Run 取消」。
        let mut guard = CallGuard::new(ctx);
        let result = self
            .client
            .call_tool(&self.original_name, args)
            .await
            .map_err(ToolError::from);
        guard.complete();
        result
    }
}

/// 调用未完成的守卫：被丢弃时按 `ToolContext::deadline` 判断是不是超时。
///
/// 超时的判定权在执行器（`[tools] timeout_ms`，D-P4-018）：MCP 侧不自己计时，只在
/// **超时把 future 丢掉**这一事实发生时记一次 `call_timeouts`；Run 取消（未到 deadline）
/// 不算超时。丢弃本身不会阻塞任何路径。
struct CallGuard {
    /// 执行器给出的截止时刻。
    deadline: std::time::Instant,
    /// 是否已正常拿到结果。
    completed: bool,
}

impl CallGuard {
    /// 进入一次调用。
    fn new(ctx: &ToolContext) -> Self {
        Self {
            deadline: ctx.deadline,
            completed: false,
        }
    }

    /// 调用正常返回（成功或失败都算完成）。
    fn complete(&mut self) {
        self.completed = true;
    }
}

impl Drop for CallGuard {
    fn drop(&mut self) {
        if !self.completed && std::time::Instant::now() >= self.deadline {
            crate::metrics::record_call_timeout();
        }
    }
}
