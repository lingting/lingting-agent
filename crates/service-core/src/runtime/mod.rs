//! 运行时内核：客户端注册表、会话占用与断连标记（P1.1）。
//!
//! 本模块是 `service-core` 里唯一持有“连接期状态”的地方，纯内存、无 Web 依赖：
//! 传输（SSE 帧、响应头、状态码）由 `lib-web` 投影，事件落盘与转发在后续任务接入。
//!
//! 注入方式：`lib-bin-web` 启动时构造 [`RuntimeState`] 并调用 [`install`]，与 P2 的
//! Repository 注入同形；`holder` 的单例替换留到 P2，本阶段只新增这条并行路径。

mod client;
mod registry;

pub use client::{ClientRecord, ClientState};
pub use registry::RuntimeState;

use std::sync::OnceLock;

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
