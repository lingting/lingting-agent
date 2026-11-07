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
    /// 心跳：长时间没有事件时保持连接不被中间层掐断。
    Heartbeat,
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
    /// 失败原因；仅 `run_failed` 有值，且不含内部细节。
    pub error: Option<String>,
    /// 输入 token 用量；`run_started` 与 `run_completed` 有值（Mock 按字符数估算）。
    pub input_tokens: Option<i64>,
    /// 输出 token 用量；`run_completed` 有值（Mock 按字符数估算）。
    pub output_tokens: Option<i64>,
}
