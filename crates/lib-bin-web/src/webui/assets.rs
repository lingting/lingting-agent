//! WebUI 资源来源：嵌入构建读编译期资源表，非嵌入构建读本地构建目录（D-009）。
//!
//! 是否嵌入由 `build.rs` 按 `PROFILE` 决定，并以 `webui_embed` cfg 交给这里，两侧判定同源。
//! 嵌入的资源表里文本资源已 gzip；非嵌入构建直接读 `lingting-agent-ui/dist`，前端改动不必重编 Rust。

use bytes::Bytes;
use tracing::info;

#[cfg(not(webui_embed))]
use std::path::PathBuf;
#[cfg(not(webui_embed))]
use tracing::warn;

/// 本地资源目录：非嵌入构建从这里读盘。
///
/// 用编译期路径而不是进程工作目录：测试与 IDE 启动的进程工作目录不固定。
#[cfg(not(webui_embed))]
const DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lingting-agent-ui/dist");

/// 内嵌资源表：只在嵌入构建时生成。
#[cfg(webui_embed)]
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

/// 一次资源读取结果。
pub(super) struct Asset {
    /// 响应字节：嵌入资源零拷贝，磁盘资源按需读取。
    pub body: Bytes,
    /// 字节是否为 gzip 压缩结果，为 true 时响应需带 `content-encoding: gzip`。
    pub gzip: bool,
}

/// 读取相对路径对应的资源；未嵌入的资源返回 `None`。
///
/// 资源表是编译期常量，按相对路径精确匹配，不存在目录穿越。
#[cfg(webui_embed)]
pub(super) fn read(relative: &str) -> Option<Asset> {
    let (bytes, gzip) = embedded::lookup(relative)?;

    Some(Asset {
        body: Bytes::from_static(bytes),
        gzip,
    })
}

/// 读取相对路径对应的资源；路径缺失或越界时返回 `None`。
#[cfg(not(webui_embed))]
pub(super) fn read(relative: &str) -> Option<Asset> {
    let path = PathBuf::from(DIRECTORY).join(safe_relative(relative)?);
    let body = std::fs::read(path).ok()?;

    Some(Asset {
        body: Bytes::from(body),
        gzip: false,
    })
}

/// 启动时记录内嵌资源数量：嵌入构建的产物必然带着资源，无需请求期兜底。
#[cfg(webui_embed)]
pub(super) fn log_ready() {
    info!(count = embedded::COUNT, "WebUI 静态资源已内嵌进二进制");
}

/// 启动时记录本地资源目录状态，避免资源缺失只能在请求期发现。
#[cfg(not(webui_embed))]
pub(super) fn log_ready() {
    let directory = PathBuf::from(DIRECTORY);
    if directory.join(super::INDEX).is_file() {
        info!(directory = %directory.display(), "WebUI 静态资源目录就绪");
    } else {
        warn!(
            directory = %directory.display(),
            "WebUI 静态资源缺失：前端未构建时访问前端路由会返回 503"
        );
    }
}

/// 把 URL 相对路径转换为磁盘相对路径，拒绝目录穿越与盘符前缀。
#[cfg(not(webui_embed))]
fn safe_relative(path: &str) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') || segment.contains(':') {
            return None;
        }
        relative.push(segment);
    }
    if relative.as_os_str().is_empty() {
        return None;
    }

    Some(relative)
}
