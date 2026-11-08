//! TUI 用例：用无终端驱动跑界面状态机、工作线程与渲染。
//!
//! 子模块按断言对象拆分：输入编辑、粘贴、渲染定位、界面命令、发送、退出键与流式闭环；
//! 只有需要服务端参与的用例才起工作线程，纯状态与渲染断言走 [`Harness::offline`]。

mod commands;
mod connection;
mod input;
mod keys;
mod paste;
mod render;
mod send;
mod stream;

use crate::case::Case;
use crate::context::TestContext;
use anyhow::{Result, bail, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiClient, RunGetParams, RunStatus};
use lib_bin_tui::testing::{Connection, Harness};
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// 用例渲染帧的宽度与高度：宽到能放下底部快捷键提示。
const SCREEN_WIDTH: u16 = 140;
const SCREEN_HEIGHT: u16 = 30;
/// 探活与首屏的上限。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 一次完整回复的上限：Mock 每 120ms 推一段增量。
const STREAM_TIMEOUT: Duration = Duration::from_secs(20);
/// 退出后取消落地的上限。
const CANCEL_TIMEOUT: Duration = Duration::from_secs(10);
/// 一次界面内动作（新建会话、重连）的等待上限。
const STEP_TIMEOUT: Duration = Duration::from_secs(10);
/// 轮询间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// TUI 领域用例；排序由上层统一处理。
pub(crate) fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    cases.extend(input::cases());
    cases.extend(paste::cases());
    cases.extend(render::cases());
    cases.extend(commands::cases());
    cases.extend(send::cases());
    cases.extend(keys::cases());
    cases.extend(connection::cases());
    cases.extend(stream::cases());
    cases
}

/// 只建界面状态、不起工作线程：按键迁移与渲染的用例走这条路，不产生网络请求。
fn offline(context: &TestContext) -> Harness {
    Harness::offline(context.base_url().to_string())
}

/// 轮询直到服务端把 Run 落成已取消；跑到完成视为取消未生效。
async fn wait_run_cancelled(context: &TestContext, run_id: &str) -> Result<()> {
    let deadline = Instant::now() + CANCEL_TIMEOUT;
    loop {
        let run = context
            .client()
            .run_get(RunGetParams {
                run_id: run_id.to_string(),
            })
            .await
            .map_err(ClientError::from_call)?;
        match run.status {
            RunStatus::Cancelled => return Ok(()),
            RunStatus::Completed => bail!("Run 在取消生效前跑到了完成"),
            _ => {}
        }
        if Instant::now() >= deadline {
            bail!(
                "Run 在 {} 秒内未落成已取消，当前状态 {:?}",
                CANCEL_TIMEOUT.as_secs(),
                run.status
            );
        }
        sleep(POLL_INTERVAL).await;
    }
}

/// 起工作线程并等到探活成功。
fn open(context: &TestContext) -> Result<Harness> {
    let mut harness = Harness::start(context.base_url().to_string())?;
    ensure!(
        harness.wait_until(CONNECT_TIMEOUT, |app| matches!(
            app.connection,
            Connection::Online { .. }
        )),
        "界面在 {} 秒内未连上服务端",
        CONNECT_TIMEOUT.as_secs()
    );
    Ok(harness)
}

/// 输入框当前内容，与发送时的取文本规则一致。
fn draft(harness: &Harness) -> String {
    harness.app().input.lines().join("\n")
}
