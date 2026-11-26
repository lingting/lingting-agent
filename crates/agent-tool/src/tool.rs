//! `Tool` 契约：一个工具对运行时的全部自述与唯一执行入口。
//!
//! 运行时只按这份契约行事，不按工具名特判（D-P3-001 的硬约束）。

use async_trait::async_trait;
use serde_json::Value;

use crate::context::ToolContext;
use crate::definition::{ToolConcurrency, ToolFileAccess, ToolRisk};
use crate::error::ToolError;
use crate::output::ToolOutput;
use crate::schema::ToolSchema;

/// 模型可调用的能力。
///
/// 由 `Arc<dyn Tool>` 持有（`Send + Sync`，方法签名里不出现泛型参数），
/// 这样 MCP 工具（P4）与内置工具对运行时是完全同构的。
#[async_trait]
pub trait Tool: Send + Sync {
    /// 模型可见的稳定标识，必须满足 [`validate_name`] 的规则。
    ///
    /// 名字可以来自运行期（P4 的 MCP 工具：连接成功后才拿到 server 的工具名），
    /// 因此这里不要求 `&'static str`；但**任何来源的名字都要过 [`validate_name`]**，
    /// 注册期是唯一校验点。
    fn name(&self) -> &str;

    /// 工具说明：模型据此判断什么时候用它。同样可以来自运行期数据。
    fn description(&self) -> &str;

    /// 参数 JSON Schema：由参数类型派生（D-P3-002），MCP 工具则由 server 的 `inputSchema` 透传。
    fn schema(&self) -> &ToolSchema;

    /// 风险等级。
    fn risk(&self) -> ToolRisk;

    /// 本地文件操作声明（D-P3-021）。
    fn file_access(&self) -> ToolFileAccess;

    /// 并发语义：默认由 [`ToolFileAccess::default_concurrency`] 推导，工具可覆盖。
    fn concurrency(&self) -> ToolConcurrency {
        self.file_access().default_concurrency()
    }

    /// 该错误是否值得重试：默认不重试（D-P3-008）。
    fn retryable(&self, _error: &ToolError) -> bool {
        false
    }

    /// 执行一次调用。`args` 是模型给出的原始 JSON，由实现用 `schema::parse` 反序列化。
    async fn call(&self, ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError>;
}

/// 工具名规则：小写 `snake_case`，长度 2 ~ 64（`^[a-z][a-z0-9_]{1,63}$`）。
///
/// 注册期调用：非法名字直接拒绝注册，模型因此不会看到一个不符合规则的标识。
pub fn validate_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() < 2 || bytes.len() > 64 {
        return false;
    }
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}
