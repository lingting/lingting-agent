//! 运行时内核：客户端注册表、会话占用、断连标记与心跳扫描（P1.1）。
//!
//! 本模块是 `service-core` 里唯一持有“连接期状态”的地方，纯内存、无 Web 依赖：
//! 传输（SSE 帧、响应头、状态码）由 `lib-web` 投影，事件落盘与转发在后续任务接入；
//! 判定与投递由 [`timer`] 的定时任务按周期驱动。
//!
//! 注入方式：`lib-bin-web` 启动时构造 [`RuntimeState`] 并调用 [`install`]，与 P2 的
//! Repository 注入同形；`holder` 的单例替换留到 P2，本阶段只新增这条并行路径。

mod client;
pub mod handler;
pub mod journal;
mod occupancy;
mod registry;
pub mod session;
mod timer;

pub use client::{CLIENT_TTL_MS, ClientRecord, ClientState, INQUIRY_AFTER_MS, INQUIRY_WINDOW_MS, PendingInquiry};
pub use registry::RuntimeState;
pub use session::{RuntimeSession, SessionStop};
pub use timer::{SCAN_INTERVAL_MS, scan};

use std::fmt;
use std::sync::OnceLock;

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

/// 进程内运行时状态，只允许注入一次。
static STATE: OnceLock<RuntimeState> = OnceLock::new();

/// 注入运行时状态并安装断连处理链；已经注入过时返回 `false`，不覆盖原实例。
///
/// 必须在 Tokio 运行时内调用：处理链的监听任务由运行时承载。
pub fn install(state: RuntimeState) -> bool {
    let installed = STATE.set(state).is_ok();
    if installed && !handler::install() {
        tracing::warn!("断连处理链未安装，断连标记将不会被处理");
    }
    installed
}

/// 取已注入的运行时状态；未注入时为空（当前由 `lib-bin-web` 保证注入）。
pub fn state() -> Option<&'static RuntimeState> {
    STATE.get()
}
