//! 进程生命周期：关闭请求的单一事实来源。
//!
//! 关闭请求只在这里记录一次，需要感知关闭的模块（Web 启动、SSE 生产方、请求守门）读同一个
//! 原子标记，不各自监听操作系统信号。真正“停止服务”的动作由 `lib-bin-web` 通过
//! [`set_shutdown_hook`] 注册进来，本模块不依赖具体 Web 框架。
//!
//! 标记是电平语义（一旦置位不再清除）：进程关闭后不回到运行态，P1 不需要重新开放的场景。

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// 是否已请求关闭；未请求时恒为 `false`。
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// 关闭钩子：关闭请求首次生效时调用一次。
static HOOK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
/// 是否已请求关闭。
pub fn is_shutdown() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}

/// 注册关闭钩子：关闭请求首次生效时调用一次。
///
/// 重复注册返回 `false`，保留首次注册的钩子。钩子由触发方所在线程同步调用，
/// 因此实现里要避免阻塞；需要等待或耗时处理时在钩子内自行投递任务。
pub fn set_shutdown_hook(hook: impl Fn() + Send + Sync + 'static) -> bool {
    HOOK.set(Box::new(hook)).is_ok()
}

/// 请求进入关闭流程，返回本次调用是否完成了状态切换。
///
/// 幂等：重复调用返回 `false`，不会重复触发关闭，也不会重复调用钩子。
pub fn request_shutdown() -> bool {
    let first = SHUTDOWN
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok();
    if first && let Some(hook) = HOOK.get() {
        hook();
    }
    first
}
