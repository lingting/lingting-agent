//! 连接用例：服务端未启动时界面不崩溃、显示地址与原因，并提供重试。

use super::{CONNECT_TIMEOUT, SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use crate::server;
use anyhow::ensure;
use lib_bin_tui::testing::{Connection, Harness, KeyCode};

pub(super) fn cases() -> Vec<Case> {
    vec![Case {
        name: "tui.offline",
        kind: CaseKind::Shared,
        prep: &[],
        run: offline_server,
    }]
}

/// 服务端未启动：不崩溃、不退出，显示服务地址与失败原因，并给出重试方式。
fn offline_server(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        // 取一个没有服务监听的端口：这个地址上不存在服务端。
        let port = server::free_port()?;
        let base_url = format!("http://127.0.0.1:{port}");
        let mut harness = Harness::start(base_url.clone())?;

        ensure!(
            harness.wait_until(CONNECT_TIMEOUT, |app| matches!(app.connection, Connection::Offline)),
            "服务端不可达时应显示未连接"
        );
        let notice = harness.app().notice.clone().expect("未连接时应给出原因");
        ensure!(notice.contains("Ctrl+R"), "提示应给出重试方式：{notice}");
        ensure!(!harness.app().should_quit(), "连不上服务端不应退出");
        ensure!(harness.app().base_url == base_url, "界面应记录服务地址");
        ensure!(harness.app().sessions.is_empty(), "未连接时不应有会话");

        // 重试仍失败：回到未连接并重新给出提示。
        harness.press_ctrl(KeyCode::Char('r'));
        ensure!(
            matches!(harness.app().connection, Connection::Connecting),
            "Ctrl+R 后应进入连接中"
        );
        ensure!(
            harness.wait_until(CONNECT_TIMEOUT, |app| matches!(app.connection, Connection::Offline)),
            "重试失败后仍应显示未连接"
        );

        // 未连接状态下渲染正常：状态栏有地址与未连接，提示行有原因与重试方式。
        let screen = harness.screen(SCREEN_WIDTH, SCREEN_HEIGHT);
        ensure!(screen.contains("未连接"), "渲染应显示未连接：\n{screen}");
        ensure!(screen.contains(&base_url), "渲染应显示服务地址：\n{screen}");
        ensure!(screen.contains("Ctrl+R 重试"), "渲染应显示重试提示：\n{screen}");

        Ok(vec![
            format!("不可达地址：{base_url}"),
            format!("提示行：{notice}"),
            "重试失败后仍显示未连接，界面可正常渲染".to_string(),
        ])
    })
}
