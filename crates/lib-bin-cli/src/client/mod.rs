//! 客户端：把生成的 SDK 接口接到 `framework-http` 上，并提供 Run 事件流订阅。
//!
//! 分工：
//!
//! - [`Client`] 实现生成 SDK 的 `ApiClient`，`send` 负责拼地址、加请求头、判定状态码；
//! - [`Client::subscribe_run`] 走流式读取：生成的 `RUN_STREAM` 在元数据里是 `Blob`，
//!   `run_stream` 只能整体返回响应体，无法边收边显示；
//! - 请求地址全部来自生成的 `ApiDefinition.path`，客户端不出现任何 path 字面量；
//! - HTTP 收发只经 `framework-http`，本 crate 不直接使用 reqwest 发请求。

mod error;
mod query;
mod stream;

pub use error::ClientError;
pub use stream::EventStream;

use crate::sdk::{ApiClient, ApiDefinition, ApiError, R, RUN_STREAM, RunStreamParams};
use framework_http::{HttpClient, HttpMethod, HttpRequest, HttpStreamResponse};
use serde_json::Value;
use std::time::Duration;

/// 普通接口的请求超时：状态查询、会话与消息操作都应在这个时间内返回。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// 服务端 API 客户端。
pub struct Client {
    /// 服务基地址，形如 `http://127.0.0.1:20100`。
    base_url: String,
    /// 普通接口用的 HTTP 客户端：带请求超时。
    http: HttpClient,
    /// 事件流用的 HTTP 客户端：不设总超时，长回复不会被截断。
    stream_http: HttpClient,
}

impl Client {
    /// 由服务基地址构建客户端。
    ///
    /// 两个客户端分工明确：普通接口带 30 秒总超时，事件流不设总超时（一次回复可能持续很久，
    /// 总超时会在回复中途截断；连接是否存活由服务端心跳与用户中断决定）。
    pub fn new(base_url: impl Into<String>) -> Result<Self, ClientError> {
        let http = HttpClient::with(|builder| builder.timeout(Some(REQUEST_TIMEOUT))).map_err(ClientError::setup)?;
        let stream_http = HttpClient::with(|builder| builder.no_timeout()).map_err(ClientError::setup)?;
        Ok(Self {
            base_url: base_url.into(),
            http,
            stream_http,
        })
    }

    /// 服务基地址。
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 订阅 Run 的事件流：只有 `QUEUED` 的 Run 可以订阅（D-026）。
    pub async fn subscribe_run(&self, params: RunStreamParams) -> Result<EventStream, ClientError> {
        let query = serde_json::to_vec(&params).map_err(|error| ClientError::Decode {
            detail: format!("查询参数序列化失败：{error}"),
        })?;
        let url = self.url(RUN_STREAM.path, Some(&query))?;
        let request = HttpRequest::new(url.clone()).with_accept("text/event-stream");
        let response = self
            .stream_http
            .stream(request)
            .await
            .map_err(|error| ClientError::from_send_error(&url, error))?;

        if !response.is_success() {
            return Err(error_message_of(url, response).await);
        }
        Ok(EventStream::new(url, response))
    }

    /// 拼接服务地址与接口 path，并按需附加查询串。
    fn url(&self, path: &str, query: Option<&[u8]>) -> Result<String, ClientError> {
        let mut url = format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        if let Some(query) = query {
            let query = query::encode(query)?;
            if !query.is_empty() {
                url.push('?');
                url.push_str(&query);
            }
        }
        Ok(url)
    }
}

impl ApiClient for Client {
    type Error = ClientError;

    async fn send(
        &self,
        definition: &ApiDefinition,
        body: Option<Vec<u8>>,
        query: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, Self::Error> {
        let method = method_of(definition.method)?;
        let url = self.url(definition.path, query.as_deref())?;
        let mut request = HttpRequest::new(url.clone()).with_accept("application/json");
        if let Some(body) = body {
            request = request.with_content_type("application/json").with_body(body);
        }

        let response = self
            .http
            .request(method, request)
            .await
            .map_err(|error| ClientError::from_send_error(&url, error))?;
        if !response.is_success() {
            return Err(error_message(response.status, &response.body));
        }
        Ok(unwrap_envelope(&response.body))
    }
}

/// 剥掉成功响应的 `R<T>` 包装，返回载荷 JSON。
///
/// 服务端成功响应统一为 `{"code":200,"message":"Success","data":<载荷>}`，而生成的 SDK
/// 按载荷类型直接反序列化，因此 Transport 必须在这里解包；`data` 缺失（`Void` 的 204 空响应）
/// 与非 JSON 响应体（`Blob`）原样返回。
fn unwrap_envelope(body: &[u8]) -> Vec<u8> {
    match serde_json::from_slice::<R<Value>>(body) {
        Ok(envelope) => match envelope.data {
            Some(data) => serde_json::to_vec(&data).unwrap_or_else(|_| body.to_vec()),
            None => body.to_vec(),
        },
        Err(_) => body.to_vec(),
    }
}

/// 接口元数据里的方法名转 `framework-http` 的方法。
fn method_of(method: &str) -> Result<HttpMethod, ClientError> {
    match method {
        "GET" => Ok(HttpMethod::Get),
        "POST" => Ok(HttpMethod::Post),
        "PUT" => Ok(HttpMethod::Put),
        "PATCH" => Ok(HttpMethod::Patch),
        "DELETE" => Ok(HttpMethod::Delete),
        "HEAD" => Ok(HttpMethod::Head),
        "OPTIONS" => Ok(HttpMethod::Options),
        other => Err(ClientError::Unsupported {
            detail: format!("未知的 HTTP 方法：{other}"),
        }),
    }
}

/// 读取失败响应的响应体并转成错误。
async fn error_message_of(url: String, mut response: HttpStreamResponse) -> ClientError {
    let status = response.status;
    let mut body = Vec::new();
    while let Some(chunk) = response.next().await {
        match chunk {
            Ok(chunk) => body.extend_from_slice(&chunk),
            Err(error) => return ClientError::from_send_error(&url, error),
        }
    }
    error_message(status, &body)
}

/// 由状态码与错误体构造错误：优先使用服务端 `{code, message}` 里的 `message`。
fn error_message(status: u16, body: &[u8]) -> ClientError {
    let message = serde_json::from_slice::<ApiError>(body)
        .map(|error| error.message)
        .unwrap_or_else(|_| String::from_utf8_lossy(body).into_owned());
    ClientError::Status {
        code: status,
        message: single_line(&message),
    }
}

/// 把错误体压成一行：错误呈现约定是「一行可读错误」，不能把 HTML 或多行文本原样灌到终端。
fn single_line(message: &str) -> String {
    /// 单行错误的最大字符数，超出部分截断。
    const MAX_CHARS: usize = 200;

    let text = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return "服务端未返回错误信息".to_string();
    }
    if text.chars().count() > MAX_CHARS {
        return text.chars().take(MAX_CHARS).collect::<String>() + "…";
    }
    text
}
