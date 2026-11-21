//! 运行时内核：客户端注册表、会话占用、断连标记与心跳扫描（P1.1）。
//!
//! 进程内运行时：客户端注册表、会话占用、事件文件与断连处理链（P1.1）。
//!
//! 本模块持有“连接期状态”（纯内存、无 Web 依赖）：传输（SSE 帧、响应头、状态码）由
//! `lib-web` 投影；事件落盘与转发的写者在 [`session`]；心跳与轮转由 [`timer`] 驱动。
//!
//! 归属（D-P2-035）：运行时与「会话业务」同属装配层 `manager-agent`，因为两者互相引用
//! （占用校验要读会话、会话停止要取消 Run）。`service-core` 只提供业务与数据访问。
//!
//! 注入方式：装配层在启动时构造 [`RuntimeState`] 并调用 [`handler::install`]，
//! 定时任务通过同一处登记的 `'static` 引用取注册表。

mod client;
pub mod handler;
pub mod journal;
/// 运行时的进程内指标：清理链、会话停止、事件落盘与锁表。
pub mod metrics;
mod occupancy;
mod registry;
pub mod session;
mod timer;

pub use client::{CLIENT_TTL_MS, ClientRecord, ClientState, INQUIRY_AFTER_MS, INQUIRY_WINDOW_MS, PendingInquiry};
pub use journal::{JournalConfig, RotationSummary};
pub use registry::RuntimeState;
pub use session::{RuntimeSession, SessionStop};
pub use timer::{ROTATION_INTERVAL_MS, SCAN_INTERVAL_MS, scan};

use std::fmt;

/// 运行时业务的语义化错误。
///
/// 本 crate 只给语义，不携带 HTTP 细节：状态码与固定 message 由 `lib-web/src/runtime` 的一个函数
/// 统一映射（D-P1.1-005），框架支持自定义 code 后只改那一处。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// 客户端未注册或已被反注册：客户端据此重新注册。
    ClientNotRegistered,
    /// 会话已被其他客户端占用。
    SessionOccupied,
    /// 该客户端已占用其他会话：必须先 `occupancy/delete` 释放后再占用（不做隐式切换）。
    ClientBusy,
    /// 会话不存在。
    SessionNotFound(String),
    /// 会话当前无人占用。
    NotOccupied,
    /// 内部失败（建目录 / 开事件文件 / 起运行时会话）：对外只给固定文案，细节只进日志。
    Internal(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClientNotRegistered => formatter.write_str("客户端未注册"),
            Self::SessionOccupied => formatter.write_str("会话已被其他客户端占用"),
            Self::ClientBusy => formatter.write_str("客户端已占用其他会话"),
            Self::SessionNotFound(session_id) => write!(formatter, "会话不存在：{session_id}"),
            Self::NotOccupied => formatter.write_str("会话未被占用"),
            Self::Internal(detail) => write!(formatter, "运行时内部错误：{detail}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// 装配层注入的运行时状态指针：本模块的定时任务要靠它取注册表。
///
/// 值的所有权在装配层（`manager-agent` 的 holder），这里只保存一个 `'static` 引用；
/// 进程内只设置一次，重复注入返回 `false`。
static INSTALLED: std::sync::OnceLock<&'static RuntimeState> = std::sync::OnceLock::new();

/// 记录装配层注入的运行时状态；已注入过时返回 `false`。
pub(crate) fn bind(state: &'static RuntimeState) -> bool {
    INSTALLED.set(state).is_ok()
}

/// 取已注入的运行时状态；未注入时为空（定时任务据此跳过）。
pub(crate) fn bound() -> Option<&'static RuntimeState> {
    INSTALLED.get().copied()
}
