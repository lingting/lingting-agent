//! 场景选择与节奏参数。
//!
//! 场景按优先级取第一个命中的：最后一条消息里的关键字 `mock:<场景>`、查询参数
//! `scenario=<场景>`、路径前缀 `/mock/<场景>/`。关键字走的是真实 Provider 那条路，
//! 服务端只改 `[provider] base_url` 就能选中异常场景（P2-026 / P2-031）。

use serde_json::Value;
use std::time::Duration;

use super::http::Request;

/// 关键字前缀：最后一条消息里出现 `mock:<场景>` 即选中该场景。
const KEYWORD: &str = "mock:";
/// 默认分块间隔。
const DEFAULT_CHUNK_MS: u64 = 10;
/// 慢速分块场景的默认间隔。
const SLOW_CHUNK_MS: u64 = 200;
/// 静默 stall 场景的默认静默时长。
const DEFAULT_STALL_MS: u64 = 30_000;
/// 分块间隔上限，避免用例把等待写成分钟级。
const MAX_CHUNK_MS: u64 = 2_000;
/// 静默时长上限。
const MAX_STALL_MS: u64 = 120_000;

/// Mock 场景：正常 + 六种异常。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scenario {
    /// 正常：分块间隔按 `chunk_ms`，完整回复。
    Normal,
    /// 认证失败：401 + OpenAI 风格错误体。
    AuthFailure,
    /// 上游 5xx：503 + OpenAI 风格错误体。
    UpstreamError,
    /// 非法 JSON：200 但响应体不是 JSON。
    InvalidJson,
    /// 中途断连：写出一帧后直接关连接，没有 `[DONE]`。
    Disconnect,
    /// 静默 stall：写出一帧后静默，最后关连接，没有 `[DONE]`。
    Stall,
    /// 慢速分块：分块间隔默认 200ms。
    Slow,
}

impl Scenario {
    /// 场景名：查询参数、路径前缀与 `/mock/stats` 里的键都用它。
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::AuthFailure => "auth",
            Self::UpstreamError => "upstream",
            Self::InvalidJson => "invalid_json",
            Self::Disconnect => "disconnect",
            Self::Stall => "stall",
            Self::Slow => "slow",
        }
    }

    /// 场景名解析：接受主名与常见别名。
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "normal" | "ok" => Some(Self::Normal),
            "auth" | "auth_failure" | "unauthorized" => Some(Self::AuthFailure),
            "upstream" | "upstream_error" | "5xx" => Some(Self::UpstreamError),
            "invalid_json" | "bad_json" => Some(Self::InvalidJson),
            "disconnect" | "broken" => Some(Self::Disconnect),
            "stall" | "silent" => Some(Self::Stall),
            "slow" => Some(Self::Slow),
            _ => None,
        }
    }
}

/// 一次 chat 请求要执行的行为：场景 + 分块间隔 + 静默时长。
pub(crate) struct Plan {
    pub(crate) scenario: Scenario,
    pub(crate) chunk_interval: Duration,
    pub(crate) stall: Duration,
}

impl Plan {
    /// 解析场景与节奏参数。
    pub(crate) fn resolve(request: &Request, body: &Value) -> Self {
        let scenario = prompt_scenario(body)
            .or_else(|| request.query_value("scenario").and_then(Scenario::parse))
            .or_else(|| path_scenario(&request.path))
            .unwrap_or(Scenario::Normal);
        let default_chunk = if scenario == Scenario::Slow {
            SLOW_CHUNK_MS
        } else {
            DEFAULT_CHUNK_MS
        };
        let chunk_ms = milliseconds(request.query_value("chunk_ms"), default_chunk, MAX_CHUNK_MS);
        let stall_ms = milliseconds(request.query_value("stall_ms"), DEFAULT_STALL_MS, MAX_STALL_MS);
        Self {
            scenario,
            chunk_interval: Duration::from_millis(chunk_ms),
            stall: Duration::from_millis(stall_ms),
        }
    }
}

/// 最后一条消息里 `mock:<场景>` 的第一个命中。
fn prompt_scenario(body: &Value) -> Option<Scenario> {
    let content = body.get("messages")?.as_array()?.last()?.get("content")?.as_str()?;
    let start = content.find(KEYWORD)? + KEYWORD.len();
    let token: String = content[start..]
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
        .collect();
    Scenario::parse(&token)
}

/// 路径前缀 `/mock/<场景>/…`。
fn path_scenario(path: &str) -> Option<Scenario> {
    let rest = path.strip_prefix("/mock/")?;
    let (name, _) = rest.split_once('/')?;
    Scenario::parse(name)
}

/// 取毫秒参数：缺省或非法时用默认值，超过上限时截到上限。
fn milliseconds(value: Option<&str>, default: u64, max: u64) -> u64 {
    value
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
        .min(max)
}
