//! 进程级装配结果与 holder。
//!
//! holder 归装配层：业务 Crate 不再持有进程级单例，装配顺序与单例生命周期都由启动方
//! 与本模块负责（D-P2-032）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use agent_route::ProviderService;
use agent_tool::{ToolLimits, ToolRegistry};
use service_core::session::SessionService;
use types_core::config::AgentConfig;

use crate::runtime::{RuntimeState, handler};

/// 一次装配的产物：会话业务服务 + 进程内运行时 + 只读的 Provider / Agent 配置。
///
/// 会话服务与运行时互相引用（运行时用 Service 校验会话与取消 Run，发送消息后要唤醒运行时的执行队列），
/// 因此由装配层持有并集中注入；Provider 配置也从这里交给运行时使用，Runtime 不自己读配置。
pub struct AgentHolder {
    /// 会话业务服务。
    session: Arc<SessionService>,
    /// 运行时状态：客户端注册表、占用表与运行时会话。
    runtime: RuntimeState,
    /// Provider 配置服务：装载结果只读，未配置时照常存在。
    provider: ProviderService,
    /// Agent 运行参数。
    agent: AgentConfig,
    /// 应用根目录：系统提示词（`AGENTS.md`）与 `config.toml` 都在这里。
    home: PathBuf,
    /// 工具注册表：装配期建一次，运行期只读（P3-014）。
    tools: Arc<ToolRegistry>,
    /// 工具执行参数（`[tools]` 的超时、截断与并发上限）。
    tool_limits: ToolLimits,
}

impl AgentHolder {
    /// 由装配流程构造：各部件必须来自同一次装配。
    pub(crate) fn new(
        session: Arc<SessionService>,
        runtime: RuntimeState,
        provider: ProviderService,
        agent: AgentConfig,
        home: PathBuf,
        tools: Arc<ToolRegistry>,
        tool_limits: ToolLimits,
    ) -> Self {
        Self {
            session,
            runtime,
            provider,
            agent,
            home,
            tools,
            tool_limits,
        }
    }

    /// 会话业务服务。
    pub fn session(&self) -> &Arc<SessionService> {
        &self.session
    }

    /// 运行时状态。
    pub fn runtime(&self) -> &RuntimeState {
        &self.runtime
    }

    /// Provider 配置服务：`configured()` 为假表示 `[provider]` 段缺省。
    pub fn provider(&self) -> &ProviderService {
        &self.provider
    }

    /// Agent 运行参数（并发、超时与 Context 预算）。
    pub fn agent(&self) -> &AgentConfig {
        &self.agent
    }

    /// 应用根目录：执行方用它找系统提示词（`AGENTS.md`）。
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// 工具注册表：执行方据此产出模型可见清单与执行期查找（P3-014）。
    pub fn tools(&self) -> &Arc<ToolRegistry> {
        &self.tools
    }

    /// 工具执行参数：超时、结果截断与一轮内的并发上限。
    pub fn tool_limits(&self) -> ToolLimits {
        self.tool_limits
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
