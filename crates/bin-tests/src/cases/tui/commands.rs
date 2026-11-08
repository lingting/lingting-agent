//! 界面命令用例：Ctrl+N 新建会话、Ctrl+R 重连、会话列表的 Ctrl 组合键不当成导航。

use super::{STEP_TIMEOUT, open};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::ensure;
use lib_bin_tui::testing::{Connection, Focus, KeyCode, KeyModifiers};

pub(super) fn cases() -> Vec<Case> {
    vec![Case {
        name: "tui.commands",
        kind: CaseKind::Shared,
        run: commands,
    }]
}

fn commands(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut harness = open(context)?;

        // Ctrl+N 新建会话：不会被输入组件当成“下移”，也不会写入字母。
        harness.press_ctrl(KeyCode::Char('n'));
        ensure!(
            harness.wait_until(STEP_TIMEOUT, |app| app.current.is_some()),
            "Ctrl+N 未能新建会话"
        );
        let first = harness.app().current.clone();
        ensure!(harness.app().focus == Focus::Input, "新建会话后焦点应在输入框");
        ensure!(
            harness
                .app()
                .sessions
                .iter()
                .any(|session| Some(&session.id) == first.as_ref()),
            "新建的会话应出现在会话列表"
        );

        // 再建一个：列表里至少两条，后面的 Ctrl+J 断言才有意义。
        harness.press_ctrl(KeyCode::Char('n'));
        ensure!(
            harness.wait_until(STEP_TIMEOUT, |app| app.current != first && app.current.is_some()),
            "Ctrl+N 未能切换到第二个会话"
        );

        // Ctrl+R 重连：立即进入连接中，随后重新连上。
        harness.press_ctrl(KeyCode::Char('r'));
        ensure!(
            matches!(harness.app().connection, Connection::Connecting),
            "Ctrl+R 后应进入连接中，实际 {:?}",
            harness.app().connection
        );
        ensure!(
            harness.wait_until(STEP_TIMEOUT, |app| matches!(app.connection, Connection::Online { .. })),
            "重连未在 {} 秒内完成",
            STEP_TIMEOUT.as_secs()
        );

        // 重连会重新拉会话列表：当前会话应重新出现在列表里。
        ensure!(
            harness.wait_until(STEP_TIMEOUT, |app| app
                .current
                .as_deref()
                .is_some_and(|id| app.sessions.iter().any(|session| session.id == id))),
            "重连后当前会话应重新出现在会话列表"
        );

        // 会话列表不提供 Ctrl 组合键功能：Ctrl+J 不能移动选中项。
        harness.press(KeyCode::Tab, KeyModifiers::NONE);
        ensure!(harness.app().focus == Focus::Sessions, "Tab 应把焦点切到会话列表");
        ensure!(harness.app().sessions.len() >= 2, "重连后会话列表应至少两条");
        let selected = harness.app().selected;
        harness.press_ctrl(KeyCode::Char('j'));
        ensure!(
            harness.app().selected == selected,
            "Ctrl+J 不应移动选中项：{selected} → {}",
            harness.app().selected
        );

        Ok(vec![
            format!("Ctrl+N 新建会话后列表共 {} 条", harness.app().sessions.len()),
            "Ctrl+R 重连后连接与会话列表恢复".to_string(),
            format!("会话列表选中项 {selected}，Ctrl+J 未移动"),
        ])
    })
}
