//! 失败态用例：`run_failed` 的脱敏原因落在消息末尾与提示行，界面不卡在流式阶段。

use super::{SCREEN_HEIGHT, SCREEN_WIDTH, STREAM_TIMEOUT, draft, open};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::ensure;
use lib_bin_cli::sdk::MessageRole;
use lib_bin_tui::testing::{Harness, KeyCode, KeyModifiers, Phase};

/// 认证失败的脱敏文案（与 `agent-route` 的 `public_message()` 逐字一致）。
const AUTH_ERROR: &str = "调用模型失败：认证失败";

pub(super) fn cases() -> Vec<Case> {
    vec![Case {
        name: "tui.run_failed",
        kind: CaseKind::Shared,
        prep: &[],
        run: run_failed,
    }]
}

/// 认证失败：等待执行提示可见、提示行给出脱敏错误、消息末尾挂同一文案，收尾后仍可输入。
fn run_failed(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        // `run_started` 之前（Run 还在排队或刚开始）状态栏给出等待提示；这一段只驱动状态机。
        let mut waiting = Harness::offline(context.base_url().to_string());
        waiting.type_text("排队提示");
        waiting.press(KeyCode::Enter, KeyModifiers::NONE);
        ensure!(
            waiting.app().phase == Phase::Waiting,
            "发送后应进入等待执行阶段，实际 {:?}",
            waiting.app().phase
        );
        let screen = waiting.screen(SCREEN_WIDTH, SCREEN_HEIGHT);
        ensure!(
            screen.contains("等待执行"),
            "run_started 之前状态栏应给出等待提示：\n{screen}"
        );

        let mut harness = open(context).await?;
        let content = format!("mock:auth TUI 失败 {}", context.token());
        harness.type_text(&content);
        harness.press(KeyCode::Enter, KeyModifiers::NONE);

        ensure!(
            harness
                .wait_until_async(STREAM_TIMEOUT, |app| app.phase == Phase::Idle && app.notice.is_some())
                .await,
            "失败在 {} 秒内未收尾",
            STREAM_TIMEOUT.as_secs()
        );
        let notice = harness.app().notice.clone().expect("失败应给出一行提示");
        ensure!(notice.contains(AUTH_ERROR), "提示行应给出脱敏错误：{notice}");

        let last = harness.app().messages.last().expect("失败后应有一条助手消息承载原因");
        ensure!(last.role == MessageRole::Assistant, "末尾应为助手消息");
        ensure!(last.content.is_empty(), "失败的 Run 不应写正文，实际：{}", last.content);
        let note = last.note.clone().expect("助手消息末尾应挂脱敏原因");
        ensure!(note.contains(AUTH_ERROR), "消息末尾应为脱敏文案，实际：{note}");
        ensure!(!last.interrupted, "执行失败不是连接中断，不应带中断标记");

        // 渲染：提示行与消息末尾都能看到同一条脱敏文案，且界面回到可输入状态。
        let screen = harness.screen(SCREEN_WIDTH, SCREEN_HEIGHT);
        ensure!(screen.contains("执行失败"), "渲染结果缺少失败说明：\n{screen}");
        ensure!(screen.contains(AUTH_ERROR), "渲染结果缺少脱敏错误：\n{screen}");
        harness.type_text("失败后继续");
        ensure!(
            draft(&harness) == "失败后继续",
            "失败后输入框应可用：{}",
            draft(&harness)
        );

        let result = vec![
            format!("提示行：{notice}"),
            format!("消息末尾：{note}"),
            "失败后界面回到空闲且可继续输入".to_string(),
        ];
        harness.close().await;
        Ok(result)
    })
}
