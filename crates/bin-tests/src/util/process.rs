//! 按 PID 操作进程的跨平台封装（P2.2-013）。
//!
//! 只为「用例派生了一个常驻子进程，收尾时必须确认它真的没了」服务：`server --async` 不提供
//! `--stop`（D-P2.2-004），用例只能靠 PID 文件拿到句柄。这里不引入第三方依赖，
//! 做法与平台一致：Windows 走 `taskkill` / `tasklist`，其他平台走 `kill`。

use anyhow::{Context, Result, bail};
use std::process::Command;
use std::time::{Duration, Instant};

/// 进程消失的轮询间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// 结束指定 PID 的进程：Windows 用 `taskkill /F`，其他平台用 `kill -9`。
///
/// 进程已经不在时命令也会返回非 0，因此这里只做「发出终止请求」，
/// 「是否真的没了」一律由 [`wait_gone`] 复核。
pub fn kill(pid: u32) -> Result<()> {
    #[cfg(windows)]
    let output = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output()
        .context("执行 taskkill 失败")?;
    #[cfg(not(windows))]
    let output = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .output()
        .context("执行 kill -9 失败")?;

    let _ = output;
    Ok(())
}

/// 进程是否仍在运行。
pub fn is_running(pid: u32) -> Result<bool> {
    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .context("执行 tasklist 失败")?;
        let listing = String::from_utf8_lossy(&output.stdout);
        Ok(listing
            .lines()
            .any(|line| line.split_whitespace().any(|field| field == pid.to_string())))
    }
    #[cfg(not(windows))]
    {
        let status = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .context("执行 kill -0 失败")?;
        Ok(status.success())
    }
}

/// 等进程消失：超时说明它没被结束掉，报错里带上 PID 便于人工回收。
///
/// 同步阻塞（只在用例收尾调用一次），不依赖任何异步运行时。
pub fn wait_gone(pid: u32, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while is_running(pid)? {
        if Instant::now() >= deadline {
            bail!("进程 {pid} 在 {} 秒内没有退出", timeout.as_secs());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    Ok(())
}
