//! 一次工具调用的审计记录与脱敏（P3-009）。
//!
//! 记录是**纯数据**：字段与迁移 `0002` 的 `tool_calls` 列一一对应，不持有 `Arc<dyn Tool>`
//! 之类的运行时句柄。参数在**写出侧**脱敏（D-P3-011），落库的是已脱敏文本，查询接口不再二次处理。
//!
//! 字段对应关系（`id` / `run_id` / `session_id` 由 Service 在插入时补齐，因此不在记录里）：
//!
//! ```text
//! tool_call_id -> tool_call_id    tool_name    -> tool_name
//! file_access  -> file_access     status       -> status（serde wire 值）
//! arguments    -> arguments       result       -> result
//! duration_ms  -> duration_ms     tool_round   -> tool_round
//! created_at_ms-> created_at_ms
//! ```
//!
//! 失败原因也存进 `result`：OpenAI 兼容协议的工具结果没有 error 字段，审计表也只有一列 `result`，
//! 因此不额外造一个无列可落的 `error` 字段。

use framework_proc_auto::{auto_enum, auto_type};
use lib_core::time::now_millis;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::call::ToolCall;
use crate::definition::ToolFileAccess;
use crate::output::truncate;

/// 审计里 `arguments` 的整体上限（字符）。
pub const MAX_ARGUMENT_CHARS: usize = 2048;

/// 用户驳回时回灌给模型的固定文本（P3-016）。
pub const REJECTED_TEXT: &str = "用户拒绝调用该工具";

/// 命中的敏感键名（大小写不敏感，按「包含」匹配）。
const SECRET_KEYS: [&str; 7] = [
    "api_key",
    "apikey",
    "token",
    "secret",
    "password",
    "authorization",
    "cookie",
];

/// 一次工具调用的状态。
///
/// 取值与 Run 事件、审计表共用（`serde` wire 值是 SCREAMING_SNAKE_CASE）：
/// `PENDING` 只出现在挂起等待确认期间，是审计表里的中间态，不会出现在 `tool_call_completed` 事件里。
#[auto_enum]
pub enum ToolCallStatus {
    /// 执行成功。
    Ok,
    /// 执行失败（含可重试失败重试用尽）。
    Error,
    /// 权限拒绝或未注册：工具没有被调用。
    Denied,
    /// 执行超时。
    Timeout,
    /// 执行期间 Run 被取消。
    Cancelled,
    /// 参数不合法：工具没有被真正执行。
    InvalidArguments,
    /// 等待客户端确认（挂起态）。
    Pending,
}

/// 一次工具调用的审计记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallRecord {
    /// 模型给出的调用标识。
    pub tool_call_id: String,
    /// 工具名。
    pub tool_name: String,
    /// 本地文件操作声明（事后判断一次调用对本地文件的影响面）。
    pub file_access: ToolFileAccess,
    /// 调用状态。
    pub status: ToolCallStatus,
    /// 已脱敏并截断的参数文本。
    pub arguments: String,
    /// 结果或错误文本（已脱敏、已按 `max_result_chars` 截断）；挂起时为空。
    pub result: Option<String>,
    /// 耗时（毫秒），含重试。
    pub duration_ms: u64,
    /// 第几轮工具调用（从 1 开始）。
    pub tool_round: u32,
    /// 记录产生时间（Unix 毫秒）。
    pub created_at_ms: i64,
}

impl ToolCallRecord {
    /// 以「一次调用的既定事实」建记录：状态为 `PENDING`，结果与耗时由调用方补齐。
    ///
    /// 挂起时直接用这一条记录插入（`PENDING` 是挂起的事实源），确认后由同一条记录收尾。
    pub fn pending(call: &ToolCall, file_access: ToolFileAccess) -> Self {
        Self {
            tool_call_id: call.tool_call_id.clone(),
            tool_name: call.tool_name.clone(),
            file_access,
            status: ToolCallStatus::Pending,
            arguments: redact_arguments(&call.arguments),
            result: None,
            duration_ms: 0,
            tool_round: call.tool_round,
            created_at_ms: now_millis(),
        }
    }

    /// 收尾：写入最终状态、模型可见文本与耗时。
    pub fn finished(mut self, status: ToolCallStatus, result: impl Into<String>, duration_ms: u64) -> Self {
        self.status = status;
        self.result = Some(result.into());
        self.duration_ms = duration_ms;
        self
    }

    /// 用户驳回：状态 `DENIED`，结果文本固定。
    pub fn rejected(mut self) -> Self {
        self.status = ToolCallStatus::Denied;
        self.result = Some(REJECTED_TEXT.to_owned());
        self
    }
}

/// 审计记录的契约快照：`api/tool/audit/list` 的响应载荷（P3-019）。
///
/// 与 `ToolCallRecord` 字段同构，差别只有两处：`duration_ms` 用 `i64`（与其余 `i64` 字段一样
/// 走字符串 wire 形态），且不携带 `run_id` / `session_id`（查询本身已经限定了 Run）。
#[auto_type(default = false, clone = true)]
pub struct ToolCallVO {
    /// 模型给出的调用标识。
    pub tool_call_id: String,
    /// 工具名。
    pub tool_name: String,
    /// 本地文件操作声明（事后判断一次调用对本地文件的影响面）。
    pub file_access: ToolFileAccess,
    /// 调用状态；`PENDING` 表示仍在等待确认。
    pub status: ToolCallStatus,
    /// 已脱敏并截断的参数文本。
    pub arguments: String,
    /// 结果或错误文本；挂起时为空。
    pub result: Option<String>,
    /// 耗时（毫秒），含重试。
    pub duration_ms: i64,
    /// 第几轮工具调用（从 1 开始）。
    pub tool_round: u32,
    /// 记录产生时间（Unix 毫秒）。
    pub created_at_ms: i64,
}

impl From<&ToolCallRecord> for ToolCallVO {
    fn from(record: &ToolCallRecord) -> Self {
        Self {
            tool_call_id: record.tool_call_id.clone(),
            tool_name: record.tool_name.clone(),
            file_access: record.file_access,
            status: record.status,
            arguments: record.arguments.clone(),
            result: record.result.clone(),
            // 耗时的 `u64` 转 `i64` 饱和：真实耗时不可能到 2^63 毫秒。
            duration_ms: i64::try_from(record.duration_ms).unwrap_or(i64::MAX),
            tool_round: record.tool_round,
            created_at_ms: record.created_at_ms,
        }
    }
}

/// 审计查询的查询参数：按 Run 查该次执行的全部工具调用。
#[auto_type]
pub struct ToolAuditListParams {
    /// 目标 Run ID；未知 ID 返回空列表，不返回 404。
    pub run_id: String,
}

/// 脱敏参数并转成审计文本：命中的键值替换为 `***`，整体按 [`MAX_ARGUMENT_CHARS`] 截断。
pub fn redact_arguments(arguments: &Value) -> String {
    let text = redact_value(arguments).to_string();
    truncate(&text, MAX_ARGUMENT_CHARS).0
}

/// 递归脱敏：键名命中即整值替换，未命中的键继续往里走。
fn redact_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let redacted = if is_secret_key(key) {
                        Value::String("***".to_owned())
                    } else {
                        redact_value(value)
                    };
                    (key.clone(), redacted)
                })
                .collect::<Map<String, Value>>(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        other => other.clone(),
    }
}

/// 键名是否命中敏感词：大小写不敏感，`access_token` / `x-api-key` 这类组合也算命中。
fn is_secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    SECRET_KEYS.iter().any(|secret| key.contains(secret))
}
