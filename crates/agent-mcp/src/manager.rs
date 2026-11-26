//! `McpManager`：多 server 的门面——状态快照、启停、退避重连与工具快照差量注册
//!（README §4.5 / §4.7 / §4.8，D-P4-010 / D-P4-011）。
//!
//! 对外只有「状态与动作」：`snapshot` / `tool_snapshot` 只读，`enable` / `disable` /
//! `reconnect` / `probe` 四个动作各自幂等；对内负责把发现结果与注册表做差集、按 server 退避
//! 重连、响应 `notifications/tools/list_changed`。
//!
//! 锁纪律（P2.1 延续）：每个 server 一把操作闸门（[`tokio::sync::Mutex`]），启停 / 发现 / 关闭
//! 只在**该 server** 内串行，任意时刻都不持全局锁跨 `await`；只读快照走一把同步小锁，锁内
//! 只做内存读写。监督任务只持有自己 server 的 `Arc`，因此关闭流程不会与它争同一把锁。
//!
//! 关闭期间的自动重连由两处挡住：`shutdown_all` 置 `stopped` 并取消全部监督任务；
//! 监督任务在每次重连前重新检查 `lib_core::lifecycle` 的关闭标记（D-P4-011）。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use agent_tool::ToolRegistry;
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

use crate::client::McpClient;
use crate::config::{McpConfig, McpServerSpec};
use crate::metrics::{self, McpStateCounts};
use crate::state::McpState;
use crate::tool::McpTool;
use crate::vo::{McpAction, McpServerVO, McpStatusKind, McpToolVO};

/// 关闭单个连接的优雅超时：固定常量，不新增配置项（D-P4-020）。
pub const GRACEFUL_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// 退避的增长倍率（D-P4-011：初始值起、2 倍增长、到上限封顶、无限次）。
const BACKOFF_FACTOR: u32 = 2;

/// 管理面动作的唯一失败取值：未知的 server `id`。
///
/// 文案只有一句（`MCP 服务器不存在`，README §4.8），不含任何内部细节；接口层据此返回 400。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpManagerError {
    /// 配置里没有这个 `id`。
    UnknownServer,
}

impl std::fmt::Display for McpManagerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownServer => formatter.write_str("MCP 服务器不存在"),
        }
    }
}

impl std::error::Error for McpManagerError {}

/// 退避参数：从配置里读一次，之后按 server 各自演进。
#[derive(Debug, Clone, Copy)]
struct RetryPolicy {
    /// 初始退避。
    initial: Duration,
    /// 上限；0 表示关闭自动重连。
    max: Duration,
}

impl RetryPolicy {
    /// 下一次退避：2 倍增长到上限封顶。
    fn next(self, backoff: Duration) -> Duration {
        backoff.saturating_mul(BACKOFF_FACTOR).min(self.max)
    }
}

/// 多 server 的门面：装配期建一次，运行期由管理接口与工具发现共同驱动。
pub struct McpManager {
    registry: Arc<ToolRegistry>,
    retry: RetryPolicy,
    servers: BTreeMap<String, Arc<ServerEntry>>,
    /// 关闭标记：置位后快照一律报 `STOPPED`，重连与启停都不再排新的动作。
    stopped: AtomicBool,
}

/// 一个 server 的全部运行期状态：客户端 + 配置 + 操作闸门 + 只读视图。
struct ServerEntry {
    client: Arc<McpClient>,
    spec: McpServerSpec,
    /// 该 server 的操作闸门：连接 / 发现 / 关闭串行（不同 server 之间互不影响）。
    gate: AsyncMutex<()>,
    /// 只读视图：锁内只做内存读写，不跨 `await`。
    view: Mutex<View>,
}

/// 一个 server 的轻量状态（快照与差集都用它）。
#[derive(Default)]
struct View {
    /// 配置里的启用标记；`disable` 后为假。
    enabled: bool,
    /// 当前注册在册的模型可见名（差集键）。
    registered: BTreeSet<String>,
    /// 最近一次连接成功的时间（Unix 毫秒）。
    last_connected_at_ms: Option<i64>,
    /// 下一次自动重连的时间（Unix 毫秒）。
    next_retry_at_ms: Option<i64>,
    /// 监督任务的取消令牌（工具清单变化与连接结束的响应者）。
    supervisor: Option<CancellationToken>,
}

impl McpManager {
    /// 由配置建门面：只建句柄，不连接（是否连接由 `connect_enabled_all` 或动作决定）。
    pub fn new(config: McpConfig, registry: Arc<ToolRegistry>) -> Self {
        let mut servers = BTreeMap::new();
        for spec in config.servers {
            let id = spec.id.clone();
            let entry = Arc::new(ServerEntry::new(spec));
            servers.insert(id, entry);
        }
        Self {
            registry,
            retry: RetryPolicy {
                initial: config.reconnect_initial_backoff,
                max: config.reconnect_max_backoff,
            },
            servers,
            stopped: AtomicBool::new(false),
        }
    }

    /// 并发连接全部 `enabled` 的 server：每个 server 受自己的 `connect_timeout` 约束，
    /// 失败只记状态（不返回错误，装配照常完成）。
    pub async fn connect_enabled_all(&self) {
        let enabled: Vec<Arc<ServerEntry>> = self
            .servers
            .values()
            .filter(|entry| entry.is_enabled())
            .map(Arc::clone)
            .collect();
        let connects = enabled.into_iter().map(|entry| {
            let registry = Arc::clone(&self.registry);
            let retry = self.retry;
            async move { entry.enable(&registry, retry).await }
        });
        futures_util::future::join_all(connects).await;
    }

    /// 全部 server 的状态快照：按 `id` 升序，只读。
    pub fn snapshot(&self) -> Vec<McpServerVO> {
        self.servers.values().map(|entry| self.server_vo(entry)).collect()
    }

    /// 当前已发现的工具快照：按模型可见名升序，只读（不含未注册成功的条目）。
    pub fn tool_snapshot(&self) -> Vec<McpToolVO> {
        let mut tools: Vec<McpToolVO> = self
            .servers
            .values()
            .flat_map(|entry| {
                let registered = entry.registered_names();
                entry
                    .client
                    .tools()
                    .into_iter()
                    .filter(|spec| registered.contains(&spec.tool_name))
                    .map(|spec| McpToolVO {
                        server_id: spec.server_id,
                        tool_name: spec.tool_name,
                        original_name: spec.original_name,
                        description: spec.description,
                        parameters: spec.schema.to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        tools.sort_by(|left, right| left.tool_name.cmp(&right.tool_name));
        tools
    }

    /// 取快照时刻的 server 与工具现状（`metrics` 的 `mcp` 段用，不做累加）。
    pub fn state_counts(&self) -> McpStateCounts {
        let mut counts = McpStateCounts {
            servers: u64::try_from(self.servers.len()).unwrap_or(u64::MAX),
            ..McpStateCounts::default()
        };
        for entry in self.servers.values() {
            counts.tools = counts
                .tools
                .saturating_add(u64::try_from(entry.registered_names().len()).unwrap_or(u64::MAX));
            match self.status_of(entry) {
                McpStatusKind::Ready => counts.ready += 1,
                McpStatusKind::Failed => counts.failed += 1,
                McpStatusKind::Disabled => counts.disabled += 1,
                // `CONNECTING` 不落任何计数（状态在变），`STOPPED` 只在关闭后出现。
                McpStatusKind::Connecting | McpStatusKind::Stopped => {}
            }
        }
        counts
    }

    /// 执行一个管理动作，返回动作完成后的状态（P4-014 的 handler 只调这里）。
    pub async fn apply(&self, server_id: &str, action: McpAction) -> Result<McpServerVO, McpManagerError> {
        match action {
            McpAction::Enable => self.enable(server_id).await,
            McpAction::Disable => self.disable(server_id).await,
            McpAction::Reconnect => self.reconnect(server_id).await,
            McpAction::Probe => self.probe(server_id).await,
        }
    }

    /// `ENABLE`：连接并发现；已 `READY` 时直接返回当前状态（幂等，不建第二条连接）。
    pub async fn enable(&self, server_id: &str) -> Result<McpServerVO, McpManagerError> {
        let entry = self.entry(server_id)?;
        if self.stopped() {
            // 关闭后拒绝新动作：只回报当前 `STOPPED` 状态，不再拉起任何连接。
            return Ok(self.server_vo(entry));
        }
        entry.enable(&self.registry, self.retry).await;
        Ok(self.server_vo(entry))
    }

    /// `DISABLE`：断开并注销该 server 的全部工具（幂等）。
    pub async fn disable(&self, server_id: &str) -> Result<McpServerVO, McpManagerError> {
        let entry = self.entry(server_id)?;
        if self.stopped() {
            return Ok(self.server_vo(entry));
        }
        entry.disable(&self.registry).await;
        Ok(self.server_vo(entry))
    }

    /// `RECONNECT`：先关闭旧连接，再按 `ENABLE` 走一遍。
    pub async fn reconnect(&self, server_id: &str) -> Result<McpServerVO, McpManagerError> {
        let entry = self.entry(server_id)?;
        if self.stopped() {
            return Ok(self.server_vo(entry));
        }
        entry.reconnect(&self.registry, self.retry).await;
        Ok(self.server_vo(entry))
    }

    /// `PROBE`：只读探测，`READY` 时真发一次 `tools/list`，其余状态直接返回状态本身。
    pub async fn probe(&self, server_id: &str) -> Result<McpServerVO, McpManagerError> {
        let entry = self.entry(server_id)?;
        if self.stopped() {
            return Ok(self.server_vo(entry));
        }
        entry.probe().await;
        Ok(self.server_vo(entry))
    }

    /// 关闭全部连接：置 `STOPPED` → 取消在途调用 → 逐个优雅关闭（超时则强制终止）。
    ///
    /// 返回被强制终止的连接数（`shutdown_kills`），由调用方记日志。
    pub async fn shutdown_all(&self) -> usize {
        self.stopped.store(true, Ordering::SeqCst);
        // 先打断监督任务：关闭期间不排新的重连，也不再刷新工具清单（D-P4-011）。
        for entry in self.servers.values() {
            entry.stop_supervisor();
        }
        let closes = self.servers.values().map(|entry| entry.close(&self.registry));
        let kills = futures_util::future::join_all(closes).await;
        kills.into_iter().filter(|killed| *killed).count()
    }

    /// 关闭标记：置位后拒绝新动作（README §4.8 的 `STOPPED`）。
    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// 按 `id` 取 server。
    fn entry(&self, server_id: &str) -> Result<&Arc<ServerEntry>, McpManagerError> {
        self.servers.get(server_id).ok_or(McpManagerError::UnknownServer)
    }

    /// 一个 server 的状态快照。
    fn server_vo(&self, entry: &ServerEntry) -> McpServerVO {
        let view = entry.view();
        McpServerVO {
            id: entry.spec.id.clone(),
            transport: entry.spec.transport.label().to_owned(),
            enabled: view.enabled,
            status: self.status_of(entry),
            tool_count: u32::try_from(view.registered.len()).unwrap_or(u32::MAX),
            last_error: entry.client.last_error(),
            last_connected_at_ms: view.last_connected_at_ms,
            next_retry_at_ms: view.next_retry_at_ms,
        }
    }

    /// 对外状态：关闭后一律 `STOPPED`，未启用一律 `DISABLED`，其余取客户端状态机。
    fn status_of(&self, entry: &ServerEntry) -> McpStatusKind {
        if self.stopped() {
            return McpStatusKind::Stopped;
        }
        if !entry.is_enabled() {
            return McpStatusKind::Disabled;
        }
        match entry.client.state() {
            McpState::Disabled => McpStatusKind::Disabled,
            McpState::Connecting => McpStatusKind::Connecting,
            McpState::Ready => McpStatusKind::Ready,
            McpState::Failed { .. } => McpStatusKind::Failed,
            McpState::Stopped => McpStatusKind::Stopped,
        }
    }
}

impl Drop for McpManager {
    fn drop(&mut self) {
        // 优雅关闭没走完就丢掉了门面（进程 panic 或装配提前退出）：子进程只剩 `kill_on_drop` 兜底。
        let alive: Vec<&str> = self
            .servers
            .values()
            .filter(|entry| matches!(entry.client.state(), McpState::Ready | McpState::Connecting))
            .map(|entry| entry.spec.id.as_str())
            .collect();
        if !alive.is_empty() {
            tracing::warn!(servers = ?alive, "MCP 连接在关闭前被丢弃，子进程由 kill_on_drop 兜底");
        }
    }
}

impl ServerEntry {
    /// 由配置建条目：初始视图按 `enabled` 落位（是否连接仍由调用方决定）。
    fn new(spec: McpServerSpec) -> Self {
        let enabled = spec.enabled;
        Self {
            client: Arc::new(McpClient::new(spec.clone())),
            spec,
            gate: AsyncMutex::new(()),
            view: Mutex::new(View {
                enabled,
                ..View::default()
            }),
        }
    }

    /// 是否启用（含关闭后的情形：关闭流程不改这个标记，状态由 manager 统一报 `STOPPED`）。
    fn is_enabled(&self) -> bool {
        self.view().enabled
    }

    /// 当前注册在册的工具名。
    fn registered_names(&self) -> BTreeSet<String> {
        self.view().registered.clone()
    }

    /// `ENABLE`：置启用 → 起监督任务 → 未 `READY` 就连接并发现（幂等）。
    async fn enable(self: &Arc<Self>, registry: &Arc<ToolRegistry>, retry: RetryPolicy) {
        // 幂等：已经 `READY` 时不建第二条连接，也不重排监督任务。
        if self.client.state().is_ready() && self.is_enabled() {
            return;
        }
        {
            let mut view = self.lock_view();
            view.enabled = true;
        }
        self.start_supervisor(registry, retry);
        let _gate = self.gate.lock().await;
        self.connect_locked(registry).await;
    }

    /// `DISABLE`：先打断监督任务与在途调用，再注销工具（幂等）。
    async fn disable(&self, registry: &ToolRegistry) {
        self.stop_supervisor();
        {
            let mut view = self.lock_view();
            view.enabled = false;
            view.next_retry_at_ms = None;
        }
        // 先关连接：取消在途调用（包括可能卡在 `tools/list` 的那一次），再去等操作闸门。
        let _ = self.close_connection().await;
        let _gate = self.gate.lock().await;
        self.unregister_all(registry);
    }

    /// `RECONNECT`：关闭旧连接与工具，再按 `ENABLE` 走一遍。
    async fn reconnect(self: &Arc<Self>, registry: &Arc<ToolRegistry>, retry: RetryPolicy) {
        self.stop_supervisor();
        {
            let mut view = self.lock_view();
            view.enabled = true;
            view.next_retry_at_ms = None;
        }
        let _ = self.close_connection().await;
        {
            let _gate = self.gate.lock().await;
            self.unregister_all(registry);
        }
        self.start_supervisor(registry, retry);
        let _gate = self.gate.lock().await;
        self.connect_locked(registry).await;
    }

    /// `PROBE`：`READY` 时真正发一次 `tools/list`，其余状态不发请求、不改状态。
    async fn probe(&self) {
        if self.client.state().is_ready() {
            let _ = self.client.probe().await;
        }
    }

    /// 关闭流程里的单个连接：置 `STOPPED`、注销工具，返回是否被强制终止。
    async fn close(&self, registry: &ToolRegistry) -> bool {
        {
            let mut view = self.lock_view();
            view.next_retry_at_ms = None;
        }
        let killed = self.close_connection().await;
        if killed {
            metrics::record_shutdown_kill();
        }
        let _gate = self.gate.lock().await;
        self.unregister_all(registry);
        killed
    }

    /// 关掉客户端连接，返回是否因超时被强制终止（`DISABLE` / `RECONNECT` / 关闭共用）。
    ///
    /// 客户端自己记一条 `warn`；`shutdown_kills` 只在关闭流程累加（口径是「服务关闭时」）。
    async fn close_connection(&self) -> bool {
        self.client.close(GRACEFUL_CLOSE_TIMEOUT).await.killed
    }

    /// 连接并发现：失败只记状态（`Failed` + `lastError`），不向上传播。
    async fn connect_locked(&self, registry: &Arc<ToolRegistry>) {
        metrics::record_connect();
        match self.client.connect().await {
            Ok(()) => {
                {
                    let mut view = self.lock_view();
                    view.last_connected_at_ms = Some(lib_core::time::now_millis());
                    view.next_retry_at_ms = None;
                }
                self.refresh_locked(registry).await;
                tracing::info!(
                    server = %self.spec.id,
                    transport = self.spec.transport.label(),
                    "MCP 服务器已连接"
                );
            }
            Err(error) => {
                metrics::record_connect_failure();
                // 错误文案已是固定中文（不含 URL / 请求头 / 命令行）。
                tracing::warn!(server = %self.spec.id, error = %error, "MCP 服务器连接失败");
            }
        }
    }

    /// 重新发现并做差集注册（工具清单变化与重连都走这里）。
    async fn refresh(self: &Arc<Self>, registry: &Arc<ToolRegistry>) {
        let _gate = self.gate.lock().await;
        self.refresh_locked(registry).await;
    }

    /// 差集刷新：新增注册、消失注销，仍在清单里的保持不动（D-P4-010）。
    async fn refresh_locked(&self, registry: &Arc<ToolRegistry>) {
        if !self.client.state().is_ready() {
            return;
        }
        // 差集键是模型可见名。占用名里先摘掉本 server 已注册的：否则会给自己加哈希后缀。
        let mut taken: BTreeSet<String> = registry.names().into_iter().collect();
        for name in self.registered_names() {
            taken.remove(&name);
        }
        let specs = match self.client.discover(&taken).await {
            Ok(specs) => specs,
            // 发现失败保留上一次快照，状态仍是 `READY`（连接是好的）。
            Err(error) => {
                tracing::warn!(server = %self.spec.id, error = %error, "MCP 工具发现失败，保留上一次快照");
                return;
            }
        };
        let previous = self.registered_names();
        let next: BTreeSet<String> = specs.iter().map(|spec| spec.tool_name.clone()).collect();
        // 先注销消失的，再注册新增的：名字被释放后才能复用，不会与本 server 的旧条目撞名。
        for name in previous.difference(&next) {
            registry.unregister(name);
        }
        let mut registered: BTreeSet<String> = previous.intersection(&next).cloned().collect();
        let permission = Some(self.spec.permission);
        for spec in specs {
            if registered.contains(&spec.tool_name) {
                continue;
            }
            let name = spec.tool_name.clone();
            let tool = Arc::new(McpTool::new(Arc::clone(&self.client), spec));
            match registry.register_with_default(tool, permission) {
                Ok(()) => {
                    registered.insert(name);
                }
                // 注册失败（撞名）不进视图：下一次发现会再试一次。
                Err(error) => tracing::warn!(server = %self.spec.id, error = %error, "MCP 工具注册失败"),
            }
        }
        self.lock_view().registered = registered;
    }

    /// 注销该 server 的全部工具（差集视图同时清空）。
    fn unregister_all(&self, registry: &ToolRegistry) {
        for name in std::mem::take(&mut self.lock_view().registered) {
            registry.unregister(&name);
        }
    }

    /// 起（或换）监督任务：旧的先取消，新的从当前状态继续。
    ///
    /// 任务被分离（不等待 join）：它自己观察取消令牌后退出，因此启停与关闭都不会被它阻塞。
    fn start_supervisor(self: &Arc<Self>, registry: &Arc<ToolRegistry>, retry: RetryPolicy) {
        self.stop_supervisor();
        let cancel = CancellationToken::new();
        let entry = Arc::clone(self);
        let registry = Arc::clone(registry);
        let token = cancel.clone();
        tokio::spawn(async move { entry.supervise(registry, retry, token).await });
        self.lock_view().supervisor = Some(cancel);
    }

    /// 取消监督任务（不等待：任务自己观察取消后退出）。
    fn stop_supervisor(&self) {
        if let Some(cancel) = self.lock_view().supervisor.take() {
            cancel.cancel();
        }
    }

    /// 监督任务：响应工具清单变化（重新发现）与连接结束（退避重连）。
    async fn supervise(self: Arc<Self>, registry: Arc<ToolRegistry>, retry: RetryPolicy, cancel: CancellationToken) {
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                () = self.client.wait_tool_list_changed() => {
                    // `notifications/tools/list_changed`：重新拉一次该 server 的工具（D-P4-010）。
                    let _ = self.client.take_tool_list_changed();
                    self.refresh(&registry).await;
                }
                () = self.client.wait_state_change() => {
                    if cancel.is_cancelled() || !self.is_enabled() || lib_core::lifecycle::is_shutdown() {
                        return;
                    }
                    if matches!(self.client.state(), McpState::Failed { .. }) {
                        self.retry_until_ready(&registry, retry, &cancel).await;
                    }
                }
            }
        }
    }

    /// 退避重连：初始值起、2 倍增长到上限、无限次；`max == 0` 关闭自动重连。
    async fn retry_until_ready(
        self: &Arc<Self>,
        registry: &Arc<ToolRegistry>,
        retry: RetryPolicy,
        cancel: &CancellationToken,
    ) {
        if retry.max.is_zero() {
            self.lock_view().next_retry_at_ms = None;
            return;
        }
        let mut backoff = retry.initial.min(retry.max);
        loop {
            {
                let mut view = self.lock_view();
                view.next_retry_at_ms = Some(lib_core::time::now_millis().saturating_add(millis(backoff)));
            }
            tokio::select! {
                () = cancel.cancelled() => return,
                () = tokio::time::sleep(backoff) => {}
            }
            // 关闭流程置位后不再排新的重连（D-P4-011）。
            if cancel.is_cancelled() || !self.is_enabled() || lib_core::lifecycle::is_shutdown() {
                return;
            }
            self.lock_view().next_retry_at_ms = None;
            metrics::record_reconnect();
            {
                let _gate = self.gate.lock().await;
                self.connect_locked(registry).await;
            }
            if self.client.state().is_ready() {
                return;
            }
            backoff = retry.next(backoff);
        }
    }

    /// 取只读视图的副本。
    fn view(&self) -> ViewSnapshot {
        let view = self.lock_view();
        ViewSnapshot {
            enabled: view.enabled,
            registered: view.registered.clone(),
            last_connected_at_ms: view.last_connected_at_ms,
            next_retry_at_ms: view.next_retry_at_ms,
        }
    }

    /// 取视图锁；锁中毒时沿用内部数据（与注册表口径一致）。
    fn lock_view(&self) -> MutexGuard<'_, View> {
        self.view.lock().unwrap_or_else(|error| error.into_inner())
    }
}

/// 视图的一次性副本（快照读一次，不带锁出去）。
struct ViewSnapshot {
    enabled: bool,
    registered: BTreeSet<String>,
    last_connected_at_ms: Option<i64>,
    next_retry_at_ms: Option<i64>,
}

/// 毫秒 → `i64`（饱和：真实退避不可能接近 2^63 毫秒）。
fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}
