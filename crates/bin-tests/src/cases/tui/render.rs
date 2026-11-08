//! 渲染用例：输入框光标定位、焦点切换后不再定位、超长内容滚动与极小终端。

use super::offline;
use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::ensure;
use lib_bin_tui::testing::{KeyCode, KeyModifiers, Position};

pub(super) fn cases() -> Vec<Case> {
    vec![Case {
        name: "tui.render",
        kind: CaseKind::Shared,
        prep: &[],
        run: render,
    }]
}

fn render(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        // 输入框内容区左上角是 (1, 19)：状态栏 1 行、主体 17 行、上边框 1 行；
        // 光标在 "ab中文" 之后，显示宽度 6 列。
        let mut input = offline(context);
        input.paste("ab中文");
        let frame = input.render(80, 24);
        ensure!(
            frame.cursor == Some(Position { x: 7, y: 19 }),
            "80x24 下光标应停在输入框内容末尾，实际 {:?}",
            frame.cursor
        );

        // 焦点不在输入框时不设置终端光标，位置交给终端自己决定。
        input.press(KeyCode::Tab, KeyModifiers::NONE);
        let frame = input.render(80, 24);
        ensure!(frame.cursor.is_none(), "会话列表焦点下不应定位终端光标");

        // 内容超过框内 3 行可见高度时，光标停在内容区最后一行，不会越界到底部提示行。
        let mut scrolled = offline(context);
        scrolled.paste("第一行\n第二行\n第三行\n第四行\n第五行");
        let frame = scrolled.render(80, 24);
        ensure!(
            frame.cursor.map(|position| position.y) == Some(21),
            "输入框滚动后光标应停在内容区最后一行，实际 {:?}",
            frame.cursor
        );

        // 行数放不下全部固定高度时也不能 panic：光标只落在终端可见范围内。
        let mut tiny = offline(context);
        tiny.paste("很长的输入内容会软换行\n第二行\n第三行\n第四行");
        let frame = tiny.render(20, 6);
        ensure!(
            frame.cursor.is_none_or(|position| position.x < 20 && position.y < 6),
            "极小终端下光标越界：{:?}",
            frame.cursor
        );
        ensure!(!frame.text.is_empty(), "极小终端下仍应渲染出内容");

        Ok(vec![
            "输入框光标停在内容末尾，焦点切换后不再定位".to_string(),
            "输入框滚动后光标留在内容区，20x6 终端不越界".to_string(),
        ])
    })
}
