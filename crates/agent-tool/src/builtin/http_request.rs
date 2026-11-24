//! `http_request`：受 host 白名单约束的 HTTP 调用（P3-025，D-P3-014）。
//!
//! 安全边界（这不是 SSRF 防护，README §七 第 9 条）：
//!
//! - 只允许 `http` / `https`，且 host 必须命中 `[tools.http] allow_hosts`；白名单为空即一律拒绝；
//! - 禁止跟随重定向：3xx 原样返回状态码，避免「白名单 host 愿意跳到哪里」变成缺口；
//! - 只允许 `GET` / `POST`；
//! - 响应体按 `max_bytes` 边读边截断（不整包进内存），只回灌状态码、内容类型与正文——
//!   不回灌响应头，避免把 `Set-Cookie` / `Authorization` 之类带进模型上下文。
//!
//! HTTP 客户端走 `framework-http`（`reqwest` 特性，D-P3-018），本 Crate 不直接依赖 `reqwest`。

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::context::ToolContext;
use crate::definition::{ToolFileAccess, ToolRisk};
use crate::error::ToolError;
use crate::output::ToolOutput;
use crate::schema::{self, ToolSchema};
use crate::tool::Tool;

/// 请求方法：只两条，足够「读取内容」与「提交 JSON」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Get,
    Post,
}

/// 连接失败 / 读超时的模型可见文本（`retryable` 为真的那一类，D-P3-008）。
const RETRYABLE_TEXT: &str = "连接失败或读取超时";
/// 其余请求失败的模型可见文本：不带 URL、不带底层错误原文（避免把地址与内部细节交给模型）。
const FAILED_TEXT: &str = "请求失败";

/// `http_request` 的参数。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HttpRequestArgs {
    /// 请求地址：只支持 http / https，host 必须命中白名单。
    pub url: String,
    /// 请求方法：`GET`（缺省）或 `POST`。
    pub method: Option<String>,
    /// 请求体：仅 `POST` 使用。
    pub body: Option<String>,
}

/// 参数 schema：由 [`HttpRequestArgs`] 派生一次，进程内只算一遍（D-P3-002）。
static SCHEMA: LazyLock<ToolSchema> = LazyLock::new(schema::of::<HttpRequestArgs>);

/// HTTP 调用工具。
pub struct HttpRequestTool {
    allow_hosts: Vec<String>,
    max_bytes: u64,
}

impl HttpRequestTool {
    /// 装配工具：白名单与响应上限来自 `[tools.http]`（D-P3-014）。
    pub fn new(allow_hosts: Vec<String>, max_bytes: u64) -> Self {
        Self { allow_hosts, max_bytes }
    }

    /// host 白名单：允许写 `host` 或 `host:port`，两者都按不区分大小写比较。
    fn allowed(&self, host: &str, port: Option<u16>) -> bool {
        let with_port = match port {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        self.allow_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(host) || allowed.eq_ignore_ascii_case(&with_port))
    }

    /// 请求失败的归一化：区分「值得重试」与「其它失败」，两者都不回显 URL。
    fn failure(&self, error: &anyhow::Error) -> ToolError {
        if is_retryable(error) {
            ToolError::Retryable(RETRYABLE_TEXT.to_owned())
        } else {
            ToolError::Failed(FAILED_TEXT.to_owned())
        }
    }
}

#[async_trait::async_trait]
impl Tool for HttpRequestTool {
    fn name(&self) -> &'static str {
        "http_request"
    }

    fn description(&self) -> &'static str {
        "向白名单内的地址发起 GET 或 POST 请求，返回状态码、内容类型与响应正文。默认不跟随重定向。"
    }

    fn schema(&self) -> &'static ToolSchema {
        &SCHEMA
    }

    fn risk(&self) -> ToolRisk {
        // 会主动向外部发起请求：默认拒绝，必须由 `[tools.permissions]` 显式 allow（D-P3-005）。
        ToolRisk::Network
    }

    fn file_access(&self) -> ToolFileAccess {
        // 只访问网络，不碰本地文件（D-P3-021）。
        ToolFileAccess::None
    }

    fn retryable(&self, error: &ToolError) -> bool {
        // 只有「连接失败 / 读超时」这一类值得重试（D-P3-008），且 `max_retries = 0` 时仍不重试。
        matches!(error, ToolError::Retryable(_))
    }

    async fn call(&self, ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        let args: HttpRequestArgs = schema::parse(args)?;
        let method = method_of(args.method.as_deref())?;
        let target =
            url::Url::parse(&args.url).map_err(|_| ToolError::InvalidArguments("url 不是合法的地址".to_owned()))?;
        if !matches!(target.scheme(), "http" | "https") {
            return Err(ToolError::Failed("只支持 http / https 地址".to_owned()));
        }
        let Some(host) = target.host_str() else {
            return Err(ToolError::InvalidArguments("url 缺少 host".to_owned()));
        };
        // 白名单判定在执行任何网络动作之前：越界请求一个字节都不发出去（P3-025 判据）。
        if !self.allowed(host, target.port()) {
            return Err(ToolError::Failed("请求的主机不在允许的范围内".to_owned()));
        }
        let client = self.client(ctx)?;
        let body = args.body.unwrap_or_default();
        let mut response = match method {
            Method::Get => client.stream_get(&args.url).await,
            Method::Post => client.stream_post(&args.url, body).await,
        }
        .map_err(|error| self.failure(&error))?;

        let status = response.status;
        let content_type = response.content_type().cloned().unwrap_or_else(|| "未知".to_owned());
        let (text, truncated) = read_body(&mut response, self.max_bytes, |error| self.failure(error)).await?;
        let mut report = format!("状态码：{status}\n内容类型：{content_type}\n\n{text}");
        if truncated {
            report.push_str(&format!("\n…（响应超过 {} 字节，已截断）", self.max_bytes));
        }
        Ok(ToolOutput::ok(report))
    }
}

impl HttpRequestTool {
    /// 按剩余时间建客户端：禁止重定向 + 本次调用的超时上限（D-P3-014）。
    ///
    /// 每次调用新建客户端（新的连接池）：超时来自本次调用的截止时刻，无法提前固化；
    /// http 工具一轮最多调一次，这点开销换「超时准确」是划算的。
    fn client(&self, ctx: &ToolContext) -> Result<framework_http::HttpClient, ToolError> {
        let remaining = ctx.deadline.saturating_duration_since(Instant::now());
        let timeout = Some(remaining.max(Duration::from_millis(1)));
        framework_http::HttpClient::with(|builder| builder.no_redirect().timeout(timeout))
            .map_err(|_| ToolError::Failed("初始化 HTTP 客户端失败".to_owned()))
    }
}

/// 方法解析：缺省 `GET`，大小写不敏感，其余一律参数非法。
fn method_of(requested: Option<&str>) -> Result<Method, ToolError> {
    let value = requested.unwrap_or("GET").to_ascii_uppercase();
    match value.as_str() {
        "GET" => Ok(Method::Get),
        "POST" => Ok(Method::Post),
        _ => Err(ToolError::InvalidArguments("method 只支持 GET 或 POST".to_owned())),
    }
}

/// 边读边累积，读到 `max_bytes` 就停：不把整个响应读进内存（D-P3-014）。
async fn read_body(
    response: &mut framework_http::HttpStreamResponse,
    max_bytes: u64,
    failure: impl Fn(&anyhow::Error) -> ToolError,
) -> Result<(String, bool), ToolError> {
    let limit = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let mut buffer: Vec<u8> = Vec::new();
    let mut truncated = false;
    while let Some(chunk) = response.next().await {
        let chunk = chunk.map_err(|error| failure(&error))?;
        let remaining = limit.saturating_sub(buffer.len());
        if chunk.len() > remaining {
            buffer.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        buffer.extend_from_slice(&chunk);
    }
    Ok((String::from_utf8_lossy(&buffer).into_owned(), truncated))
}

/// 是否值得重试：错误链里出现「超时 / 连接类」的 IO 错误即算。
///
/// `framework-http` 把后端错误包成 `anyhow::Error`，`reqwest` 类型不出框架（D-P3-018），
/// 因此按 `std::io::ErrorKind` 判定而不是按错误原文猜。
fn is_retryable(error: &anyhow::Error) -> bool {
    use std::io::ErrorKind;

    error.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                ErrorKind::TimedOut
                    | ErrorKind::WouldBlock
                    | ErrorKind::ConnectionRefused
                    | ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::BrokenPipe
            )
        })
    })
}
