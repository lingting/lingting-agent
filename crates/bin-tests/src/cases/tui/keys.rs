//! 键位用例：Ctrl+C 不响应（输入框与会话列表），Ctrl+D 取消进行中的回复并退出，
//! Esc 中断后回到可输入状态。

use super::{STREAM_TIMEOUT, draft, offline, open, wait_run_cancelled};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::{bail, ensure};
use lib_bin_tui::testing::{Focus, KeyCode, KeyModifiers, Phase};

pub(super) fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "tui.ctrl_c_ignored",
            kind: CaseKind::Shared,
            run: ctrl_c_ignored,
        },
        Case {
            name: "tui.esc_cancel",
            kind: CaseKind::Shared,
            run: esc_cancel,
        },
        Case {
            name: "tui.ctrl_d_cancel",
            kind: CaseKind::Shared,
            run: ctrl_d_cancel,
        },
    ]
}

fn ctrl_c_ignored(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut harness = offline(context);

        // 输入焦点下 Ctrl+C 既不复制输入框内容，也不退出。
        harness.type_text("草稿");
        harness.press_ctrl(KeyCode::Char('c'));
        ensure!(!harness.app().should_quit(), "输入焦点按 Ctrl+C 不应退出");
        ensure!(
            draft(&harness) == "草稿",
            "Ctrl+C 改动了输入框内容：{}",
            draft(&harness)
        );

        // 会话列表焦点下同样不响应。
        harness.press(KeyCode::Tab, KeyModifiers::NONE);
        ensure!(harness.app().focus == Focus::Sessions, "Tab 应把焦点切到会话列表");
        harness.press_ctrl(KeyCode::Char('c'));
        ensure!(!harness.app().should_quit(), "会话列表按 Ctrl+C 不应退出");

        // 空闲时 Ctrl+D 直接退出：没有进行中的回复，就没有取消请求。
        harness.press_ctrl(KeyCode::Char('d'));
        ensure!(harness.app().should_quit(), "Ctrl+D 应退出");
        ensure!(
            harness.app().notice.is_none(),
            "退出不应留下提示：{:?}",
            harness.app().notice
        );

        Ok(vec![
            "Ctrl+C 在输入焦点与会话焦点均不响应".to_string(),
            "Ctrl+D 在空闲时直接退出".to_string(),
        ])
    })
}

fn ctrl_d_cancel(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut harness = open(context)?;
        harness.type_text(&format!("Ctrl+D 取消 {}", context.token()));
        harness.press(KeyCode::Enter, KeyModifiers::NONE);

        // 等到真的开始收增量：此时服务端的 Run 正在执行。
        ensure!(
            harness.wait_until(STREAM_TIMEOUT, |app| app.phase == Phase::Streaming),
            "回复在 {} 秒内未进入流式输出",
            STREAM_TIMEOUT.as_secs()
        );
        let Some(run_id) = harness.app().run_id.clone() else {
            bail!("流式阶段应持有进行中的 Run");
        };

        harness.press_ctrl(KeyCode::Char('d'));
        ensure!(harness.app().should_quit(), "Ctrl+D 应退出");
        ensure!(
            harness.app().phase == Phase::Cancelling,
            "退出前应进入正在中断阶段，实际 {:?}",
            harness.app().phase
        );
        // 退出后由工作线程在收尾窗口里把取消请求送到服务端。
        drop(harness);

        wait_run_cancelled(context, &run_id).await?;

        Ok(vec![
            format!("流式输出中按 Ctrl+D：退出并发出取消请求（run={run_id}）"),
            "服务端 Run 终态：已取消".to_string(),
        ])
    })
}

fn esc_cancel(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut harness = open(context)?;
        harness.type_text(&format!("Esc 中断 {}", context.token()));
        harness.press(KeyCode::Enter, KeyModifiers::NONE);

        ensure!(
            harness.wait_until(STREAM_TIMEOUT, |app| app.phase == Phase::Streaming),
            "回复在 {} 秒内未进入流式输出",
            STREAM_TIMEOUT.as_secs()
        );
        let Some(run_id) = harness.app().run_id.clone() else {
            bail!("流式阶段应持有进行中的 Run");
        };

        harness.press(KeyCode::Esc, KeyModifiers::NONE);
        ensure!(
            harness.app().phase == Phase::Cancelling,
            "Esc 后应进入正在中断阶段，实际 {:?}",
            harness.app().phase
        );
        ensure!(harness.app().notice.is_some(), "Esc 后应给出提示");

        // 事件流收尾后回到可输入状态，被中断的回复带上中断标记。
        ensure!(
            harness.wait_until(STREAM_TIMEOUT, |app| app.phase == Phase::Idle && app.run_id.is_none()),
            "中断未在 {} 秒内收尾",
            STREAM_TIMEOUT.as_secs()
        );
        ensure!(
            harness.app().messages.last().is_some_and(|message| message.interrupted),
            "被中断的回复应带中断标记"
        );
        ensure!(!harness.app().should_quit(), "Esc 中断不应退出");
        harness.type_text("中断后继续");
        ensure!(draft(&harness) == "中断后继续", "中断后输入框应可继续输入");

        wait_run_cancelled(context, &run_id).await?;

        Ok(vec![
            format!("Esc 中断 Run：{run_id}"),
            "服务端 Run 终态：已取消".to_string(),
            "界面回到可输入状态，回复标注中断".to_string(),
        ])
    })
}
