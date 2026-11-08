//! 粘贴用例：CRLF 拆成多行，会话列表焦点下粘贴被忽略。

use super::{draft, offline};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::ensure;
use lib_bin_tui::testing::{Focus, KeyCode, KeyModifiers};

pub(super) fn cases() -> Vec<Case> {
    vec![Case {
        name: "tui.paste",
        kind: CaseKind::Shared,
        run: paste,
    }]
}

fn paste(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut harness = offline(context);

        // 粘贴整体进输入组件，`\r\n` 拆成两行。
        harness.paste("第一行\r\n第二行");
        ensure!(harness.app().input.lines().len() == 2, "粘贴应拆成两行");
        ensure!(
            draft(&harness) == "第一行\n第二行",
            "粘贴结果不一致：{}",
            draft(&harness)
        );

        // 会话列表没有输入焦点：粘贴不写入输入框。
        harness.press(KeyCode::Tab, KeyModifiers::NONE);
        ensure!(harness.app().focus == Focus::Sessions, "Tab 应把焦点切到会话列表");
        harness.paste("粘贴");
        ensure!(
            draft(&harness) == "第一行\n第二行",
            "会话列表焦点下粘贴改动了输入框：{}",
            draft(&harness)
        );

        Ok(vec![
            "粘贴整体进输入框，CRLF 拆成两行".to_string(),
            "会话列表焦点下粘贴被忽略".to_string(),
        ])
    })
}
