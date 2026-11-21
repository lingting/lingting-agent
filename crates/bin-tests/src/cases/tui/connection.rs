//! 连接用例：服务端未启动时界面不崩溃、显示地址与原因，并提供重试；心跳持续；
//! 服务端反注册后界面整体重连并按服务端消息重载。

use super::{CONNECT_TIMEOUT, POLL_INTERVAL, RECONNECT_TIMEOUT, SCREEN_HEIGHT, SCREEN_WIDTH, STREAM_TIMEOUT, open};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use crate::server;
use anyhow::ensure;
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiClient, ClientDeletePO};
use lib_bin_tui::testing::{Connection, Harness, KeyCode, KeyModifiers, Phase};
use std::time::Instant;
use tokio::time::sleep;
pub(super) fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "tui.offline",
            kind: CaseKind::Shared,
            prep: &[],
            run: offline_server,
        },
        Case {
            name: "tui.heartbeat",
            kind: CaseKind::Shared,
            prep: &[],
            run: heartbeat,
        },
        Case {
            name: "tui.reconnect",
            kind: CaseKind::Shared,
            prep: &[],
            run: reconnect,
        },
    ]
}

/// 服务端未启动：不崩溃、不退出，显示服务地址与失败原因，并给出重试方式。
fn offline_server(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        // 取一个没有服务监听的端口：这个地址上不存在服务端。
        let port = server::free_port()?;
        let base_url = format!("http://127.0.0.1:{port}");
        let mut harness = Harness::start(base_url.clone())?;

        ensure!(
            harness
                .wait_until_async(CONNECT_TIMEOUT, |app| matches!(app.connection, Connection::Offline))
                .await,
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
            harness
                .wait_until_async(CONNECT_TIMEOUT, |app| matches!(app.connection, Connection::Offline))
                .await,
            "重试失败后仍应显示未连接"
        );

        // 未连接状态下渲染正常：状态栏有地址与未连接，提示行有原因与重试方式。
        let screen = harness.screen(SCREEN_WIDTH, SCREEN_HEIGHT);
        ensure!(screen.contains("未连接"), "渲染应显示未连接：\n{screen}");
        ensure!(screen.contains(&base_url), "渲染应显示服务地址：\n{screen}");
        ensure!(screen.contains("Ctrl+R 重试"), "渲染应显示重试提示：\n{screen}");

        let result = vec![
            format!("不可达地址：{base_url}"),
            format!("提示行：{notice}"),
            "重试失败后仍显示未连接，界面可正常渲染".to_string(),
        ];
        harness.close().await;
        Ok(result)
    })
}

/// 心跳：注册后立刻有一拍 ping，空闲期心跳持续，连接不被判超时（D-P1.1-006）。
fn heartbeat(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut harness = open(context).await?;
        let client_id = harness.snapshot().client_id.expect("注册成功后应有 clientId");

        // 注册响应里的首个 pong 之外，心跳任务的第一拍 ping 也会立刻拿到 pong。
        ensure!(
            wait_pongs(&mut harness, 2).await,
            "心跳未在 {} 秒内完成一拍",
            CONNECT_TIMEOUT.as_secs()
        );
        ensure!(
            matches!(harness.app().connection, Connection::Online { .. }),
            "心跳不应把连接判成未连接"
        );
        ensure!(
            harness.snapshot().client_id.as_deref() == Some(client_id.as_str()),
            "心跳不应改变 clientId"
        );

        let result = vec![
            format!("clientId：{client_id}"),
            format!("已收 pong 次数：{}", harness.snapshot().pongs),
            "空闲期心跳持续，连接保持已连接".to_string(),
        ];
        harness.close().await;
        Ok(result)
    })
}

/// 服务端反注册：界面按「客户端未注册」整体重连，换新 `clientId` 后按服务端消息整体重载。
fn reconnect(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut harness = open(context).await?;

        // 先跑完一轮对话：重连后要按服务端消息整体重载，而不是拼接半条消息。
        harness.type_text(&format!("重连用例 {}", context.token()));
        harness.press(KeyCode::Enter, KeyModifiers::NONE);
        ensure!(
            harness
                .wait_until_async(STREAM_TIMEOUT, |app| app.phase == Phase::Idle
                    && app.messages.len() == 2
                    && !app.messages[1].content.is_empty())
                .await,
            "首轮回复未在 {} 秒内结束",
            STREAM_TIMEOUT.as_secs()
        );

        let first = harness.snapshot().client_id.expect("注册成功后应有 clientId");
        // 模拟服务端反注册：此后该 clientId 的上行请求一律 400 固定 message。
        context
            .client()
            .client_delete(ClientDeletePO {
                client_id: first.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;

        // 心跳/重连判据都在后台跑：等到出现新的 clientId 且界面恢复已连接。
        let deadline = Instant::now() + RECONNECT_TIMEOUT;
        let recovered = loop {
            if let Some(current) = harness.snapshot().client_id
                && current != first
                && matches!(harness.app().connection, Connection::Online { .. })
            {
                break Some(current);
            }
            if Instant::now() >= deadline {
                break None;
            }
            harness.pump();
            sleep(POLL_INTERVAL).await;
        };
        let current = recovered.expect("界面未在超时内按「客户端未注册」整体重连");

        // 整体重载：消息条数与服务端一致，且没有残留的中断标记。
        ensure!(
            harness
                .wait_until_async(STREAM_TIMEOUT, |app| app.messages.len() == 2
                    && !app.messages[1].content.is_empty())
                .await,
            "重连后未按服务端消息整体重载"
        );
        ensure!(
            harness.app().messages.iter().all(|message| !message.interrupted),
            "整体重载后不应留下中断标记"
        );

        let result = vec![
            format!("旧 clientId：{first}"),
            format!("新 clientId：{current}"),
            "反注册后心跳收到固定 message，界面整体重连并重载消息".to_string(),
        ];
        harness.close().await;
        Ok(result)
    })
}

/// 轮询直到收到指定数量的 pong。
async fn wait_pongs(harness: &mut Harness, expected: u64) -> bool {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        if harness.snapshot().pongs >= expected {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        harness.pump();
        sleep(POLL_INTERVAL).await;
    }
}
