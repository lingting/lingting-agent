//! `McpClient`：把一个 server 的连接、握手与状态机收敛在一个类型里（README §4.3）。
//!
//! 两种 transport 共用本模块的全部逻辑：握手、状态迁移、完成监听、关闭与调用路径只有一份，
//! 差异留在 [`crate::transport`] 的构造函数分支（P4-006 的验收点）。
//!
//! 状态迁移的触发点只有四个：
//!
//! - [`McpClient::connect`] 开始 → `Connecting`；
//! - `serve` 返回 → `Ready`；
//! - 握手失败 / 超时 → `Failed`；
//! - 后台的完成监听结束（读循环结束 = 崩溃或断开）→ `Failed`；[`McpClient::close`] → `Stopped`。
//!
//! 「读循环结束」由 [`rmcp::service::RunningService::waiting`] 驱动：没有这一步就只能在下次
//! 调用时才发现断了——那会表现为调用超时，而不是可读的错误。

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use agent_tool::ToolOutput;
use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation};
use rmcp::service::{NotificationContext, Peer, RoleClient, RunningService};
use rmcp::transport::IntoTransport;
use rmcp::{ClientHandler, ServiceExt};
use serde_json::Value;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::{McpServerSpec, McpTransport};
use crate::discover::{self, McpToolSpec, RawTool};
use crate::error::McpError;
use crate::state::McpState;
use crate::transport;

/// initialize 时携带的客户端名。
pub const CLIENT_NAME: &str = "lingting-agent";

/// 关闭一个连接的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseOutcome {
    /// 优雅关闭是否在超时内完成。
    pub graceful: bool,
    /// 是否因为超时而强制终止（记入 `shutdown_kills` 指标）。
    pub killed: bool,
}

/// 一次连接运行期的句柄：调用走 `peer`，关闭走 `cancel` + `monitor`。
struct Link {
    /// 克隆出来的 peer：调用路径不持有任何锁。
    peer: Peer<RoleClient>,
    /// `serve` 用的取消令牌：取消即让服务循环退出并按顺序关掉 transport。
    cancel: CancellationToken,
    /// 持有 `RunningService` 的监听任务：结束时改状态，关闭时被等待。
    monitor: JoinHandle<()>,
}

/// 一个 MCP server 的客户端。
pub struct McpClient {
    spec: McpServerSpec,
    state: Mutex<McpState>,
    last_error: Mutex<Option<String>>,
    tools: Mutex<Vec<McpToolSpec>>,
    link: Mutex<Option<Link>>,
    tool_changed: Arc<AtomicU64>,
    /// 连接代次：旧连接的完成监听不能覆盖新连接的状态。
    generation: AtomicU64,
}

impl McpClient {
    /// 由连接配置建客户端：初始状态为 [`McpState::Disabled`]（是否连接由装配层决定）。
    pub fn new(spec: McpServerSpec) -> Self {
        Self {
            spec,
            state: Mutex::new(McpState::Disabled),
            last_error: Mutex::new(None),
            tools: Mutex::new(Vec::new()),
            link: Mutex::new(None),
            tool_changed: Arc::new(AtomicU64::new(0)),
            generation: AtomicU64::new(0),
        }
    }

    /// 连接配置。
    pub fn spec(&self) -> &McpServerSpec {
        &self.spec
    }

    /// server 标识。
    pub fn id(&self) -> &str {
        &self.spec.id
    }

    /// 当前状态（对外只读）。
    pub fn state(&self) -> McpState {
        self.lock(&self.state).clone()
    }

    /// 最近一次失败的可读原因；成功发现后清空。
    pub fn last_error(&self) -> Option<String> {
        self.lock(&self.last_error).clone()
    }

    /// 上一次成功发现的工具快照（失败时保留旧快照）。
    pub fn tools(&self) -> Vec<McpToolSpec> {
        self.lock(&self.tools).clone()
    }

    /// 取走「`tools/list_changed` 已到过」的标记（manager 据此触发重新发现）。
    pub fn take_tool_list_changed(&self) -> bool {
        self.tool_changed.swap(0, Ordering::Relaxed) > 0
    }

    /// 建立连接并完成握手：成功置 `Ready`，失败置 `Failed`。
    ///
    /// 已 `Ready` 时直接返回（幂等）；`connect_timeout_ms` 是「建 transport + initialize」的总上限，
    /// 超时会先取消刚启动的子进程再置 `Failed`。
    pub async fn connect(self: &Arc<Self>) -> Result<(), McpError> {
        if self.state().is_ready() {
            return Ok(());
        }
        self.set_state(McpState::Connecting);
        let cancel = CancellationToken::new();
        let handler = LingtingClientHandler {
            tool_changed: Arc::clone(&self.tool_changed),
        };
        // 差异只在「怎么建 transport」：stdio 会在这里起动子进程。
        let result = match &self.spec.transport {
            McpTransport::Stdio { .. } => match transport::stdio(&self.spec) {
                Ok(transport) => handshake(&self.spec, handler, transport, &cancel).await,
                Err(error) => Err(error),
            },
            McpTransport::Http { .. } => match transport::http(&self.spec) {
                Ok(transport) => handshake(&self.spec, handler, transport, &cancel).await,
                Err(error) => Err(error),
            },
        };
        let service = match result {
            Ok(service) => service,
            Err(error) => {
                self.fail(&error);
                return Err(error);
            }
        };

        let peer = service.peer().clone();
        let client = Arc::clone(self);
        // 读循环结束（崩溃 / 断开）必须立刻改状态：否则「Ready 但连接已断」的窗口会一直存在。
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let monitor = tokio::spawn(async move {
            let _ = service.waiting().await;
            client.on_service_end(generation);
        });
        *self.lock(&self.link) = Some(Link { peer, cancel, monitor });
        self.set_state(McpState::Ready);
        self.set_last_error(None);
        tracing::debug!(server = %self.spec.id, transport = self.spec.transport.label(), "MCP 服务器已就绪");
        Ok(())
    }

    /// 关闭连接：置 `Stopped`（拒绝新动作与新调用）→ 取消在途调用 → 等优雅收尾 → 超时强制终止。
    pub async fn close(&self, graceful_timeout: Duration) -> CloseOutcome {
        self.set_state(McpState::Stopped);
        let link = self.lock(&self.link).take();
        let Some(mut link) = link else {
            return CloseOutcome {
                graceful: true,
                killed: false,
            };
        };
        // 取消令牌让服务循环退出，循环退出时会按顺序关掉 transport（stdio 走优雅关闭）。
        link.cancel.cancel();
        let graceful = tokio::time::timeout(graceful_timeout, &mut link.monitor).await.is_ok();
        if !graceful {
            // 超时：中止监听任务并丢掉 `RunningService`，transport 的 drop 会杀掉直接子进程。
            link.monitor.abort();
            tracing::warn!(server = %self.spec.id, "MCP 连接未在超时内收尾，已强制终止");
        }
        self.set_last_error(None);
        CloseOutcome {
            graceful,
            killed: !graceful,
        }
    }

    /// 调用一个工具：参数原样透传（参数校验归 server），结果映射成 [`ToolOutput`]。
    ///
    /// 状态非 `Ready` 时直接返回错误、不发请求；`METHOD_NOT_FOUND` 会把状态改成 `Failed`
    /// （README §4.7「工具已被移除 → 调用期错误」）。`tool_name` 是 server 里的原名：server 不认
    /// 模型可见名，因此错误文案里出现的也是原名（可见名到原名的映射只有一层）。
    pub async fn call_tool(&self, tool_name: &str, args: Value) -> Result<ToolOutput, McpError> {
        let peer = self.ready_peer()?;
        let params = match args {
            Value::Object(arguments) => CallToolRequestParams::new(tool_name.to_owned()).with_arguments(arguments),
            // 非对象参数无法表达成 `arguments`，按「不带参数」发出，由 server 判定。
            _ => CallToolRequestParams::new(tool_name.to_owned()),
        };
        let result = match peer.call_tool(params).await {
            Ok(result) => result,
            Err(error) => {
                let error = McpError::from_service(&self.spec.id, Some(tool_name), &error);
                if matches!(error, McpError::ToolNotFound { .. }) {
                    self.fail(&error);
                }
                return Err(error);
            }
        };
        Ok(crate::result::map(result))
    }

    /// 发现工具：自动翻页 → 命名与冲突消解 → 存快照。
    ///
    /// `taken` 是「已被占用的名字」（内置工具与先发现的 server）。发现失败时保留上一次的快照、
    /// 记 `lastError`，状态仍是 `Ready`（连接是好的）。
    pub async fn discover(&self, taken: &BTreeSet<String>) -> Result<Vec<McpToolSpec>, McpError> {
        let peer = self.ready_peer()?;
        let raw: Vec<RawTool> = match discover::fetch(&peer, &self.spec.id).await {
            Ok(raw) => raw,
            Err(error) => {
                self.set_last_error(Some(error.to_string()));
                return Err(error);
            }
        };
        let specs = discover::resolve(&self.spec, raw, taken);
        *self.lock(&self.tools) = specs.clone();
        self.set_last_error(None);
        Ok(specs)
    }

    /// 取一个可用的 peer：非 `Ready` 时按状态给出固定文案（不发请求）。
    fn ready_peer(&self) -> Result<Peer<RoleClient>, McpError> {
        match self.state() {
            McpState::Ready => self
                .lock(&self.link)
                .as_ref()
                .map(|link| link.peer.clone())
                .ok_or_else(|| McpError::unavailable(&self.spec.id)),
            // 已经失败过：说「通信失败」比「不可用」更贴近事实。
            McpState::Failed { .. } => Err(McpError::communication(&self.spec.id)),
            _ => Err(McpError::unavailable(&self.spec.id)),
        }
    }

    /// 读循环结束：`Stopped` 保持不动（关闭流程自己负责），其余一律改 `Failed`。
    ///
    /// 旧连接（代次已过期）的收尾不参与判定：重连期间两代连接会短暂并存。
    fn on_service_end(&self, generation: u64) {
        if self.generation.load(Ordering::Relaxed) != generation {
            return;
        }
        if self.state() == McpState::Stopped {
            return;
        }
        let error = McpError::communication(&self.spec.id);
        tracing::warn!(server = %self.spec.id, "MCP 连接已结束");
        self.fail(&error);
    }

    /// 置失败状态并记原因（唯一写入者）。
    fn fail(&self, error: &McpError) {
        let reason = error.to_string();
        *self.lock(&self.state) = McpState::Failed { reason: reason.clone() };
        self.set_last_error(Some(reason));
    }

    /// 置状态。
    fn set_state(&self, state: McpState) {
        *self.lock(&self.state) = state;
    }

    /// 记 / 清 `lastError`。
    fn set_last_error(&self, reason: Option<String>) {
        *self.lock(&self.last_error) = reason;
    }

    /// 取锁；锁中毒时沿用内部数据（与 `agent-tool` 的注册表口径一致）。
    fn lock<'a, T>(&self, target: &'a Mutex<T>) -> MutexGuard<'a, T> {
        target.lock().unwrap_or_else(|error| error.into_inner())
    }
}

/// 握手：`serve` 即完成 initialize 协商，整体受 `connect_timeout` 约束。
async fn handshake<T, E, A>(
    spec: &McpServerSpec,
    handler: LingtingClientHandler,
    transport: T,
    cancel: &CancellationToken,
) -> Result<RunningService<RoleClient, LingtingClientHandler>, McpError>
where
    T: IntoTransport<RoleClient, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let timeout = spec.connect_timeout;
    let served = tokio::time::timeout(timeout, handler.serve_with_ct(transport, cancel.clone())).await;
    match served {
        Ok(Ok(service)) => Ok(service),
        Ok(Err(error)) => {
            // 失败的 `serve` 已经把 transport 丢掉：stdio 的直接子进程随之被终止。
            cancel.cancel();
            Err(McpError::from_initialize(&spec.id, &error))
        }
        Err(_elapsed) => {
            // 超时：先取消（服务循环退出并关闭 transport），再置 `ConnectTimeout`。
            cancel.cancel();
            Err(McpError::ConnectTimeout {
                server_id: spec.id.clone(),
                timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
            })
        }
    }
}

/// 客户端处理器：只做两件事——带 `clientInfo`、把 `tools/list_changed` 记成一次待刷新。
struct LingtingClientHandler {
    tool_changed: Arc<AtomicU64>,
}

impl ClientHandler for LingtingClientHandler {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new(CLIENT_NAME, env!("CARGO_PKG_VERSION")),
        )
    }

    async fn on_tool_list_changed(&self, _context: NotificationContext<RoleClient>) {
        // 只记账，重新发现由 manager 触发（D-P4-010）：客户端不认识注册表。
        self.tool_changed.fetch_add(1, Ordering::Relaxed);
    }
}
