//! 进程级装配结果与 holder。
//!
//! holder 归装配层：业务 Crate 不再持有进程级单例，装配顺序与单例生命周期都由启动方
//! 与本模块负责（D-P2-032）。

use std::sync::{Arc, OnceLock};

use service_core::session::SessionService;

use crate::runtime::{RuntimeState, handler};

/// 一次装配的产物：会话业务服务 + 进程内运行时。
///
/// 两者互相引用（运行时用 Service 校验会话与取消 Run，发送消息后要唤醒运行时的执行队列），
/// 因此由装配层持有并集中注入。
pub struct AgentHolder {
    /// 会话业务服务。
    session: Arc<SessionService>,
    /// 运行时状态：客户端注册表、占用表与运行时会话。
    runtime: RuntimeState,
}

impl AgentHolder {
    /// 由装配流程构造：`session` 与 `runtime` 必须来自同一次装配。
    pub(crate) fn new(session: Arc<SessionService>, runtime: RuntimeState) -> Self {
        Self { session, runtime }
    }

    /// 会话业务服务。
    pub fn session(&self) -> &Arc<SessionService> {
        &self.session
    }

    /// 运行时状态。
    pub fn runtime(&self) -> &RuntimeState {
        &self.runtime
    }
}

/// 进程内装配结果，只允许注入一次。
static HOLDER: OnceLock<AgentHolder> = OnceLock::new();

/// 注入装配结果并安装断连处理链；已注入过时返回 `false`，不覆盖原实例。
///
/// 必须在 Tokio 运行时内调用：处理链的监听任务由运行时承载。
pub fn install(holder: AgentHolder) -> bool {
    if HOLDER.set(holder).is_err() {
        return false;
    }
    let Some(installed) = HOLDER.get() else {
        return false;
    };
    if !handler::install(installed.runtime()) {
        tracing::warn!("断连处理链未安装，断连标记将不会被处理");
    }
    true
}

/// 取已注入的装配结果；未注入时为空（启动装配不完整）。
pub fn holder() -> Option<&'static AgentHolder> {
    HOLDER.get()
}

/// 取已注入的运行时状态；未注入时为空。
pub fn runtime() -> Option<&'static RuntimeState> {
    holder().map(AgentHolder::runtime)
}

/// 关闭流程入口：把「停全部客户端与运行时会话」交给断连处理链，返回处理的客户端数量。
///
/// 未注入时（只跑用例的进程）直接返回 0：没有东西需要收尾。
pub async fn shutdown() -> usize {
    match runtime() {
        Some(runtime) => handler::shutdown(runtime).await,
        None => 0,
    }
}
