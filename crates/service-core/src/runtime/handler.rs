//! 断连处理链：订阅总线通知、串行执行清理（RT-011 ~ RT-013）。
//!
//! 所有断连来源只做两件事：在 [`RuntimeState`] 里写一次标记，再发一条
//! [`ClientDisconnected`] 通知。处理链订阅同一通知，串行执行
//! 「反注册 → 释放占用 → 关闭下行通道」，任一步失败都继续后续步骤并记日志。
//!
//! 顺序不可交换（`README.md` 7.2）：先移除记录，重连的新连接才不会与旧运行时会话抢同一份文件。

use std::sync::OnceLock;

use super::RuntimeState;
use super::session::SessionStop;
use framework_bus::{Bus, Watcher};
use tracing::{info, warn};
use types_core::runtime::{ClientCloseReason, ClientDisconnected};

/// 进程内通知总线；`None` 表示未安装（只操作注册表的用例不需要总线）。
static BUS: OnceLock<Bus> = OnceLock::new();

/// 监听任务的安装位：同一进程只装一次，避免同一条通知被两条链重复处理。
static LISTENING: OnceLock<()> = OnceLock::new();

/// 处理链的串行入口：同一处理函数不会并发执行（`README.md` 第五节并发规则）。
static CHAIN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 订阅断连通知并启动监听任务，返回本次是否真正完成安装。
///
/// 必须在 Tokio 运行时内调用：监听任务与每次清理都由运行时承载。
pub fn install() -> bool {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        warn!("断连处理链需要 Tokio 运行时，跳过安装");
        return false;
    };
    let bus = BUS.get_or_init(Bus::new);
    if LISTENING.set(()).is_err() {
        return false;
    }
    let watcher = bus.watch::<ClientDisconnected>();
    runtime.spawn(listen(watcher));
    true
}

/// 通知入口：断连标记写入后调用；未安装总线时静默。
pub(crate) fn notify() {
    if let Some(bus) = BUS.get() {
        bus.notify::<ClientDisconnected>();
    }
}

/// 处理入口：串行执行一次清理，返回本次处理的客户端数量。
///
/// 通知按“自上次处理以来至少发生过一次”合并，因此取到的标记为空是正常路径，不是错误。
pub async fn process(state: &RuntimeState) -> usize {
    let _guard = CHAIN.lock().await;
    handle(state).await
}

/// 监听断连通知：每次唤醒都完整执行一遍处理链，链内取走全部标记。
async fn listen(mut watcher: Watcher) {
    while watcher.changed().await.is_ok() {
        let Some(state) = super::state() else {
            continue;
        };
        process(state).await;
    }
}

/// 处理链主体：取走全部标记，逐个客户端清理。
async fn handle(state: &RuntimeState) -> usize {
    let marks = state.take_marks();
    for (client_id, reason) in &marks {
        clean(state, client_id, *reason).await;
    }
    marks.len()
}

/// 单个客户端的清理链：通知帧 → 反注册 → 停止运行时会话 → 撤回占用登记。
///
/// 顺序不可交换：`client_closed` 帧必须在反注册前发（反注册会丢弃下行通道），
/// 反注册必须先于停止会话（重连的新连接不会与旧运行时会话抢同一份文件）。
/// 任一步失败都继续后续步骤：清理不能因为单点失败而中断。
async fn clean(state: &RuntimeState, client_id: &str, reason: ClientCloseReason) {
    let notified = state.close_notice(client_id, reason);
    let removed = state.remove(client_id);

    let occupancy = state.occupancy_of_client(client_id);
    if let Some(occupancy) = &occupancy {
        // 与占用接口共用同一把会话锁：停止动作不会与建立 / 释放交叉。
        let lock = state.session_lock(&occupancy.session_id);
        let _guard = lock.lock().await;
        match state
            .stop_session(&occupancy.session_id, SessionStop::ClientDisconnected)
            .await
        {
            Some(Ok(())) => {}
            Some(Err(error)) => warn!(
                %error,
                session_id = %occupancy.session_id,
                "停止运行时会话失败，继续清理"
            ),
            // 占用记录还在但会话已不在：补偿路径，其余清理照做。
            None => warn!(
                session_id = %occupancy.session_id,
                "占用记录存在但运行时会话已不在，继续清理"
            ),
        }
    }
    let released = state.release(client_id);

    match (&removed, &released) {
        (Some(_), occupancy) => info!(
            client_id,
            ?reason,
            notified,
            session_id = occupancy.as_ref().map(|occupancy| occupancy.session_id.as_str()),
            "客户端断连已清理"
        ),
        (None, _) => warn!(client_id, ?reason, "断连清理时客户端已不在注册表"),
    }
}
