//! 运行时内核：客户端注册表、会话占用、断连标记与心跳扫描（P1.1）。
//!
//! 本模块是 `service-core` 里唯一持有“连接期状态”的地方，纯内存、无 Web 依赖：
//! 传输（SSE 帧、响应头、状态码）由 `lib-web` 投影，事件落盘与转发在后续任务接入；
//! 判定与投递由 [`timer`] 的定时任务按周期驱动。
//!
//! 注入方式：`lib-bin-web` 启动时构造 [`RuntimeState`] 并调用 [`install`]，与 P2 的
//! Repository 注入同形；`holder` 的单例替换留到 P2，本阶段只新增这条并行路径。

mod client;
mod registry;
mod timer;

pub use client::{CLIENT_TTL_MS, ClientRecord, ClientState, INQUIRY_AFTER_MS, INQUIRY_WINDOW_MS, PendingInquiry};
pub use registry::RuntimeState;
pub use timer::{SCAN_INTERVAL_MS, scan};

use std::fmt;
use std::sync::OnceLock;

/// 运行时业务的语义化错误。
///
/// 本 crate 只给语义，不携带 HTTP 细节：状态码与固定 message 由 `lib-web/src/runtime` 的一个函数
/// 统一映射（D-P1.1-005），框架支持自定义 code 后只改那一处。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeError {
    /// 客户端未注册或已被反注册：客户端据此重新注册。
    ClientNotRegistered,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClientNotRegistered => formatter.write_str("客户端未注册"),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// 进程内运行时状态，只允许注入一次。
static STATE: OnceLock<RuntimeState> = OnceLock::new();

/// 注入运行时状态；已经注入过时返回 `false`，不覆盖原实例。
pub fn install(state: RuntimeState) -> bool {
    STATE.set(state).is_ok()
}

/// 取已注入的运行时状态；未注入时为空（当前由 `lib-bin-web` 保证注入）。
pub fn state() -> Option<&'static RuntimeState> {
    STATE.get()
}
