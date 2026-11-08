//! 客户端错误分支用例：服务未启动、请求超时、服务返回非 2xx（R-5）。
//!
//! 三条分支都走生成 SDK 的真实调用路径：前两条用不可达地址与“只接受不回包”的本地监听器构造，
//! 第三条用隔离服务端的探针接口。

use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::{Context, Result, ensure};
use lib_bin_cli::client::{Client, ClientError};
use lib_bin_cli::sdk::{ApiClient, ProbeQueryPO};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

/// 客户端请求超时是 30 秒，留出 15 秒余量等它自己判超时。
const TIMEOUT_BUDGET: Duration = Duration::from_secs(45);

/// 客户端错误分支用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![Case {
        name: "client.errors",
        kind: CaseKind::Isolated,
        prep: &[],
        run: errors,
    }]
}

fn errors(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();

        // 服务未启动：拿到可读的连接失败，而不是 panic 或空错误。
        let port = crate::server::free_port()?;
        let url = format!("http://127.0.0.1:{port}");
        let error = call(&url).await?;
        match error {
            ClientError::Connection { url: failed, .. } => {
                ensure!(
                    failed.starts_with(&url),
                    "连接失败应带上请求地址（{url}），实际 {failed}"
                );
                notes.push(format!("服务未启动：{failed}"));
            }
            other => ensure!(false, "服务未启动应归为连接失败，实际 {other}"),
        }

        // 请求超时：监听器接受连接但从不回包，客户端必须在超时后给出 Timeout。
        let listener = TcpListener::bind("127.0.0.1:0").await.context("创建本地监听器失败")?;
        let address = listener.local_addr().context("读取监听地址失败")?;
        let hold = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                held.push(stream);
            }
        });
        let timeout_url = format!("http://{address}");
        let started = Instant::now();
        let error = tokio::time::timeout(TIMEOUT_BUDGET, call(&timeout_url))
            .await
            .context("客户端未在超时预算内返回")??;
        let elapsed = started.elapsed();
        hold.abort();
        match error {
            ClientError::Timeout { url: failed } => {
                ensure!(
                    failed.starts_with(&timeout_url),
                    "超时应带上请求地址（{timeout_url}），实际 {failed}"
                );
                notes.push(format!("请求超时：{failed}（{} 秒）", elapsed.as_secs()));
            }
            other => ensure!(false, "不回包的监听器应归为超时，实际 {other}"),
        }

        // 服务返回非 2xx：错误里必须带服务端的 code 与 message，且消息可直接展示。
        match context
            .client()
            .probe_fail(ProbeQueryPO {
                keyword: None,
                size: Some(101),
                desc: None,
            })
            .await
            .map_err(ClientError::from_call)
        {
            Err(ClientError::Status { code, message }) => {
                ensure!(code == 400, "参数错误应为 400，实际 {code}");
                ensure!(
                    message == "size 超出探针允许范围",
                    "非 2xx 的 message 应来自服务端错误体：{message}"
                );
                notes.push(format!("非 2xx：{code} {message}"));
            }
            Err(other) => ensure!(false, "参数错误应归为非 2xx，实际 {other}"),
            Ok(_) => ensure!(false, "参数错误调用不应成功"),
        }

        Ok(notes)
    })
}

/// 调一次探活，返回归一化后的客户端错误。
async fn call(base_url: &str) -> Result<ClientError> {
    let client = Client::new(base_url).context("创建客户端失败")?;
    match client.probe_health().await {
        Ok(health) => anyhow::bail!("调用不应成功：{health:?}"),
        Err(error) => Ok(ClientError::from_call(error)),
    }
}
