//! 客户端错误：把传输失败、服务端错误与解码失败收敛成一行可读消息。
//!
//! 分类依据只有两个：`framework-http` 返回的错误链（区分超时与连接失败）与服务端响应体
//! 里的 `ApiError`。上层命令不再判断错误细节，只负责打印 [`Display`] 文本。

use crate::sdk::ApiCallError;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io;

/// 客户端调用错误。
#[derive(Debug)]
pub enum ClientError {
    /// 连接失败：服务未启动、地址不可达或被拒绝。
    Connection { url: String, detail: String },
    /// 请求超时。
    Timeout { url: String },
    /// 其他传输错误（DNS、TLS、连接中断等）。
    Transport { url: String, detail: String },
    /// 服务返回非 2xx，携带服务端 `{code, message}` 中的 `message`。
    Status { code: u16, message: String },
    /// 响应体或事件帧无法解析。
    Decode { detail: String },
    /// 客户端不支持该请求形态（例如未知的 HTTP 方法、嵌套查询对象）。
    Unsupported { detail: String },
    /// 客户端初始化失败（构建 `framework-http` 客户端）。
    Setup { detail: String },
}

impl ClientError {
    /// 把生成 SDK 的调用错误统一成本客户端的错误类型。
    pub fn from_call(error: ApiCallError<Self>) -> Self {
        match error {
            ApiCallError::Transport(error) => error,
            ApiCallError::Format(error) => Self::Decode {
                detail: format!("接口载荷序列化失败：{error}"),
            },
        }
    }

    /// 构建 HTTP 客户端失败：只可能是代理配置非法，客户端不设代理时属于异常。
    pub fn setup(error: anyhow::Error) -> Self {
        Self::Setup {
            detail: format!("{error:#}"),
        }
    }

    /// 按错误链分类：连接被拒绝、超时、其他传输错误。
    ///
    /// 不依赖具体后端类型：`framework-http` 把后端错误原样包在 `anyhow` 里，连接失败看链上的
    /// `std::io::Error` 类型，超时同时认 `TimedOut` 与 `operation timed out` 文案（后者是
    /// reqwest 的固定英文文案，不随系统语言变化）。地址不可达等其他失败归入一般传输错误，
    /// 消息里仍带最内层原因。
    pub fn from_send_error(url: &str, error: anyhow::Error) -> Self {
        let detail = deepest_message(&error);
        let url = url.to_string();
        if has_io_kind(&error, io::ErrorKind::TimedOut) || detail.contains("operation timed out") {
            return Self::Timeout { url };
        }
        if has_io_kind(&error, io::ErrorKind::ConnectionRefused) {
            return Self::Connection { url, detail };
        }
        Self::Transport { url, detail }
    }
}

impl Display for ClientError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connection { url, detail } => {
                write!(formatter, "无法连接服务 {url}：{detail}（服务是否已启动？）")
            }
            Self::Timeout { url } => write!(formatter, "请求 {url} 超时"),
            Self::Transport { url, detail } => write!(formatter, "请求 {url} 失败：{detail}"),
            Self::Status { code, message } => write!(formatter, "服务返回错误 {code}：{message}"),
            Self::Decode { detail } => write!(formatter, "响应解析失败：{detail}"),
            Self::Unsupported { detail } => write!(formatter, "不支持的请求：{detail}"),
            Self::Setup { detail } => write!(formatter, "初始化 HTTP 客户端失败：{detail}"),
        }
    }
}

impl Error for ClientError {}

/// 取错误链里最深的一层消息，避免只看到“error sending request”这类外壳。
fn deepest_message(error: &anyhow::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if !text.is_empty() {
            message = text;
        }
        source = cause.source();
    }
    message
}

/// 错误链上是否存在给定类型的 I/O 错误。
fn has_io_kind(error: &anyhow::Error, kind: io::ErrorKind) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<io::Error>())
        .any(|io| io.kind() == kind)
}
