//! 客户端连接契约：注册、上行消息、主动断开与下行事件（D-P1.1-001 / D-P3-022）。
//!
//! 传输形态固定为「SSE 下行 + POST 上行」：注册、上行消息、主动断开全部走 `POST`，
//! 服务端推送只有 `api/client/stream` 一条通道。本模块只描述入参与载荷，不涉及帧编码。
//!
//! 上行只有一条通道 `api/client/post`，按 `kind` 承载三种无状态动作：心跳、问询应答、
//! 工具确认（D-P3-022）。`pong` 只出现在 `client/create` 与 `client/post` 的同步响应体里，
//! 不进入下行事件；下行事件的判别字段为 `event`，取值 snake_case，与 SSE 的 `event:` 行同值。

use framework_proc_auto::{auto_enum, auto_type};

use agent_core::session::RunEvent;

/// 注册的请求体：注册动作本身不携带字段，`clientId` 由服务端生成。
#[auto_type]
pub struct ClientCreatePO {}

/// 注册的响应：响应里的首个 pong 即注册成功信号。
#[auto_type]
pub struct ClientCreateVO {
    /// 客户端 ID，由服务端在注册时生成。
    pub client_id: String,
    /// 首个 pong 的产生时间（Unix 毫秒）。
    pub pong_at_ms: i64,
}

/// 上行请求的种类。
#[auto_enum]
pub enum ClientPostKind {
    /// 心跳：刷新 `lastSeen`，响应体里回 pong。
    Ping,
    /// 问询应答：清掉待答状态；按约定不刷新 `lastSeen`，否则超时判定会失效。
    InquiryAck,
    /// 工具确认：对一次挂起的工具调用给出批准或驳回（D-P3-023）。
    ToolConfirm,
}

/// 工具确认的决定：只有两个取值，没有「总是允许」（D-P3-026）。
#[auto_enum]
pub enum ClientDecision {
    /// 批准：恢复后执行这次调用。
    Approve,
    /// 驳回：恢复后不再执行，把固定文案回灌给模型。
    Reject,
}

/// 上行请求的请求体：心跳、问询应答与工具确认共用，按 `kind` 决定哪些字段有效（D-P3-022）。
#[auto_type(default = false)]
pub struct ClientPostPO {
    /// 目标客户端 ID。
    pub client_id: String,
    /// 上行请求的种类。
    pub kind: ClientPostKind,
    /// 待确认的调用标识：只有 `TOOL_CONFIRM` 允许非空。
    pub tool_call_id: Option<String>,
    /// 对挂起调用的决定：只有 `TOOL_CONFIRM` 允许非空。
    pub decision: Option<ClientDecision>,
}

/// 上行请求的响应：同步返回 pong，不进入下行事件。
///
/// 工具确认同样刷新 `lastSeen` 并回 pong（一次确认同时被当作心跳，避免确认期间心跳被误判超时）。
#[auto_type]
pub struct ClientPostVO {
    /// 本次 pong 的产生时间（Unix 毫秒）。
    pub pong_at_ms: i64,
}

/// 主动断开的请求体：幂等，已移除的客户端同样返回成功。
#[auto_type]
pub struct ClientDeletePO {
    /// 目标客户端 ID。
    pub client_id: String,
}

/// 下行事件流的查询参数。
///
/// 用 GET 而非 POST：浏览器 `EventSource` 只支持 GET。
#[auto_type]
pub struct ClientStreamParams {
    /// 目标客户端 ID。
    pub client_id: String,
}

/// 下行事件类型。
///
/// wire 值由 `serde(rename_all)` 与 `strum(serialize_all)` 共同决定，两处必须同时修改，
/// 否则 SSE 的 `event:` 行（走 `Display`）与 JSON 里的 `event` 字段（走 `Serialize`）会不一致。
#[auto_enum]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ClientEventKind {
    /// 问询：服务端要求客户端在应答窗口内应答，无应答视为断连。
    Inquiry,
    /// 运行时会话产出的一条 `RunEvent`，载荷是整条事件。
    SessionEvent,
    /// 下行通道即将关闭，载荷是关闭原因；流已断时这一帧发不出去。
    ClientClosed,
}

/// 断连原因：六条来源收敛成同一组取值，重复触发只保留第一次的原因。
#[auto_enum]
pub enum ClientCloseReason {
    /// 心跳超时：`lastSeen` 超过 TTL 未刷新。
    HeartbeatTimeout,
    /// 问询无应答：应答窗口内没有收到 `INQUIRY_ACK`。
    InquiryTimeout,
    /// 客户端主动断开：`api/client/delete`。
    ClientClosed,
    /// 下行流断开：响应体被 drop，或向下行通道发送失败。
    StreamClosed,
    /// 服务关闭。
    ServerShutdown,
    /// 事件文件写失败。
    JournalError,
}

/// 下行事件流中的一条事件。
#[auto_type(default = false, clone = true)]
pub struct ClientEvent {
    /// 事件类型。
    pub event: ClientEventKind,
    /// 客户端内的事件序号，从 0 开始递增，同时作为 SSE 的 `id`；与 `RunEvent.seq` 不同源。
    pub seq: i64,
    /// 事件产生时间（Unix 毫秒）。
    pub created_at_ms: i64,
    /// `session_event` 的载荷；其余事件类型为空。
    pub session_event: Option<RunEvent>,
    /// `inquiry` 的投递时刻（Unix 毫秒）。
    pub inquiry_at_ms: Option<i64>,
    /// `inquiry` 的应答截止时刻（Unix 毫秒）。
    pub inquiry_deadline_ms: Option<i64>,
    /// `client_closed` 的关闭原因。
    pub close_reason: Option<ClientCloseReason>,
}

/// 客户端断连通知（RT-011）：纯通知、不携带任何数据。
///
/// 六条断连来源都只在注册表里写一次标记，再用它唤醒处理链；原因存在注册表里，因此
/// 处理函数必须幂等，通知被合并触发也只处理一次（take 语义）。
pub struct ClientDisconnected;
