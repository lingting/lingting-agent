//! 断连处理链：订阅总线通知、串行执行清理（RT-011 ~ RT-013、RT-017）。
//!
//! 所有断连来源只做两件事：在 [`RuntimeState`] 里写一次标记，再发一条
//! [`ClientDisconnected`] 通知。处理链订阅同一通知，串行执行
//! 「反注册 → 释放占用 → 关闭下行通道」，任一步失败都继续后续步骤并记日志。
//!
//! 顺序不可交换（`README.md` 7.2）：先移除记录，重连的新连接才不会与旧运行时会话抢同一份文件。
//! 串行单位是**客户端**（P2.1-005）：同一客户端的清理不会并发执行，不同客户端的清理互不排队。
//! 服务关闭走同一条链（[`shutdown`]）：先给全部客户端打 `ServerShutdown` 标记，再停掉残留运行时会话。

use std::collections::HashMap;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::{Arc, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::task::Poll;

use super::RuntimeState;
use super::metrics;
use super::session::SessionStop;
use framework_bus::{Bus, Watcher};
use tracing::{info, warn};
use types_core::runtime::{ClientCloseReason, ClientDisconnected};

/// 进程内通知总线；`None` 表示未安装（只操作注册表的用例不需要总线）。
static BUS: OnceLock<Bus> = OnceLock::new();

/// 监听任务的安装位：同一进程只装一次，避免同一条通知被两条链重复处理。
static LISTENING: OnceLock<()> = OnceLock::new();

/// 处理链的串行单位：每个客户端一把锁，同一客户端不并发清理，不同客户端可并行（P2.1-005）。
///
/// 条目在每次处理链入口回收（无人持有时移除），不随历史客户端单调增长。
static CHAINS: OnceLock<RwLock<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
/// 订阅断连通知并启动监听任务，返回本次是否真正完成安装。
///
/// 必须在 Tokio 运行时内调用：监听任务与每次清理都由运行时承载。
pub fn install(state: &'static RuntimeState) -> bool {
    // 定时任务的心跳扫描与事件轮转要从同一处取注册表：先记账，再装监听任务。
    let _ = super::bind(state);
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        warn!("断连处理链需要 Tokio 运行时，跳过安装");
        return false;
    };
    let bus = BUS.get_or_init(Bus::new);
    if LISTENING.set(()).is_err() {
        return false;
    }
    let watcher = bus.watch::<ClientDisconnected>();
    runtime.spawn(listen(watcher, state));
    true
}

/// 通知入口：断连标记写入后调用；未安装总线时静默。
pub(crate) fn notify() {
    if let Some(bus) = BUS.get() {
        bus.notify::<ClientDisconnected>();
    }
}

/// 处理入口：取走全部标记，按客户端并行清理，返回本次处理的客户端数量。
///
/// 并发语义（P2.1-005）：不同客户端的清理同时进行，同一客户端由自己那把锁串行化。
/// 通知按“自上次处理以来至少发生过一次”合并，因此取到的标记为空是正常路径，不是错误。
pub async fn process(state: &RuntimeState) -> usize {
    prune_chains();
    let marks = state.take_marks();
    let handled = marks.len();
    let mut pending: Vec<Pin<Box<dyn Future<Output = ()> + Send + '_>>> = marks
        .into_iter()
        .map(|(client_id, reason)| -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            Box::pin(clean_serialized(state, client_id, reason))
        })
        .collect();
    // 手写 join：不为此引入 future 组合库，逐个轮询到全部就绪。
    poll_fn(|context| {
        pending.retain_mut(|future| future.as_mut().poll(context).is_pending());
        if pending.is_empty() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    metrics::record_chain_clients(handled);
    handled
}

/// 取客户端对应的串行锁，没有就插入一把。
fn client_chain(client_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    if let Some(chain) = read_chains().get(client_id) {
        return Arc::clone(chain);
    }
    let mut chains = write_chains();
    Arc::clone(chains.entry(client_id.to_string()).or_default())
}

/// 回收无人持有的客户端锁：只在处理链入口调用，避免锁表随历史客户端单调增长。
fn prune_chains() {
    write_chains().retain(|_, chain| Arc::strong_count(chain) > 1);
}

/// 取客户端锁表的读锁；锁中毒时沿用内部数据。
fn read_chains() -> RwLockReadGuard<'static, HashMap<String, Arc<tokio::sync::Mutex<()>>>> {
    CHAINS
        .get_or_init(RwLock::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
}

/// 取客户端锁表的写锁，中毒处理同 [`read_chains`]。
fn write_chains() -> RwLockWriteGuard<'static, HashMap<String, Arc<tokio::sync::Mutex<()>>>> {
    CHAINS
        .get_or_init(RwLock::default)
        .write()
        .unwrap_or_else(|error| error.into_inner())
}

/// 监听断连通知：每次唤醒都完整执行一遍处理链，链内取走全部标记。
async fn listen(mut watcher: Watcher, state: &'static RuntimeState) {
    while watcher.changed().await.is_ok() {
        process(state).await;
    }
}

/// 单个客户端的串行清理：先拿该客户端自己的锁，再执行清理链。
async fn clean_serialized(state: &RuntimeState, client_id: String, reason: ClientCloseReason) {
    let chain = client_chain(&client_id);
    let _guard = chain.lock().await;
    let _span = metrics::chain_enter();
    clean(state, &client_id, reason).await;
}

/// 关闭流程：给全部客户端打 `ServerShutdown` 标记，复用断连链收尾，返回处理的客户端数量。
///
/// 关闭导致的会话终止一律落 `server_shutdown`（[`stop_reason`]）；链跑完后把剩余的运行时会话
/// 一并摘下并 flush，关闭后不允许留下未收尾的事件文件。
pub async fn shutdown(state: &RuntimeState) -> usize {
    for record in state.clients() {
        state.mark_shutdown(&record.client_id);
    }
    let handled = process(state).await;

    // 补偿路径：占用记录缺失或链没覆盖到的运行时会话同样要收尾（会话已摘下，无需会话锁）。
    for session in state.take_sessions() {
        let session_id = session.session_id().to_string();
        if let Err(error) = session.stop(SessionStop::ServerShutdown).await {
            warn!(session_id, %error, "关闭时停止运行时会话失败，继续收尾");
        }
    }
    // 关闭是唯一必须等收尾的路径（P2.1-006）：链上摘下的会话在后台收尾，这里等它们全部完成。
    state.drain_finalizations().await;
    handled
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
        // 与占用接口共用同一把会话锁：只做「摘下运行时会话 + 交给后台收尾」这一步，
        // 收尾本身在锁外进行，断连清理不会被收尾时长拖住（P2.1-006）。
        {
            let _guard = state.lock_session(&occupancy.session_id).await;
            if !state.stop_session_in_background(&occupancy.session_id, stop_reason(reason)) {
                // 占用记录还在但会话已不在：补偿路径，其余清理照做。
                warn!(
                    session_id = %occupancy.session_id,
                    "占用记录存在但运行时会话已不在，继续清理"
                );
            }
        }
        // 锁外回收：没有持有者了就摘掉这把锁（P2.1-007）。
        state.reclaim_session_lock(&occupancy.session_id);
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

/// 标记原因决定会话终止语义：服务关闭落 `server_shutdown`，其余断连落 `run_cancelled`（RT-003）。
fn stop_reason(reason: ClientCloseReason) -> SessionStop {
    match reason {
        ClientCloseReason::ServerShutdown => SessionStop::ServerShutdown,
        _ => SessionStop::ClientDisconnected,
    }
}
