//! 业务标识生成：底层雪花 ID，对外统一为字符串。

use anyhow::Result;

/// 生成下一个业务标识。
///
/// 跨语言以字符串传输（D-018），因此这里直接把雪花 `i64` 转成 `String`，
/// 调用方不需要再关心数值精度问题。
pub fn next_id() -> Result<String> {
    Ok(framework_core::next_id()?.to_string())
}
