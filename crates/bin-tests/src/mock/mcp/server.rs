//! MCP 测试替身的服务端实现（P4-017）：stdio 与 streamable HTTP 共用一份 `ServerHandler`。
//!
//! 替身不做断言、不读被测代码：它只按剧本回答 `tools/list` / `tools/call`，并把「观察到了什么」
//! 写进结构化证据文件（`--evidence`）：`clientInfo`、工具清单请求次数、每次调用的开始/结束时间与
//! 结果类型。用例读证据文件取数——与 Mock LLM 的 `/mock/last-request` 同一口径（`AGENTS.md`
//! 的测试数据采集条款：结构化快照，不经日志）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData, ImageContent, Implementation,
    InitializeRequestParams, InitializeResult, JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
    ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ServerHandler, ServiceExt as _};
use serde_json::{Value, json};

use super::MockTransport;
use super::scenario::{self, Behavior, Scenario};
/// 一次 `tools/call` 的观察结果（证据文件里的 `calls` 元素）。
struct CallRecord {
    /// server 报告的工具原名。
    name: String,
    /// 参数原文（紧凑 JSON）。
    arguments: String,
    /// 收到请求的时间（Unix 毫秒）。
    started_at_ms: i64,
    /// 结束时间（Unix 毫秒）；未结束时为空。
    ended_at_ms: Option<i64>,
    /// 结果形态：`text` / `echo` / `text_image` / `structured` / `empty` / `tool_error` /
    /// `protocol_error` / `crash` / `sleep`。
    outcome: String,
}

/// 证据文件的内容：一次运行里替身观察到的事实。
struct EvidenceData {
    /// 剧本名。
    scenario: &'static str,
    /// 传输方式标签。
    transport: &'static str,
    /// 替身进程 PID。
    pid: u32,
    /// 客户端在 `initialize` 里报的名字与版本。
    client: Option<(String, String)>,
    /// `tools/list` 的成功次数。
    /// `tools/list` 的成功次数。
    tools_list_calls: u64,
    /// 每次 `tools/list` 返回的工具原名（按轮次；清单变化用例据此断言「第一次只有基础工具」）。
    tool_rounds: Vec<Vec<String>>,
    /// 已发出的 `notifications/tools/list_changed` 次数。
    list_changed_sent: u64,
    /// 按到达顺序的调用记录。
    calls: Vec<CallRecord>,
}

/// 证据写入点：内存里唯一一份，每次变更后整份重写到 `--evidence` 指定的路径。
pub(crate) struct Evidence {
    path: Option<PathBuf>,
    data: Mutex<EvidenceData>,
}

impl Evidence {
    /// 建一份证据；`path` 为空时只在内存里记（不影响调用行为）。
    fn new(path: Option<PathBuf>, scenario: Scenario, transport: MockTransport) -> Self {
        Self {
            path,
            data: Mutex::new(EvidenceData {
                scenario: scenario.name(),
                transport: transport.label(),
                pid: std::process::id(),
                client: None,
                tools_list_calls: 0,
                tool_rounds: Vec::new(),
                list_changed_sent: 0,
                calls: Vec::new(),
            }),
        }
    }

    /// 记下客户端自报的名字与版本（只在第一次 `initialize` 后有效）。
    fn record_client(&self, implementation: &Implementation) {
        if self.lock().client.is_some() {
            return;
        }
        self.lock().client = Some((implementation.name.to_string(), implementation.version.to_string()));
        self.flush();
    }

    /// 记一次成功的 `tools/list` 与这次返回的工具原名。
    fn record_tools_list(&self, names: &[String]) {
        {
            let mut data = self.lock();
            data.tools_list_calls += 1;
            data.tool_rounds.push(names.to_vec());
        }
        self.flush();
    }

    /// 记一次已发出的清单变化通知。
    fn record_list_changed(&self) {
        self.lock().list_changed_sent += 1;
        self.flush();
    }

    /// 记一次调用的开始，返回它在 `calls` 里的下标。
    fn record_call_start(&self, name: &str, arguments: &str, outcome: &str) -> usize {
        let mut data = self.lock();
        data.calls.push(CallRecord {
            name: name.to_owned(),
            arguments: arguments.to_owned(),
            started_at_ms: lib_core::time::now_millis(),
            ended_at_ms: None,
            outcome: outcome.to_owned(),
        });
        drop(data);
        self.flush();
        self.lock().calls.len() - 1
    }

    /// 记一次调用的结束。
    fn record_call_end(&self, index: usize) {
        let mut data = self.lock();
        if let Some(record) = data.calls.get_mut(index) {
            record.ended_at_ms = Some(lib_core::time::now_millis());
        }
        drop(data);
        self.flush();
    }

    /// 整份重写证据文件：文件小、次数少，直接覆盖比增量追加更不容易半行。
    fn flush(&self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        let text = self.snapshot().to_string();
        if let Err(error) = std::fs::write(path, text) {
            // 证据写不出去不是被测行为：只报错给替身自己的 stderr，用例会在「读不到证据」上失败。
            eprintln!("mock-mcp 写证据文件失败：{}（{error}）", path.display());
        }
    }

    /// 证据的 JSON 形态（文件内容与断言口径的唯一来源）。
    fn snapshot(&self) -> Value {
        let data = self.lock();
        let calls: Vec<Value> = data
            .calls
            .iter()
            .map(|call| {
                json!({
                    "name": call.name,
                    "arguments": call.arguments,
                    "startedAtMs": call.started_at_ms,
                    "endedAtMs": call.ended_at_ms,
                    "outcome": call.outcome,
                })
            })
            .collect();
        json!({
            "scenario": data.scenario,
            "transport": data.transport,
            "pid": data.pid,
            "clientInfo": data.client.as_ref().map(|(name, version)| json!({"name": name, "version": version})),
            "toolsListCalls": data.tools_list_calls,
            "toolLists": data.tool_rounds,
            "listChangedSent": data.list_changed_sent,
            "calls": calls,
        })
    }

    /// 取锁；锁中毒时沿用内部数据（与仓内其他口径一致）。
    fn lock(&self) -> MutexGuard<'_, EvidenceData> {
        self.data.lock().unwrap_or_else(|error| error.into_inner())
    }
}

/// 替身的服务端处理器：按剧本回答，并把观察写进证据。
#[derive(Clone)]
struct MockMcp {
    scenario: Scenario,
    evidence: Arc<Evidence>,
    list_rounds: Arc<AtomicU64>,
}

impl ServerHandler for MockMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        // 记下 clientInfo（T-4），然后沿用默认协商结果。
        self.evidence.record_client(&request.client_info);
        context.peer.set_peer_info(request.clone());
        self.negotiate_initialize(&request)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let round = self.list_rounds.fetch_add(1, Ordering::SeqCst);
        let defs = self.scenario.tools(round);
        let names: Vec<String> = defs.iter().map(|tool| tool.name.to_owned()).collect();
        self.evidence.record_tools_list(&names);
        if self.scenario.announces_list_changed() && round == 0 {
            // 第一次列清单之后追加工具并发通知：客户端应重新发现（L-6）。
            let peer = context.peer.clone();
            let evidence = Arc::clone(&self.evidence);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(150)).await;
                if peer.notify_tool_list_changed().await.is_ok() {
                    evidence.record_list_changed();
                }
            });
        }
        let tools = defs.into_iter().map(remote_tool).collect::<Result<Vec<_>, _>>()?;
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        let arguments = request
            .arguments
            .map(Value::Object)
            .unwrap_or_else(|| json!({}))
            .to_string();
        let def = self
            .scenario
            .tools(self.list_rounds.load(Ordering::SeqCst))
            .into_iter()
            .find(|tool| tool.name == name)
            .ok_or_else(|| ErrorData::invalid_params(format!("unknown tool: {name}"), None))?;
        let index = self
            .evidence
            .record_call_start(&name, &arguments, outcome_label(def.behavior));
        if matches!(def.behavior, Behavior::Crash) {
            // 崩溃：证据已落盘，直接退出进程（客户端读循环随之结束）。
            self.evidence.record_call_end(index);
            std::process::exit(9);
        }
        let result = self.answer(def.behavior, &arguments).await;
        self.evidence.record_call_end(index);
        result
    }
}

impl MockMcp {
    /// 按剧本行为产出结果；延迟与长睡都用异步等待，不阻塞运行时线程。
    async fn answer(&self, behavior: Behavior, arguments: &str) -> Result<CallToolResponse, ErrorData> {
        let result = match behavior {
            Behavior::Text(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Behavior::Delayed(text, millis) => {
                tokio::time::sleep(Duration::from_millis(millis)).await;
                CallToolResult::success(vec![ContentBlock::text(text)])
            }
            Behavior::Echo => CallToolResult::success(vec![ContentBlock::text(format!("ECHO：{arguments}"))]),
            Behavior::TextImage => CallToolResult::success(vec![
                ContentBlock::text(scenario::IMAGE_TEXT),
                ContentBlock::Image(ImageContent::new(scenario::IMAGE_DATA, scenario::IMAGE_MIME)),
            ]),
            Behavior::Structured => {
                // 只有结构化内容：内容段留空，文本由客户端从 `structuredContent` 退回来。
                let mut result = CallToolResult::success(Vec::new());
                result.structured_content = Some(scenario::structured_content());
                result
            }
            Behavior::Empty => CallToolResult::success(Vec::new()),
            Behavior::ToolError(text) => CallToolResult::error(vec![ContentBlock::text(text)]),
            Behavior::ProtocolError => {
                return Err(ErrorData::internal_error("mock 协议级错误", None));
            }
            Behavior::Sleep(millis) => {
                // 慢响应：阻塞到用例的超时 / 取消先发生；调用方 drop future 不影响这里。
                tokio::time::sleep(Duration::from_millis(millis)).await;
                CallToolResult::success(vec![ContentBlock::text("迟到的结果")])
            }
            // 崩溃分支在调用方直接退出进程，不会走到这里。
            Behavior::Crash => return Err(ErrorData::internal_error("mock 崩溃", None)),
        };
        Ok(CallToolResponse::Complete(result))
    }
}

impl MockMcp {
    /// 由剧本建处理器。
    fn new(scenario: Scenario, evidence: Arc<Evidence>) -> Self {
        Self {
            scenario,
            evidence,
            list_rounds: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// 剧本里的工具 → 协议里的工具（`inputSchema` 原样透传）。
fn remote_tool(def: scenario::ToolDef) -> Result<Tool, ErrorData> {
    let schema: JsonObject = serde_json::from_str::<Value>(def.schema)
        .map_err(|error| ErrorData::internal_error(format!("剧本 schema 不是 JSON：{error}"), None))?
        .as_object()
        .cloned()
        .ok_or_else(|| ErrorData::internal_error("剧本 schema 必须是对象", None))?;
    let mut tool = Tool::new(def.name, def.description, schema);
    tool.annotations = Some(ToolAnnotations::new().read_only(def.read_only));
    Ok(tool)
}

/// 证据里 `outcome` 的取值标签。
fn outcome_label(behavior: Behavior) -> &'static str {
    match behavior {
        Behavior::Text(_) | Behavior::Delayed(..) => "text",
        Behavior::Echo => "echo",
        Behavior::TextImage => "text_image",
        Behavior::Structured => "structured",
        Behavior::Empty => "empty",
        Behavior::ToolError(_) => "tool_error",
        Behavior::ProtocolError => "protocol_error",
        Behavior::Crash => "crash",
        Behavior::Sleep(_) => "sleep",
    }
}

/// 启动替身并按传输方式运行到进程结束。
pub(crate) async fn serve(
    transport: MockTransport,
    port: Option<u16>,
    scenario: Scenario,
    evidence_path: Option<PathBuf>,
    pid_file: Option<PathBuf>,
) -> Result<()> {
    let evidence = Arc::new(Evidence::new(evidence_path, scenario, transport));
    if let Some(path) = pid_file {
        std::fs::write(&path, std::process::id().to_string())
            .with_context(|| format!("写 PID 文件失败：{}", path.display()))?;
    }
    let handler = MockMcp::new(scenario, Arc::clone(&evidence));
    match transport {
        MockTransport::Stdio => {
            let service = handler
                .serve(rmcp::transport::stdio())
                .await
                .context("MCP 替身（stdio）握手失败")?;
            service.waiting().await.context("MCP 替身（stdio）异常结束")?;
            Ok(())
        }
        MockTransport::Http => {
            let port = port.context("http transport 必须给 --port")?;
            let service = StreamableHttpService::new(
                move || Ok(handler.clone()),
                Arc::new(LocalSessionManager::default()),
                StreamableHttpServerConfig::default().with_sse_keep_alive(None),
            );
            let router = axum::Router::new().nest_service("/mcp", service);
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
                .await
                .with_context(|| format!("MCP 替身（http）绑定端口 {port} 失败"))?;
            axum::serve(listener, router)
                .await
                .context("MCP 替身（http）异常结束")?;
            Ok(())
        }
    }
}
