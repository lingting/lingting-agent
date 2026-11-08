//! 流式用例：首屏、发送、增量收完、消息落状态、回复真的渲染进消息区，以及运行中断线。

use super::{SCREEN_HEIGHT, SCREEN_WIDTH, STREAM_TIMEOUT, draft, open};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::ensure;
#[cfg(debug_assertions)]
use lib_bin_cli::client::ClientError;
#[cfg(debug_assertions)]
use lib_bin_cli::sdk::ApiClient;
use lib_bin_tui::testing::{KeyCode, KeyModifiers, Phase};

pub(super) fn cases() -> Vec<Case> {
    let mut cases = vec![Case {
        name: "tui.stream",
        kind: CaseKind::Shared,
        run: stream,
    }];
    // 关闭探针只在 debug 构建注册，断线用例靠它触发服务端关闭。
    #[cfg(debug_assertions)]
    cases.push(Case {
        name: "tui.interrupted",
        kind: CaseKind::Isolated,
        run: interrupted,
    });
    cases
}

fn stream(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut harness = open(context)?;

        // 首屏：状态栏已连接、消息区给出引导文案、底部提示带退出键。
        let screen = harness.screen(SCREEN_WIDTH, SCREEN_HEIGHT);
        ensure!(screen.contains("已连接"), "首屏状态栏应显示已连接：\n{screen}");
        ensure!(
            screen.contains("输入内容后按 Enter 开始对话"),
            "首屏缺少引导文案：\n{screen}"
        );
        ensure!(screen.contains("Ctrl+D 退出"), "首屏缺少退出提示：\n{screen}");

        let content = format!("TUI 用例 {}", context.token());
        harness.type_text(&content);
        ensure!(draft(&harness) == content, "键入内容不一致：{}", draft(&harness));
        harness.press(KeyCode::Enter, KeyModifiers::NONE);
        ensure!(draft(&harness).is_empty(), "发送后输入框应清空：{}", draft(&harness));

        // 流式增长：先等第一段增量，再等它继续变长。
        ensure!(
            harness.wait_until(STREAM_TIMEOUT, |app| app.phase == Phase::Streaming
                && app.messages.last().is_some_and(|message| !message.content.is_empty())),
            "回复在 {} 秒内未开始推送增量",
            STREAM_TIMEOUT.as_secs()
        );
        let first_chars = harness
            .app()
            .messages
            .last()
            .map(|message| message.content.chars().count())
            .unwrap_or(0);

        // 流式期间按键照常生效：输入框能继续输入。
        harness.type_text("草稿");
        ensure!(
            draft(&harness) == "草稿",
            "流式期间输入框应能继续输入：{}",
            draft(&harness)
        );

        ensure!(
            harness.wait_until(STREAM_TIMEOUT, |app| app.messages.last().is_some_and(|message| message
                .content
                .chars()
                .count()
                > first_chars)),
            "回复在 {} 秒内未见增量增长",
            STREAM_TIMEOUT.as_secs()
        );

        // 收完这一轮回复：仍是用户 + 助手两条消息。
        ensure!(
            harness.wait_until(STREAM_TIMEOUT, |app| app.phase == Phase::Idle
                && app.messages.len() == 2
                && !app.messages[1].content.is_empty()),
            "界面在 {} 秒内未完成一次回复",
            STREAM_TIMEOUT.as_secs()
        );

        let messages = &harness.app().messages;
        ensure!(
            messages[0].content == content,
            "用户消息内容不一致：{}",
            messages[0].content
        );
        ensure!(
            messages[1].content.contains(&content),
            "助手回复未回显用户消息：{}",
            messages[1].content
        );
        ensure!(!messages[1].interrupted, "正常结束的回复不应带中断标记");
        let reply_chars = messages[1].content.chars().count();

        // 增量落进消息区：助手角色标签与 Mock 回复正文都要真的渲染出来。
        let screen = harness.screen(SCREEN_WIDTH, SCREEN_HEIGHT);
        ensure!(screen.contains("助手"), "渲染结果缺少助手消息：\n{screen}");
        ensure!(screen.contains("本段由 P1 的 Mock"), "渲染结果缺少回复正文：\n{screen}");

        Ok(vec![
            format!("会话：{}", harness.app().current.as_deref().unwrap_or("-")),
            format!("用户消息：{}", messages[0].content),
            format!("助手回复：{reply_chars} 字符"),
            format!("渲染帧：{SCREEN_WIDTH}x{SCREEN_HEIGHT}"),
        ])
    })
}

/// 运行中断线（服务关闭）：保留已收到的增量、标注中断、给出提示，且界面不崩溃。
#[cfg(debug_assertions)]
fn interrupted(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut harness = open(context)?;
        harness.type_text(&format!("断线 {}", context.token()));
        harness.press(KeyCode::Enter, KeyModifiers::NONE);

        // 等到已经收到增量：确认有内容需要保留。
        ensure!(
            harness.wait_until(STREAM_TIMEOUT, |app| app.phase == Phase::Streaming
                && app.messages.last().is_some_and(|message| !message.content.is_empty())),
            "回复在 {} 秒内未开始推送增量",
            STREAM_TIMEOUT.as_secs()
        );
        let partial = harness
            .app()
            .messages
            .last()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        ensure!(!partial.is_empty(), "触发关闭前应已收到部分增量");

        // 服务端进入关闭流程：事件流推一条 `server_shutdown` 后收尾。
        context
            .client()
            .probe_shutdown()
            .await
            .map_err(ClientError::from_call)?;

        ensure!(
            harness.wait_until(STREAM_TIMEOUT, |app| app.phase == Phase::Idle),
            "断线后界面应回到空闲"
        );
        let last = harness.app().messages.last().expect("应有助手消息");
        ensure!(last.interrupted, "断线的回复应带中断标记");
        ensure!(last.content.starts_with(&partial), "应保留已收到的增量");

        let notice = harness.app().notice.clone().expect("断线应给出一行提示");
        ensure!(notice.contains("连接中断"), "提示应说明连接中断：{notice}");
        ensure!(!harness.app().should_quit(), "断线不应退出");

        // 断线后仍能渲染与输入：状态栏未连接，消息区标注不完整，提示行给出原因。
        let screen = harness.screen(SCREEN_WIDTH, SCREEN_HEIGHT);
        ensure!(screen.contains("未连接"), "断线后状态栏应显示未连接：\n{screen}");
        ensure!(screen.contains("连接中断"), "提示行应显示连接中断：\n{screen}");
        ensure!(
            screen.contains("内容可能不完整"),
            "消息区应标注内容可能不完整：\n{screen}"
        );
        harness.type_text("断线后继续");
        ensure!(draft(&harness) == "断线后继续", "断线后输入框应可用");

        Ok(vec![
            format!("断线前已收到 {} 字符", partial.chars().count()),
            format!("提示行：{notice}"),
            "断线后保留内容、标注中断，渲染与输入仍可用".to_string(),
        ])
    })
}
