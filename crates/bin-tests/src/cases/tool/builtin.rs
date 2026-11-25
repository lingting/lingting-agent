//! 内置工具与工具自述用例（T-1 ~ T-6、T-8）。
//!
//! `Calculator` / `CurrentTime` / `FileRead` / `FileSearch` / `HttpRequestTool` 都在进程内调用：
//! 工具契约本身就是纯异步函数，绕开服务端能让边界（除零、二进制、越界、重定向）断言更直接，
//! 也避免为了一个边界值多起一个服务端。

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use agent_tool::{
    Tool, ToolConcurrency, ToolContext, ToolFileAccess, ToolPermission, ToolPolicy, ToolRegistry, ToolRisk,
};
use anyhow::{Context as _, Result, ensure};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::case;
use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;
use crate::util::support::temp_directory;

/// 文件类工具的单次读取上限（字节）：足够小，方便构造「超过上限」的文件。
const FILE_MAX_BYTES: u64 = 64;

/// 内置工具与自述用例。
pub(super) fn cases() -> Vec<Case> {
    vec![
        case("tool.builtin.calculator", CaseKind::Shared, calculator),
        case("tool.builtin.current_time", CaseKind::Shared, current_time),
        case("tool.builtin.file", CaseKind::Shared, file),
        case("tool.builtin.http", CaseKind::Shared, http),
        case("tool.decl", CaseKind::Shared, decl),
    ]
}

/// 一次工具调用的上下文：`home` 是应用根目录，文件类工具的相对路径基于它。
fn context_of(home: &Path) -> ToolContext {
    ToolContext {
        home: home.to_path_buf(),
        session_id: "s-tool".to_string(),
        run_id: "r-tool".to_string(),
        tool_call_id: "c-tool".to_string(),
        deadline: Instant::now() + Duration::from_secs(10),
    }
}

/// 一次调用的结果：文本与是否错误。
async fn call_text<T: Tool>(tool: &T, context: &ToolContext, args: serde_json::Value) -> (String, bool) {
    match tool.call(context, args).await {
        Ok(output) => (output.text, output.is_error),
        Err(error) => (error.to_string(), true),
    }
}

/// `calculator`：四则与括号正确；除零、语法错误、超长表达式都是固定文案，不 panic。
fn calculator(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let tool = agent_tool::builtin::Calculator;
        let home = temp_directory("tool-calculator")?;
        let ctx = context_of(&home);
        let mut notes = Vec::new();

        for (expression, expected) in [
            ("(2+3)*4", "20"),
            ("7 % 4", "3"),
            ("-3 % 2", "-1"),
            ("1.5 + 2.25", "3.75"),
        ] {
            let (text, error) = call_text(&tool, &ctx, serde_json::json!({"expression": expression})).await;
            ensure!(!error, "{expression} 应计算成功：{text}");
            ensure!(text == expected, "{expression} 应为 {expected}，实际 {text}");
            notes.push(format!("{expression} = {text}"));
        }

        // 除零：`Failed`，固定文案，不返回 inf/NaN。
        let (text, error) = call_text(&tool, &ctx, serde_json::json!({"expression": "1/0"})).await;
        ensure!(error, "除零应被拒绝");
        ensure!(text == "工具执行失败：除数不能为零", "除零文案不符：{text}");

        // 语法错误与不支持的字符：`Failed` 与 `InvalidArguments` 各自的固定文案。
        let (text, _) = call_text(&tool, &ctx, serde_json::json!({"expression": "1+"})).await;
        ensure!(text == "工具执行失败：表达式语法错误", "语法错误文案不符：{text}");
        let (text, _) = call_text(&tool, &ctx, serde_json::json!({"expression": "pow(2,3)"})).await;
        ensure!(
            text.starts_with("参数不合法：") && text.contains("只支持数字"),
            "函数调用应被判为不支持的字符：{text}"
        );

        // 超长表达式：按字符数上限拒绝，不进入解析器。
        let long = "1+".repeat(300) + "1";
        let (text, _) = call_text(&tool, &ctx, serde_json::json!({"expression": long})).await;
        ensure!(
            text.starts_with("参数不合法：") && text.contains("长度不能超过"),
            "超长表达式应在参数校验阶段被拒：{text}"
        );
        notes.push(format!("边界：{text}"));

        Ok(notes)
    })
}

/// `current_time`：输出含 ISO8601 与 Unix 毫秒；非法 `zone` 是参数错误。
fn current_time(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let tool = agent_tool::builtin::CurrentTime;
        let home = temp_directory("tool-current-time")?;
        let ctx = context_of(&home);

        let (utc, error) = call_text(&tool, &ctx, serde_json::json!({})).await;
        ensure!(!error, "缺省应取 UTC：{utc}");
        ensure!(
            utc.contains("（Unix 毫秒：") && utc.contains("时区：utc"),
            "缺省输出应带毫秒与时区：{utc}"
        );
        ensure!(
            utc.starts_with("20") && utc.contains('T') && utc.ends_with("时区：utc）"),
            "输出应是 ISO8601：{utc}"
        );

        let (local, error) = call_text(&tool, &ctx, serde_json::json!({"zone": "LOCAL"})).await;
        ensure!(!error, "zone 大小写不敏感：{local}");
        ensure!(local.contains("时区：local"), "local 时区应回显 local：{local}");

        let (rejected, error) = call_text(&tool, &ctx, serde_json::json!({"zone": "Asia/Shanghai"})).await;
        ensure!(error, "不支持时区名");
        ensure!(
            rejected == "参数不合法：zone 只支持 utc 或 local",
            "非法 zone 的文案不符：{rejected}"
        );

        Ok(vec![
            format!("utc：{utc}"),
            format!("local：{local}"),
            format!("非法 zone：{rejected}"),
        ])
    })
}

/// `file_read` / `file_search`：白名单、大小上限、二进制、排除目录剪枝。
fn file(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let home = temp_directory("tool-file")?;
        let outside = temp_directory("tool-file-outside")?;
        std::fs::write(home.join("a.txt"), "hello")?;
        std::fs::write(home.join("big.txt"), "x".repeat(256))?;
        std::fs::write(home.join("binary.bin"), [0xff, 0xfe, 0x00, 0x01])?;
        std::fs::create_dir_all(home.join("target"))?;
        std::fs::write(home.join("target/skip.txt"), "should not be found")?;
        std::fs::create_dir_all(home.join("nested"))?;
        std::fs::write(home.join("nested/keep.txt"), "keep")?;
        std::fs::write(outside.join("secret.txt"), "outside")?;

        let ctx = context_of(&home);
        let read = agent_tool::builtin::FileRead::new(Vec::new(), FILE_MAX_BYTES);
        let mut notes = Vec::new();

        let (text, error) = call_text(&read, &ctx, serde_json::json!({"path": "a.txt"})).await;
        ensure!(!error, "白名单内应能读：{text}");
        ensure!(text == "文件：a.txt\nhello", "读取结果应带相对路径与正文：{text}");

        // 越界：拒绝文案不回显真实绝对路径。
        let outside_path = outside.join("secret.txt").to_string_lossy().to_string();
        let (text, error) = call_text(&read, &ctx, serde_json::json!({"path": outside_path})).await;
        ensure!(error, "白名单外应被拒绝：{text}");
        ensure!(
            text == "工具执行失败：路径不在允许的范围内或不存在",
            "越界文案不符：{text}"
        );
        ensure!(!text.contains(&outside_path), "拒绝文案不应回显绝对路径：{text}");

        // 超上限：先看长度再读，直接拒绝。
        let (text, _) = call_text(&read, &ctx, serde_json::json!({"path": "big.txt"})).await;
        ensure!(text.contains("文件超过读取上限"), "超上限文件应被拒：{text}");

        // 二进制：UTF-8 校验失败。
        let (text, _) = call_text(&read, &ctx, serde_json::json!({"path": "binary.bin"})).await;
        ensure!(text == "工具执行失败：二进制文件不支持读取", "二进制文案不符：{text}");
        notes.push(format!(
            "file_read：a.txt 读取成功，越界/超限/二进制均被拒（{FILE_MAX_BYTES} 字节上限）"
        ));

        // 搜索：排除目录在遍历期剪枝，结果按路径排序。
        let search = agent_tool::builtin::FileSearch::new(Vec::new());
        let (text, error) = call_text(&search, &ctx, serde_json::json!({"name": "skip"})).await;
        ensure!(!error, "搜索本身应成功：{text}");
        ensure!(
            text.starts_with("没有匹配的文件"),
            "排除目录内的文件不应出现在结果里：{text}"
        );
        let (text, error) = call_text(&search, &ctx, serde_json::json!({"name": "KEEP"})).await;
        ensure!(
            !error && text.contains("nested/keep.txt"),
            "应命中 nested/keep.txt：{text}"
        );
        ensure!(!text.contains("target/"), "排除目录不应出现在结果里：{text}");
        let (text, _) = call_text(&search, &ctx, serde_json::json!({"name": "a", "max_results": 0})).await;
        ensure!(
            text == "参数不合法：max_results 必须大于 0",
            "非法 max_results 的文案不符：{text}"
        );
        notes.push("file_search：target 被剪枝，KEEP 命中 nested/keep.txt（忽略大小写）".to_string());

        Ok(notes)
    })
}

/// 只回固定几个响应的 HTTP 假服务端：`/ok` 200、`/redirect` 302、其余 404。
struct StubServer {
    host: String,
    hits: Arc<AtomicU64>,
    accept: tokio::task::JoinHandle<()>,
}

impl StubServer {
    async fn start() -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.context("绑定假服务端失败")?;
        let host = listener.local_addr().context("读取假服务端地址失败")?.to_string();
        let hits = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&hits);
        let accept = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(async move {
                    let mut buffer = [0u8; 2048];
                    // 读到请求头就够判断路径了；读不到就直接回 404。
                    let path = match socket.read(&mut buffer).await {
                        Ok(read) => request_path(&buffer[..read]),
                        Err(_) => String::new(),
                    };
                    let (status, body, extra) = match path.as_str() {
                        "/ok" => ("200 OK", "stub-body", String::new()),
                        "/redirect" => (
                            "302 Found",
                            "",
                            "location: http://127.0.0.1:1/elsewhere\r\n".to_string(),
                        ),
                        _ => ("404 Not Found", "missing", String::new()),
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\ncontent-type: text/plain\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                });
            }
        });
        Ok(Self { host, hits, accept })
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.host)
    }

    fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }
}

impl Drop for StubServer {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

/// 请求行里的路径（`GET /ok HTTP/1.1` → `/ok`）。
fn request_path(head: &[u8]) -> String {
    let text = String::from_utf8_lossy(head);
    text.lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_string()
}

/// `http_request`：默认拒绝、白名单内可访问、白名单外一个字节都不发、302 不跟随。
fn http(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let stub = StubServer::start().await?;
        let mut notes = Vec::new();

        // 默认拒绝来自风险等级，不看白名单：`Network` 的缺省权限就是 deny。
        let policy = ToolPolicy::empty();
        ensure!(
            policy.permission_of("http_request", ToolRisk::Network) == ToolPermission::Deny,
            "Network 风险的工具默认应为 deny"
        );
        ensure!(
            policy.permission_of("calculator", ToolRisk::ReadOnly) == ToolPermission::Allow,
            "ReadOnly 风险的工具默认应为 allow"
        );

        let tool = agent_tool::builtin::HttpRequestTool::new(vec![stub.host.clone()], 1024);
        let home = temp_directory("tool-http")?;
        let ctx = context_of(&home);

        let (text, error) = call_text(&tool, &ctx, serde_json::json!({"url": stub.url("/ok")})).await;
        ensure!(!error, "白名单内应能访问：{text}");
        ensure!(
            text.contains("状态码：200") && text.contains("stub-body"),
            "响应应带状态码、内容类型与正文：{text}"
        );

        // 白名单外：拒绝且不建立连接。
        let before = stub.hits();
        let (text, error) = call_text(&tool, &ctx, serde_json::json!({"url": "http://example.com/anything"})).await;
        ensure!(error, "白名单外应被拒");
        ensure!(
            text == "工具执行失败：请求的主机不在允许的范围内",
            "越界文案不符：{text}"
        );
        ensure!(stub.hits() == before, "越界请求不应建立任何连接");

        // 302：原样返回状态码，不跟随重定向。
        let (text, error) = call_text(&tool, &ctx, serde_json::json!({"url": stub.url("/redirect")})).await;
        ensure!(!error, "302 本身是一次成功调用：{text}");
        ensure!(text.contains("状态码：302"), "3xx 应原样返回状态码而不是跟随：{text}");

        // 非 http/https 与非法方法：参数与失败各自的固定文案。
        let (text, _) = call_text(&tool, &ctx, serde_json::json!({"url": "ftp://example.com/x"})).await;
        ensure!(text == "工具执行失败：只支持 http / https 地址", "协议文案不符：{text}");
        let (text, _) = call_text(
            &tool,
            &ctx,
            serde_json::json!({"url": stub.url("/ok"), "method": "DELETE"}),
        )
        .await;
        ensure!(text == "参数不合法：method 只支持 GET 或 POST", "方法文案不符：{text}");

        notes.push(format!("http_request：假服务端命中 {} 次，302 未跟随", stub.hits()));
        Ok(notes)
    })
}

/// 工具自述：五个内置工具的本地文件操作声明与并发推导逐项核对（T-8）。
fn decl(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        // 四条映射：写类串行，其余可并行。
        for (access, expected) in [
            (ToolFileAccess::None, ToolConcurrency::Parallel),
            (ToolFileAccess::Read, ToolConcurrency::Parallel),
            (ToolFileAccess::Write, ToolConcurrency::Exclusive),
            (ToolFileAccess::ReadWrite, ToolConcurrency::Exclusive),
        ] {
            ensure!(
                access.default_concurrency() == expected,
                "{access:?} 的并发默认值应为 {expected:?}"
            );
        }

        let calculator = agent_tool::builtin::Calculator;
        let current_time = agent_tool::builtin::CurrentTime;
        let file_read = agent_tool::builtin::FileRead::new(Vec::new(), FILE_MAX_BYTES);
        let file_search = agent_tool::builtin::FileSearch::new(Vec::new());
        let http = agent_tool::builtin::HttpRequestTool::new(Vec::new(), 1024);

        let declarations: [(&str, ToolFileAccess, ToolRisk); 5] = [
            (calculator.name(), calculator.file_access(), calculator.risk()),
            (current_time.name(), current_time.file_access(), current_time.risk()),
            (file_read.name(), file_read.file_access(), file_read.risk()),
            (file_search.name(), file_search.file_access(), file_search.risk()),
            (http.name(), http.file_access(), http.risk()),
        ];
        let expected = [
            ("calculator", ToolFileAccess::None, ToolRisk::ReadOnly),
            ("current_time", ToolFileAccess::None, ToolRisk::ReadOnly),
            ("file_read", ToolFileAccess::Read, ToolRisk::ReadOnly),
            ("file_search", ToolFileAccess::Read, ToolRisk::ReadOnly),
            ("http_request", ToolFileAccess::None, ToolRisk::Network),
        ];
        for ((name, access, risk), (want_name, want_access, want_risk)) in declarations.iter().zip(expected) {
            ensure!(*name == want_name, "工具名不符：{name} != {want_name}");
            ensure!(*access == want_access, "{name} 的 fileAccess 不符：{access:?}");
            ensure!(*risk == want_risk, "{name} 的风险等级不符：{risk:?}");
        }
        ensure!(
            calculator.concurrency() == ToolConcurrency::Parallel
                && file_read.concurrency() == ToolConcurrency::Parallel,
            "读类工具的并发默认值应由声明推导"
        );

        // 注册表装配：五个内置工具全部注册，模型可见清单默认摘掉 http_request。
        let mut registry = ToolRegistry::new(ToolPolicy::empty());
        agent_tool::builtin::register_all(&mut registry, &agent_tool::builtin::BuiltinOptions::default())
            .context("注册内置工具失败")?;
        ensure!(registry.len() == 5, "应注册五个内置工具，实际 {}", registry.len());
        let visible: Vec<String> = registry
            .model_definitions()
            .iter()
            .map(|definition| definition.name.clone())
            .collect();
        ensure!(
            visible == ["calculator", "current_time", "file_read", "file_search"],
            "默认可见清单不符：{visible:?}"
        );

        Ok(vec![
            "fileAccess：None / None / Read / Read / None；并发：Parallel / Parallel / Parallel / Parallel / Parallel"
                .to_string(),
            format!("注册表 {} 个工具，默认可见 {visible:?}", registry.len()),
        ])
    })
}
