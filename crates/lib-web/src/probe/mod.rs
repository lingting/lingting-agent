//! 技术验证探针 API（P1-004 / P1-009）。
//!
//! 用于确认 `web_api` 宏在真实工程中的行为边界：路径、参数提取、返回值形态与错误响应，
//! 并保留作为 P1-005 / P1-006 生成 SDK 后的联调基线；P1-009 起同时承载错误响应样例与
//! `health` / `version` 探活接口。
//!
//! 本模块**不是业务 API**：业务 API 自 P1-010 起在 `crate::session`，探针接口作为错误响应
//! 样例、错误码回归基线与客户端探活入口保留（结论见 D-025 / D-024）。

pub mod api;
pub mod dto;

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// 服务版本，与 workspace 版本一致。
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// API 协议版本：接口契约发生不兼容变更时递增。
pub const PROTOCOL_VERSION: &str = "1";

/// 服务启动时刻。
#[derive(Debug, Clone, Copy)]
pub struct Startup {
    started_at_ms: i64,
    started: Instant,
}

impl Startup {
    fn now() -> Self {
        Self {
            started_at_ms: now_millis(),
            started: Instant::now(),
        }
    }

    /// 启动时刻的 Unix 毫秒时间戳。
    pub fn started_at_ms(&self) -> i64 {
        self.started_at_ms
    }

    /// 从启动到现在的毫秒数。
    pub fn uptime_ms(&self) -> i64 {
        i64::try_from(self.started.elapsed().as_millis()).unwrap_or(i64::MAX)
    }
}

static STARTUP: OnceLock<Startup> = OnceLock::new();

/// 记录服务启动时刻；重复调用保留最早记录的一次。
pub fn mark_startup() {
    let _ = STARTUP.set(Startup::now());
}

/// 取服务启动时刻；未显式记录时以首次访问作为启动时刻（探针示例服务）。
pub fn startup() -> Startup {
    *STARTUP.get_or_init(Startup::now)
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
