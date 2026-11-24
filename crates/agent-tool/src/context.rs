//! 工具执行上下文：一次调用期间工具可以读到的只读参数。

use std::path::PathBuf;
use std::time::Instant;

/// 一次工具调用的执行上下文。
///
/// 只放「这一次调用是什么」的既定事实：应用根目录、所属会话与 Run、调用标识、截止时间。
/// 故意不提供 registry / 数据库 / Provider：工具不得从上下文里拿到超出自己职责的能力
/// （README §4.4）。
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// 应用根目录（`AGENT_HOME`）：文件类工具的默认可读范围。
    pub home: PathBuf,
    /// 所属会话 ID。
    pub session_id: String,
    /// 所属 Run ID。
    pub run_id: String,
    /// 本次工具调用的 ID：与模型给出的 `tool_call_id`、审计记录三者同值。
    pub tool_call_id: String,
    /// 本次调用的截止时刻：由执行器按 `[tools] timeout_ms` 算出。
    pub deadline: Instant,
}
