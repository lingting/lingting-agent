//! CLI 异步运行时：客户端命令共用同一个阻塞入口。
//!
//! `run` 与 `status` 都是一次性交互，各自建一个多线程运行时即可，不需要跨命令复用。

use anyhow::{Context, Result};
use std::future::Future;

/// 在独立的多线程运行时上阻塞执行一个 future。
pub fn block_on<F: Future>(future: F) -> Result<F::Output> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("创建异步运行时失败")?;
    Ok(runtime.block_on(future))
}
