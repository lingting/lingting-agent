//! 发送用例：提交时去首尾空白并清空输入框，回复未结束时 Enter 不发送、只给提示。

use super::{STREAM_TIMEOUT, draft, open};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::ensure;
use lib_bin_tui::testing::{KeyCode, KeyModifiers, Phase};

pub(super) fn cases() -> Vec<Case> {
    vec![Case {
        name: "tui.send",
        kind: CaseKind::Shared,
        run: send,
    }]
}

fn send(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut harness = open(context)?;

        // 提交：内容去首尾空白，输入框清空。
        harness.paste("  你好  \n世界  ");
        harness.press(KeyCode::Enter, KeyModifiers::NONE);
        ensure!(draft(&harness).is_empty(), "发送后输入框应清空：{}", draft(&harness));
        ensure!(
            harness.app().phase != Phase::Idle,
            "发送后应进入等待或流式阶段，实际 {:?}",
            harness.app().phase
        );

        // 回复未结束时再按 Enter：不发送，输入保留并给出提示。
        harness.type_text("排队中的消息");
        harness.press(KeyCode::Enter, KeyModifiers::NONE);
        ensure!(
            draft(&harness) == "排队中的消息",
            "忙碌时应保留输入：{}",
            draft(&harness)
        );
        ensure!(harness.app().notice.is_some(), "忙碌时应给出提示");

        // 这一轮回复结束：只有用户与助手两条消息，排队中的内容没有发出去。
        ensure!(
            harness.wait_until(STREAM_TIMEOUT, |app| app.phase == Phase::Idle
                && app.messages.len() == 2
                && !app.messages[1].content.is_empty()),
            "回复未在 {} 秒内结束",
            STREAM_TIMEOUT.as_secs()
        );
        let messages = &harness.app().messages;
        ensure!(
            messages[0].content == "你好  \n世界",
            "发送内容应按去首尾空白后的原样保存：{:?}",
            messages[0].content
        );
        ensure!(harness.app().run_id.is_none(), "结束后不应留下进行中的 Run");

        Ok(vec![
            "提交时清空输入框，内容按去首尾空白的规则保存".to_string(),
            "回复未结束时 Enter 不发送，输入保留并给出提示".to_string(),
        ])
    })
}
