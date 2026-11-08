//! 输入编辑用例：逐字键入、按字素编辑、Ctrl+A 交给输入组件、Shift+Enter 换行、空白不发送。

use super::{draft, offline};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::ensure;
use lib_bin_tui::testing::{KeyCode, KeyModifiers, Phase};

pub(super) fn cases() -> Vec<Case> {
    vec![Case {
        name: "tui.input",
        kind: CaseKind::Shared,
        run: input,
    }]
}

fn input(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        // 编辑：逐字键入后按字素编辑，中文不会被切坏。
        let mut harness = offline(context);
        ensure!(draft(&harness).is_empty(), "初始输入应为空");

        harness.type_text("ab中文");
        ensure!(draft(&harness) == "ab中文", "键入内容不一致：{}", draft(&harness));

        harness.press(KeyCode::Left, KeyModifiers::NONE);
        harness.press(KeyCode::Backspace, KeyModifiers::NONE);
        ensure!(draft(&harness) == "ab文", "Backspace 未按字素删除：{}", draft(&harness));

        harness.press(KeyCode::Home, KeyModifiers::NONE);
        harness.press(KeyCode::Delete, KeyModifiers::NONE);
        ensure!(draft(&harness) == "b文", "Delete 未删掉光标处字符：{}", draft(&harness));

        harness.press(KeyCode::End, KeyModifiers::NONE);
        harness.type_text("结尾");
        ensure!(draft(&harness) == "b文结尾", "End 后键入位置不对：{}", draft(&harness));

        // Ctrl+A 应用没有占用，交给输入组件把光标移到行首。
        harness.press_ctrl(KeyCode::Char('a'));
        harness.type_text("X");
        ensure!(
            draft(&harness) == "Xb文结尾",
            "Ctrl+A 未交给输入组件：{}",
            draft(&harness)
        );

        // Shift+Enter 换行：只改输入框，不发送。光标先回到行尾（上一步 Ctrl+A 把它移到了行首）。
        harness.press(KeyCode::End, KeyModifiers::NONE);
        harness.press(KeyCode::Enter, KeyModifiers::SHIFT);
        harness.type_text("第二行");
        ensure!(harness.app().input.lines().len() == 2, "Shift+Enter 应新增一行");
        ensure!(
            draft(&harness) == "Xb文结尾\n第二行",
            "换行后的内容不一致：{}",
            draft(&harness)
        );
        ensure!(harness.app().phase == Phase::Idle, "Shift+Enter 不应发送");

        // 空白输入按 Enter 不发送，输入内容原样保留。
        let mut blank = offline(context);
        blank.paste("  \n\t ");
        blank.press(KeyCode::Enter, KeyModifiers::NONE);
        ensure!(blank.app().phase == Phase::Idle, "空白输入不应发送");
        ensure!(blank.app().run_id.is_none(), "空白输入不应创建 Run");
        ensure!(draft(&blank) == "  \n\t ", "空白输入应原样保留：{:?}", draft(&blank));

        Ok(vec![
            "逐字键入与按字素编辑（中文、Home/End、Delete）".to_string(),
            "Ctrl+A 作为编辑快捷键交给输入组件".to_string(),
            "Shift+Enter 换行不发送，空白输入 Enter 不发送".to_string(),
        ])
    })
}
