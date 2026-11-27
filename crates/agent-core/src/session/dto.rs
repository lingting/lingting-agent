//! 会话领域模型：会话摘要、消息、Run 与它们的枚举。
//!
//! `i64` 字段（数量、时间戳）在 wire 上由 `auto_type` 注入 `DisplayFromStr`，TS 侧是字符串，
//! 与 `docs/develop.md` 的 `i64` 约定一致；id 一律用 `String`（D-018）。

use framework_proc_auto::{auto_enum, auto_type};

use super::checkpoint::CheckpointPhase;

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
    /// 等待工具确认：`pending_tool_call_id` 指明在等哪一次调用，不占全局并发许可，
    /// 也不阻塞同会话后续 Run（D-P3-027）。
    Waiting,
    /// 非预期中断留下的可恢复中间态（D-P6-004）。
    ///
    /// 三条语义：非终态，可以被恢复成 `RUNNING`、也可以被取消成 `CANCELLED`；只能由启动自愈从
    /// `RUNNING` 产生；不占并发许可、不阻塞同会话后续 Run、不计入运行中计数。
    /// 消费它的唯一路径是「会话被占用 + 该会话收到一条新消息」（D-P6-009）。
    Paused,
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

/// 助手消息里的工具调用清单（消息契约侧的一份）。
///
/// 与 `agent-route` 的 `LlmToolCall` 字段同构但不共用类型：Provider 协议类型不出
/// `agent-route`（D-P3-010），消息契约只承载「发生过哪次调用」这个事实。
#[auto_type(default = false, clone = true)]
pub struct LlmToolCallVO {
    /// 调用标识：与工具结果消息的 `tool_call_id` 配对。
    pub id: String,
    /// 工具名。
    pub name: String,
    /// 参数：模型给出的完整 JSON 文本，读取方自己解析。
    pub arguments: String,
}

/// 会话消息。
///
/// 工具轨迹靠三个可选字段表达（D-P3-003）：助手消息带 `tool_calls`，工具结果消息带
/// `tool_call_id` + `tool_name`，普通消息三者皆空。
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
    /// 消息正文；助手请求调用时可以为空串。
    pub content: String,
    /// 工具结果应答的调用标识；只有 `tool` 角色的消息有值。
    pub tool_call_id: Option<String>,
    /// 工具结果对应的工具名；只有 `tool` 角色的消息有值（客户端展示用）。
    pub tool_name: Option<String>,
    /// 助手本轮请求的调用清单；只有 `assistant` 消息可能非空。
    pub tool_calls: Option<Vec<LlmToolCallVO>>,
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
    /// 中断原因（固定文案）；仅 `paused` 时有值，不含内部细节与环境路径（D-P6-004）。
    pub interrupted_reason: Option<String>,
    /// 已成功推进的恢复次数；每次 `PAUSED -> RUNNING` 加一（D-P6-006）。
    pub resume_count: i64,
    /// 最近一条检查点的序号；没有检查点或升级前的旧 Run 为空（D-P6-001）。
    pub last_checkpoint_seq: Option<i64>,
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

/// 一个 Run 的步骤历史：状态在 `Run` 上，过程在这些步骤上（D-P6-001 / P6A-013）。
///
/// 步骤只暴露派生字段，`payload` 原文不外泄（验收 `I-2`）；升级前的旧 Run 没有检查点，
/// `steps` 是空数组而不是错误。
#[auto_type(default = false, clone = true)]
pub struct RunHistory {
    /// 该 Run 的当前状态快照。
    pub run: Run,
    /// 已成功推进的恢复次数。
    pub resume_count: i64,
    /// 中断原因；只有中断态与恢复过的 Run 有值。
    pub interrupted_reason: Option<String>,
    /// 步骤序列：按 `seq_no` 升序，空数组表示没有检查点。
    pub steps: Vec<RunStep>,
}

/// 一个已提交边界上的步骤：检查点对外可见的那部分。
#[auto_type(default = false, clone = true)]
pub struct RunStep {
    /// 单调序号：同一 Run 内从 1 开始连续递增。
    pub seq_no: i64,
    /// 落在哪一种边界上。
    pub phase: CheckpointPhase,
    /// 轮次序号：起点为 0，其后每轮加一。
    pub tool_round: u32,
    /// 记录产生时间（Unix 毫秒）。
    pub created_at_ms: i64,
}
