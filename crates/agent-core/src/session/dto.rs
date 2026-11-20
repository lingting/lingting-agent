//! 会话领域模型：会话摘要、消息、Run 与它们的枚举。
//!
//! `i64` 字段（数量、时间戳）在 wire 上由 `auto_type` 注入 `DisplayFromStr`，TS 侧是字符串，
//! 与 `docs/develop.md` 的 `i64` 约定一致；id 一律用 `String`（D-018）。

use framework_proc_auto::{auto_enum, auto_type};

/// 消息角色。
#[auto_enum]
pub enum MessageRole {
    /// 系统提示词。
    System,
    /// 用户输入。
    User,
    /// 模型输出。
    Assistant,
    /// 工具结果（P3 起使用）。
    Tool,
}

/// Run 状态。
#[auto_enum]
pub enum RunStatus {
    /// 已创建，等待执行。
    Queued,
    /// 执行中。
    Running,
    /// 正常结束。
    Completed,
    /// 已取消。
    Cancelled,
    /// 执行失败。
    Failed,
}

/// 会话摘要：会话列表与创建会话的响应。
#[auto_type(clone = true)]
pub struct SessionSummary {
    /// 会话 ID。
    pub id: String,
    /// 会话标题。
    pub title: String,
    /// 当前使用的模型标识。
    pub model: String,
    /// 已保存的消息数量。
    pub message_count: i64,
    /// 创建时间（Unix 毫秒）。
    pub created_at_ms: i64,
    /// 最后更新时间（Unix 毫秒）。
    pub updated_at_ms: i64,
}

/// 会话消息。
#[auto_type(default = false, clone = true)]
pub struct Message {
    /// 消息 ID。
    pub id: String,
    /// 所属会话 ID。
    pub session_id: String,
    /// 产生该消息的 Run；系统侧预置消息没有 Run。
    pub run_id: Option<String>,
    /// 消息角色。
    pub role: MessageRole,
    /// 消息正文。
    pub content: String,
    /// 生成该消息时使用的模型；用户消息为空。
    pub model: Option<String>,
    /// 输入 token 用量；未统计时为空。
    pub input_tokens: Option<i64>,
    /// 输出 token 用量；未统计时为空。
    pub output_tokens: Option<i64>,
    /// 创建时间（Unix 毫秒）。
    pub created_at_ms: i64,
}

/// 一次 Agent 执行。
#[auto_type(default = false, clone = true)]
pub struct Run {
    /// Run ID。
    pub id: String,
    /// 所属会话 ID。
    pub session_id: String,
    /// 当前状态。
    pub status: RunStatus,
    /// 本次执行使用的模型标识。
    pub model: String,
    /// 失败原因；仅 `failed` 时有值，且不含内部细节。
    pub error: Option<String>,
    /// 输入 token 用量；未统计时为空。
    pub input_tokens: Option<i64>,
    /// 输出 token 用量；未统计时为空。
    pub output_tokens: Option<i64>,
    /// 创建时间（Unix 毫秒）。
    pub created_at_ms: i64,
    /// 最后更新时间（Unix 毫秒）。
    pub updated_at_ms: i64,
    /// 结束时间（Unix 毫秒）；未结束时为空。
    pub finished_at_ms: Option<i64>,
}

/// 发送消息的结果：本次写入的用户消息与对应的 Run。
#[auto_type(default = false, clone = true)]
pub struct MessageSendVO {
    /// 已保存的用户消息。
    pub message: Message,
    /// 由本次发送创建的 Run。
    pub run: Run,
}
