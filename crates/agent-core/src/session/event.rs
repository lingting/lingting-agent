//! Run 事件流契约：SSE 事件的类型与载荷（D-007、D-026）。
//!
//! 事件判别字段固定为 `event`（`docs/develop.md` 的 SSE 约定），取值 snake_case，
//! 同时用作 SSE 的 `event:` 行；`RunEvent` 用扁平结构承载全部事件的载荷，
//! P2 追加事件（如 `tool_call_started`）只增加枚举值，不改已有字段与语义。
//!
//! 注意：`event` 的 wire 值由 `RunEventKind` 上的 `serde(rename_all)` 与
//! `strum(serialize_all)` 共同决定，两处必须同时修改，否则 SSE 的 `event:` 行（走 `Display`）
//! 与 JSON 里的 `event` 字段（走 `Serialize`）会不一致。

use framework_proc_auto::{auto_enum, auto_type};

use super::dto::RunStatus;

/// Run 事件类型。
#[auto_enum]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RunEventKind {
    /// Run 开始执行。
    RunStarted,
    /// 助手消息的增量文本。
    MessageDelta,
    /// Run 正常结束。
    RunCompleted,
    /// Run 执行失败；P1 的 Mock 不产出，留给 P2 的真实执行。
    RunFailed,
    /// Run 被取消：占用释放、客户端断连或服务关闭导致；`error` 带固定原因文本。
    RunCancelled,
    /// 服务关闭：事件流被主动终止，Run 已落成 `CANCELLED`。
    ServerShutdown,
    /// 心跳：保留值。P1.1 起连接保活由客户端心跳（`api/client/update {PING}`）与下行流的 `: keepalive` 注释帧承担，运行时会话不再产出该事件。
    Heartbeat,
    /// 工具调用开始执行（在真正执行之前发出，D-P3-013）。
    ToolCallStarted,
    /// 工具调用结束：结果、状态与耗时都在这一条上。
    ToolCallCompleted,
    /// 工具调用需要客户端确认：发出后 Run 挂起，等 `api/client/post` 回传决定。
    ToolCallConfirmRequired,
}

/// 一次工具调用的状态（事件侧的一份，与审计记录取值一致）。
///
/// 定义在事件契约里而不是复用 `agent-tool` 的类型：`agent-core` 是更底层的领域契约，
/// `agent-tool` 依赖它而不是反过来（D-P3-001）。取值与 `agent-tool` 的 `ToolCallStatus`
/// 一一对应（`SCREAMING_SNAKE_CASE`）；`PENDING` 只出现在挂起等待确认期间，
/// 不会出现在 `tool_call_completed` 事件里（D-P3-023）。
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

/// Run 事件流中的一条事件。
#[auto_type(default = false, clone = true)]
pub struct RunEvent {
    /// 事件类型。
    pub event: RunEventKind,
    /// 事件序号，从 0 开始递增，同时作为 SSE 的 `id`。
    pub seq: i64,
    /// 所属 Run ID。
    pub run_id: String,
    /// 所属会话 ID。
    pub session_id: String,
    /// 事件产生时间（Unix 毫秒）。
    pub created_at_ms: i64,
    /// 增量文本；仅 `message_delta` 有值。
    pub delta: Option<String>,
    /// 助手消息 ID；`run_started` 与 `run_completed` 有值。
    pub message_id: Option<String>,
    /// Run 的最新状态；`run_started` 与 `run_completed` 有值。
    pub status: Option<RunStatus>,
    /// 失败原因；仅 `run_failed` 有值；`tool_call_completed` 失败时复用为工具错误文本。
    pub error: Option<String>,
    /// 输入 token 用量；`run_started` 与 `run_completed` 有值（Mock 按字符数估算）。
    pub input_tokens: Option<i64>,
    /// 输出 token 用量；`run_completed` 有值（Mock 按字符数估算）。
    pub output_tokens: Option<i64>,
    /// 工具调用标识；三个工具事件都有值，客户端据此配对开始与结束（D-P3-013）。
    pub tool_call_id: Option<String>,
    /// 工具名；三个工具事件都有值。
    pub tool_name: Option<String>,
    /// 工具参数：已脱敏、已截断的 JSON 文本；`tool_call_started` 与 `tool_call_confirm_required` 有值。
    pub tool_arguments: Option<String>,
    /// 工具调用状态；仅 `tool_call_completed` 有值。
    pub tool_status: Option<ToolCallStatus>,
    /// 工具调用耗时（毫秒）；仅 `tool_call_completed` 有值。
    pub tool_duration_ms: Option<i64>,
    /// 第几轮工具调用（从 1 开始）；`tool_call_started` 与 `tool_call_confirm_required` 有值。
    pub tool_round: Option<u32>,
}
