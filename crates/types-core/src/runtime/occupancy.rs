//! 会话占用契约：`sessionId ↔ clientId` 的一对一独占关系（D-P1.1-002）。
//!
//! 占用成功即启动该会话的运行时会话并打开事件文件；释放会停止运行时会话，并把该会话
//! `QUEUED` / `RUNNING` 的 Run 落成 `CANCELLED`。一个客户端同时只占用一个会话。

use framework_proc_auto::auto_type;

/// 建立占用的请求体。
#[auto_type]
pub struct OccupancyCreatePO {
    /// 占用方客户端 ID，必须已注册。
    pub client_id: String,
    /// 被占用的会话 ID。
    pub session_id: String,
}

/// 释放占用的请求体：一对一约束下一个客户端最多占用一个会话，因此只需要 `clientId`。
#[auto_type]
pub struct OccupancyDeletePO {
    /// 释放方客户端 ID。
    pub client_id: String,
}

/// 查询占用的查询参数。
#[auto_type]
pub struct OccupancyGetParams {
    /// 目标会话 ID。
    pub session_id: String,
}

/// 当前占用关系。
#[auto_type(clone = true)]
pub struct OccupancyVO {
    /// 被占用的会话 ID。
    pub session_id: String,
    /// 占用方客户端 ID。
    pub client_id: String,
    /// 本次占用对应的运行时会话 ID；每次占用重新生成，也是事件文件名。
    pub runtime_session_id: String,
    /// 占用建立时间（Unix 毫秒）。
    pub acquired_at_ms: i64,
}
