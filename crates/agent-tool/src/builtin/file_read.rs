//! `file_read`：在白名单内读取文本文件（P3-023，D-P3-013）。
//!
//! 顺序固定：白名单解析 → 必须是普通文件 → `metadata().len()` 预判大小 → 读入 → UTF-8 校验。
//! 超过上限的文件**不读进内存**，二进制文件直接拒绝：模型拿到的要么是可读文本，要么是一句
//! 说清原因的固定文案。

use std::path::PathBuf;
use std::sync::LazyLock;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::builtin::path;
use crate::context::ToolContext;
use crate::definition::{ToolFileAccess, ToolRisk};
use crate::error::ToolError;
use crate::output::ToolOutput;
use crate::schema::{self, ToolSchema};
use crate::tool::Tool;

/// `file_read` 的参数。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileReadArgs {
    /// 要读取的文件路径：相对路径基于应用根目录，绝对路径必须落在白名单内。
    pub path: String,
}

/// 参数 schema：由 [`FileReadArgs`] 派生一次，进程内只算一遍（D-P3-002）。
static SCHEMA: LazyLock<ToolSchema> = LazyLock::new(schema::of::<FileReadArgs>);

/// 文本文件读取工具。
pub struct FileRead {
    allow_dirs: Vec<PathBuf>,
    max_bytes: u64,
}

impl FileRead {
    /// 装配工具：白名单与大小上限来自 `[tools.file]`（D-P3-013）。
    pub fn new(allow_dirs: Vec<PathBuf>, max_bytes: u64) -> Self {
        Self { allow_dirs, max_bytes }
    }
}

#[async_trait::async_trait]
impl Tool for FileRead {
    fn name(&self) -> &'static str {
        "file_read"
    }

    fn description(&self) -> &'static str {
        "读取白名单目录内的文本文件，返回文件内容。超过大小上限的文件与二进制文件会被拒绝。"
    }

    fn schema(&self) -> &'static ToolSchema {
        &SCHEMA
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn file_access(&self) -> ToolFileAccess {
        ToolFileAccess::Read
    }

    async fn call(&self, ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        let args: FileReadArgs = schema::parse(args)?;
        let resolved = path::resolve(&ctx.home, &self.allow_dirs, &args.path)?;
        // 同步 IO：一次读取最多 `max_bytes`（缺省 1 MiB），单次阻塞远小于一次模型往返。
        let metadata = std::fs::metadata(&resolved.path).map_err(|_| unavailable())?;
        if !metadata.is_file() {
            return Err(ToolError::Failed("该路径不是普通文件".to_owned()));
        }
        // 先看长度再读：超限文件一次字节都不读（D-P3-013）。
        if metadata.len() > self.max_bytes {
            return Err(ToolError::Failed(format!(
                "文件超过读取上限（> {} 字节）",
                self.max_bytes
            )));
        }
        let bytes = std::fs::read(&resolved.path).map_err(|_| unavailable())?;
        let text = String::from_utf8(bytes).map_err(|_| ToolError::Failed("二进制文件不支持读取".to_owned()))?;
        Ok(ToolOutput::ok(format!("文件：{}\n{text}", resolved.relative())))
    }
}

/// 读失败的统一文案：不回显绝对路径（D-P3-013）。
fn unavailable() -> ToolError {
    ToolError::Failed(path::REJECTED_TEXT.to_owned())
}
