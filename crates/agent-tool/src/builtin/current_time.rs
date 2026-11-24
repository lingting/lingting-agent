//! `current_time`：只读系统时钟，输出 ISO8601 与 Unix 毫秒（P3-022，D-P3-016）。
//!
//! 只支持 `utc` / `local` 两个取值：任意时区名需要 tzdata，属于「模型自己能算」的范畴，
//! 不值得为它新增依赖。毫秒取 `lib_core::time::now_millis`，与 `created_at_ms` 同源。

use std::sync::LazyLock;

use chrono::{DateTime, Local, SecondsFormat};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::context::ToolContext;
use crate::definition::{ToolFileAccess, ToolRisk};
use crate::error::ToolError;
use crate::output::ToolOutput;
use crate::schema::{self, ToolSchema};
use crate::tool::Tool;

/// 缺省时区：UTC（D-P3-016）。
const DEFAULT_ZONE: &str = "utc";

/// `current_time` 的参数。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CurrentTimeArgs {
    /// 时区：`utc`（缺省）或 `local`。
    pub zone: Option<String>,
}

/// 参数 schema：由 [`CurrentTimeArgs`] 派生一次，进程内只算一遍（D-P3-002）。
static SCHEMA: LazyLock<ToolSchema> = LazyLock::new(schema::of::<CurrentTimeArgs>);

/// 系统时钟工具。
#[derive(Debug, Default, Clone, Copy)]
pub struct CurrentTime;

#[async_trait::async_trait]
impl Tool for CurrentTime {
    fn name(&self) -> &'static str {
        "current_time"
    }

    fn description(&self) -> &'static str {
        "读取当前时间，返回 ISO8601 时间与 Unix 毫秒时间戳。时区只支持 utc（缺省）与 local。"
    }

    fn schema(&self) -> &'static ToolSchema {
        &SCHEMA
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn file_access(&self) -> ToolFileAccess {
        // 只读系统时钟，不碰本地文件（D-P3-021）。
        ToolFileAccess::None
    }

    async fn call(&self, _ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        let args: CurrentTimeArgs = schema::parse(args)?;
        let zone = args
            .zone
            .unwrap_or_else(|| DEFAULT_ZONE.to_owned())
            .to_ascii_lowercase();
        // 不做时区名解析：`Asia/Shanghai` 之类一律按「参数不合法」拒绝（D-P3-016）。
        if zone != "utc" && zone != "local" {
            return Err(ToolError::InvalidArguments("zone 只支持 utc 或 local".to_owned()));
        }
        let millis = lib_core::time::now_millis();
        let Some(instant) = DateTime::from_timestamp_millis(millis) else {
            return Err(ToolError::Failed("系统时间不可用".to_owned()));
        };
        let text = if zone == "local" {
            instant
                .with_timezone(&Local)
                .to_rfc3339_opts(SecondsFormat::Millis, false)
        } else {
            instant.to_rfc3339_opts(SecondsFormat::Millis, true)
        };
        Ok(ToolOutput::ok(format!("{text}（Unix 毫秒：{millis}，时区：{zone}）")))
    }
}
