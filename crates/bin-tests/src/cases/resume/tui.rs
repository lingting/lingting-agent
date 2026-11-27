//! `resume.tui`（P6A-015 / 验收 `I-4`）：TUI 能看到「这个 Run 被中断、发消息即可继续」。
//!
//! 一段：强杀 → 重启 → 打开会话（界面自己去读最后一个 Run 的状态）→ 提示条出现 →
//! 输入框照旧可用 → 发一条消息 → 被恢复的旧 Run 与本次的新 Run 各占一条助手气泡。
//!
//! 断言只读界面状态与渲染文本（`Harness` 的帧），数字与文案都来自接口。

use std::time::Duration;

use anyhow::{Context, Result, ensure};
use lib_bin_cli::sdk::MessageRole;
use lib_bin_tui::testing::{Connection, Focus, Harness, KeyCode, KeyModifiers, Phase};

use crate::case::CaseFuture;
use crate::util::context::TestContext;
use crate::util::flow;

/// 界面渲染尺寸：宽到能放下状态栏与底部提示。
const SCREEN_WIDTH: u16 = 160;
const SCREEN_HEIGHT: u16 = 30;
/// 一次界面动作的等待上限。
const STEP_TIMEOUT: Duration = Duration::from_secs(20);
/// 一轮回复的上限（Mock 每 200ms 推一段增量）。
const STREAM_TIMEOUT: Duration = Duration::from_secs(40);

/// 用例入口。
pub(super) fn case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { require(context).await })
}

async fn require(context: &TestContext) -> Result<Vec<String>> {
    let server = super::start(context).await?;
    let session = flow::create_session(context, "TUI 中断态").await?;
    let (server, paused_id) = super::interrupt_running(context, server, &session.id).await?;

    let mut harness = Harness::start(context.base_url().to_string())?;
    ensure!(
        harness
            .wait_until_async(STEP_TIMEOUT, |app| matches!(app.connection, Connection::Online { .. }))
            .await,
        "界面在 {} 秒内未连上服务端",
        STEP_TIMEOUT.as_secs()
    );
    ensure!(
        harness
            .wait_until_async(STEP_TIMEOUT, |app| !app.sessions.is_empty())
            .await,
        "会话列表在 {} 秒内为空",
        STEP_TIMEOUT.as_secs()
    );

    // 打开这条会话：界面自己会去读最后一个 Run 的状态（P6A-015）。
    let index = harness
        .app()
        .sessions
        .iter()
        .position(|item| item.id == session.id)
        .context("会话应出现在列表里")?;
    harness.press(KeyCode::Tab, KeyModifiers::NONE);
    ensure!(harness.app().focus == Focus::Sessions, "Tab 应把焦点切到会话列表");
    for _ in 0..index {
        harness.press(KeyCode::Down, KeyModifiers::NONE);
    }
    harness.press(KeyCode::Enter, KeyModifiers::NONE);
    ensure!(
        harness
            .wait_until_async(STEP_TIMEOUT, |app| app.current.as_deref() == Some(session.id.as_str())
                && app.paused.is_some())
            .await,
        "打开会话后界面应显示中断态"
    );
    let (seen_run, reason) = {
        let paused = harness.app().paused.as_ref().context("应有中断态提示")?;
        (paused.run_id.clone(), paused.reason.clone())
    };
    ensure!(
        seen_run == paused_id,
        "中断态应指向被强杀的那个 Run：{paused_id} != {seen_run}"
    );

    // 渲染：状态栏带「已中断（发送消息可继续）」+ 原因原文。
    let screen = harness.screen(SCREEN_WIDTH, SCREEN_HEIGHT);
    ensure!(
        screen.contains("已中断（发送消息可继续）"),
        "状态栏应显示中断提示：\n{screen}"
    );
    ensure!(
        screen.contains(&reason),
        "状态栏应原样显示中断原因「{reason}」：
{screen}"
    );

    // 输入框照旧可用：直接发一条消息就会先把上次没跑完的 Run 恢复。
    harness.type_text("继续");
    harness.press(KeyCode::Enter, KeyModifiers::NONE);
    ensure!(
        harness
            .wait_until_async(STREAM_TIMEOUT, |app| app.paused.is_none() && app.phase == Phase::Idle)
            .await,
        "发送之后中断提示应消失并跑完（当前 phase = {:?}）",
        harness.app().phase
    );

    let assistants = harness
        .app()
        .messages
        .iter()
        .filter(|message| message.role == MessageRole::Assistant && !message.content.is_empty())
        .count();
    ensure!(
        assistants >= 2,
        "被恢复的旧 Run 与本次的新 Run 各应有一条助手回复，实际 {assistants} 条"
    );
    let recovered = flow::run_get(context, &paused_id).await?;
    ensure!(recovered.resume_count == 1, "被恢复的 Run 应只推进一次：{recovered:?}");

    harness.close().await;
    context.observe_metrics(server.metrics().await);
    server.stop().await?;
    Ok(vec![
        format!("打开会话即显示「已中断（发送消息可继续）：{reason}」（Run {paused_id}）"),
        format!("发一条消息后提示消失，两条 Run 各有一条助手回复（共 {assistants} 条），resumeCount = 1"),
    ])
}
