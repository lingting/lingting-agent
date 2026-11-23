//! 执行端口的实现载体：把依赖装进一个结构体，`drive` 转调单轮 Agent Loop。
//!
//! 一个进程一个实例，由启动方 `lib-bin-web` 构造后注入（D-P2-028 第 3 条）；注入前
//! （`api_probe_server` 与部分进程内用例）执行队列退化为「Run 直接落 `FAILED`」。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_context::ContextBuilder;
use agent_route::rig::OpenAiProvider;
use agent_route::{LlmProvider, ProviderService};
use async_trait::async_trait;
use service_core::session::{RunDriver, RunInput, RunOutcome, SessionService};
use tokio::sync::Semaphore;
use types_core::config::AgentConfig;

use crate::loop_;

/// 执行端口实现：会话业务 + Provider 配置 + Context 组装 + 全局并发上限。
pub struct AgentRuntime {
    /// 会话业务服务：读历史、落终态都经它（`service-core` 的 Repository 不对外）。
    pub(crate) service: Arc<SessionService>,
    /// Provider 配置：模型查找与预算来源。
    pub(crate) provider: ProviderService,
    /// Provider 适配器；`config.toml` 没有 `[provider]` 段时为 `None`，调用模型时才失败。
    pub(crate) llm: Option<Arc<dyn LlmProvider>>,
    /// Context 组装器：系统提示词来自 `home` 下的 `AGENTS.md`。
    pub(crate) builder: ContextBuilder,
    /// `[agent]` 运行参数：Context 预算、排队超时与模型流空闲超时。
    pub(crate) agent: AgentConfig,
    /// 全局并发上限：许可随 Run 结束释放（P2-025）。
    pub(crate) limit: Arc<Semaphore>,
}

impl AgentRuntime {
    /// 按装配结果构造：`home` 是应用根目录（系统提示词的扫描目录）。
    ///
    /// Provider 适配器在这里一次性构造：共享 reqwest 连接池，空闲超时取
    /// `agent.llm_idle_timeout_ms`（P2-016）。未配置 Provider 时留空，`drive` 再按
    /// `LlmError::Unconfigured` 的文案落 `FAILED`。
    pub fn new(
        service: Arc<SessionService>,
        provider: ProviderService,
        agent: AgentConfig,
        home: impl Into<PathBuf>,
    ) -> Self {
        let idle_timeout = Some(Duration::from_millis(agent.llm_idle_timeout_ms));
        let llm: Option<Arc<dyn LlmProvider>> = provider
            .provider()
            .map(|config| Arc::new(OpenAiProvider::new(config, idle_timeout)) as Arc<dyn LlmProvider>);
        // 许可数按配置解析：核数只在装配时取一次，运行期不变（P2.1-015）；结果至少为 1，
        // 配置成 0 会让所有 Run 都在排队里超时，属配置错误但不该卡死服务。
        let cores = std::thread::available_parallelism().map_or(1, |cores| cores.get());
        let permits = agent.max_concurrent_runs.resolve(cores);
        let limit = Arc::new(Semaphore::new(permits));
        Self {
            service,
            provider,
            llm,
            builder: ContextBuilder::new(home),
            agent,
            limit,
        }
    }
}

#[async_trait]
impl RunDriver for AgentRuntime {
    async fn drive(&self, input: RunInput) -> RunOutcome {
        loop_::drive(self, input).await
    }
}
