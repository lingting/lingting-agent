//! 执行端口的实现载体：把依赖装进一个结构体，`drive` 转调单轮 Agent Loop。
//!
//! 一个进程一个实例，由启动方 `lib-bin-web` 构造后注入（D-P2-028 第 3 条）；注入前
//! （`api_probe_server` 与部分进程内用例）执行队列退化为「Run 直接落 `FAILED`」。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_context::ContextBuilder;
use agent_core::session::Run;
use agent_route::rig::OpenAiProvider;
use agent_route::{LlmProvider, ProviderService};
use agent_tool::{ToolExecutor, ToolLimits, ToolRegistry};
use async_trait::async_trait;
use service_core::session::{ResumePlan, RunDriver, RunInput, RunOutcome, SessionService};
use tokio::sync::Semaphore;
use types_core::config::AgentConfig;

use crate::loop_;

/// 执行端口实现：会话业务 + Provider 配置 + Context 组装 + 工具执行 + 全局并发上限。
pub struct AgentRuntime {
    /// 会话业务服务：读历史、落终态都经它（`service-core` 的 Repository 不对外）。
    pub(crate) service: Arc<SessionService>,
    /// Provider 配置：模型查找与预算来源。
    pub(crate) provider: ProviderService,
    /// Provider 适配器；`config.toml` 没有 `[provider]` 段时为 `None`，调用模型时才失败。
    pub(crate) llm: Option<Arc<dyn LlmProvider>>,
    /// Context 组装器：由装配层构造（`AGENTS.md` + Skill 索引，P5-007 / D-P5-016）。
    pub(crate) builder: ContextBuilder,
    /// `[agent]` 运行参数：Context 预算、排队超时、轮次上限与模型流空闲超时。
    pub(crate) agent: AgentConfig,
    /// 全局并发上限：许可随 Run 结束释放（P2-025）。
    pub(crate) limit: Arc<Semaphore>,
    /// 工具注册表：模型可见清单与执行期查找的唯一入口（P3-004）。
    pub(crate) tools: Arc<ToolRegistry>,
    /// 工具执行器：校验 → 权限 → 超时 → 取消 → 归一化（P3-006 / P3-007）。
    pub(crate) executor: ToolExecutor,
}

impl AgentRuntime {
    /// 按装配结果构造：`home` 是应用根目录（执行器的路径基准）。
    ///
    /// Provider 适配器在这里一次性构造：共享 reqwest 连接池，空闲超时取
    /// `agent.llm_idle_timeout_ms`（P2-016）。未配置 Provider 时留空，`drive` 再按
    /// `LlmError::Unconfigured` 的文案落 `FAILED`。
    ///
    /// 工具注册表与执行参数同样来自装配（P3-014）：注册表在装配期建一次，本函数只读它，
    /// 并用同一个 `home` 与 `limits` 构造执行器（工具的路径白名单基准与截断上限都在这里）。
    ///
    /// `builder` 由装配层造好给（P5-010 / D-P5-016）：系统提示词与 Skill 索引怎么合成是 Context
    /// 的事，运行时既不认识 Skill，也不再自己 `new` 一个 `ContextBuilder`。
    pub fn new(
        service: Arc<SessionService>,
        provider: ProviderService,
        agent: AgentConfig,
        home: impl Into<PathBuf>,
        tools: Arc<ToolRegistry>,
        limits: ToolLimits,
        builder: ContextBuilder,
    ) -> Self {
        let home = home.into();
        let idle_timeout = Some(Duration::from_millis(agent.llm_idle_timeout_ms));
        let llm: Option<Arc<dyn LlmProvider>> = provider
            .provider()
            .map(|config| Arc::new(OpenAiProvider::new(config, idle_timeout)) as Arc<dyn LlmProvider>);
        // 许可数按配置解析：核数只在装配时取一次，运行期不变（P2.1-015）；结果至少为 1，
        // 配置成 0 会让所有 Run 都在排队里超时，属配置错误但不该卡死服务。
        let cores = std::thread::available_parallelism().map_or(1, |cores| cores.get());
        let permits = agent.max_concurrent_runs.resolve(cores);
        let limit = Arc::new(Semaphore::new(permits));
        let executor = ToolExecutor::new(Arc::clone(&tools), limits, home.clone());
        Self {
            service,
            provider,
            llm,
            builder,
            agent,
            limit,
            tools,
            executor,
        }
    }
}

#[async_trait]
impl RunDriver for AgentRuntime {
    async fn drive(&self, input: RunInput) -> RunOutcome {
        loop_::drive(self, input).await
    }

    /// 恢复判定：检查点一致性校验在 `loop_::checkpoint`，这里只把服务依赖装进去。
    async fn plan_resume(&self, run: &Run) -> Result<ResumePlan, String> {
        loop_::checkpoint::plan(&self.service, run).await
    }
}
