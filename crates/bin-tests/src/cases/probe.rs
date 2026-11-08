//! 探针接口用例：探活、回声、错误响应与关闭副作用。

use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::{bail, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiCallError, ApiClient, ProbeEchoPO, ProbeQueryPO};
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// 关闭后等待服务不可访问的上限。
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
/// 关闭后的探测间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 探针领域用例。
pub(crate) fn cases() -> Vec<Case> {
    let mut cases = vec![
        Case {
            name: "probe.health",
            kind: CaseKind::Shared,
            prep: &[],
            run: health,
        },
        Case {
            name: "probe.echo",
            kind: CaseKind::Shared,
            prep: &[],
            run: echo,
        },
        Case {
            name: "probe.error",
            kind: CaseKind::Shared,
            prep: &[],
            run: error,
        },
    ];
    // 关闭接口只在 debug 构建注册（release 不包含）。
    #[cfg(debug_assertions)]
    cases.push(Case {
        name: "probe.shutdown",
        kind: CaseKind::Isolated,
        prep: &[],
        run: shutdown,
    });
    cases
}

fn health(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let health = context.client().probe_health().await.map_err(ClientError::from_call)?;

        ensure!(!health.version.is_empty(), "服务版本为空");
        ensure!(
            health.protocol_version == "1",
            "协议版本应为 1，实际 {}",
            health.protocol_version
        );
        ensure!(health.started_at_ms > 0, "启动时间为 0");
        ensure!(health.uptime_ms >= 0, "运行时长为负");
        ensure!(health.running_run_count >= 0, "运行中 Run 数为负");

        Ok(vec![
            format!("服务版本：{}", health.version),
            format!("协议版本：{}", health.protocol_version),
            format!("启动时间（Unix 毫秒）：{}", health.started_at_ms),
            format!("运行时长：{}ms", health.uptime_ms),
            format!("运行中 Run 数：{}", health.running_run_count),
        ])
    })
}

fn echo(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let client = context.client();
        let echoed = client
            .probe_echo(ProbeEchoPO {
                id: 9001,
                text: "回声".to_string(),
                tags: vec!["alpha".to_string(), "beta".to_string()],
                flag: Some(true),
            })
            .await
            .map_err(ClientError::from_call)?;

        ensure!(echoed.id == 9001, "id 未回显：{}", echoed.id);
        ensure!(echoed.text == "回声", "text 未回显：{}", echoed.text);
        ensure!(echoed.tag_count == 2, "tag_count 应为 2，实际 {}", echoed.tag_count);
        ensure!(echoed.flag, "flag 未回显");

        let defaulted = client
            .probe_echo(ProbeEchoPO {
                id: 9002,
                text: "缺省字段".to_string(),
                tags: Vec::new(),
                flag: None,
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(!defaulted.flag, "省略 flag 时应为 false");

        Ok(vec![
            format!(
                "完整回显：id={}，text={}，tag_count={}，flag={}",
                echoed.id, echoed.text, echoed.tag_count, echoed.flag
            ),
            format!("省略可选字段时 flag 默认值：{}", defaulted.flag),
        ])
    })
}

fn error(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let client = context.client();
        let mut notes = Vec::new();

        let parameter = client
            .probe_fail(ProbeQueryPO {
                keyword: None,
                size: Some(101),
                desc: None,
            })
            .await;
        match parameter {
            Err(ApiCallError::Transport(ClientError::Status { code, message })) => {
                ensure!(code == 400, "size 超限应返回 400，实际 {code}");
                ensure!(message == "size 超出探针允许范围", "参数错误的对外消息不符：{message}");
                notes.push(format!("参数错误：status={code}，message={message}"));
            }
            Err(error) => bail!("size 超限应返回 400，实际错误：{error:?}"),
            Ok(_) => bail!("size 超限应返回 400，实际成功"),
        }

        match client.probe_internal().await {
            Err(ApiCallError::Transport(ClientError::Status { code, message })) => {
                ensure!(code == 500, "内部错误应返回 500，实际 {code}");
                ensure!(
                    message == "服务器内部错误",
                    "内部错误的对外消息应固定为“服务器内部错误”，实际 {message}"
                );
                ensure!(!message.contains("sql"), "对外消息泄漏内部细节");
                notes.push(format!("内部错误：status={code}，message={message}"));
            }
            Err(error) => bail!("内部错误应返回 500，实际错误：{error:?}"),
            Ok(_) => bail!("内部错误应返回 500，实际成功"),
        }

        Ok(notes)
    })
}

#[cfg(debug_assertions)]
fn shutdown(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let client = context.client();
        client.probe_shutdown().await.map_err(ClientError::from_call)?;

        let started = Instant::now();
        let deadline = started + SHUTDOWN_TIMEOUT;
        loop {
            if client.probe_health().await.is_err() {
                return Ok(vec![
                    "关闭接口返回成功".to_string(),
                    format!("关闭后探活请求失败，耗时 {}ms", started.elapsed().as_millis()),
                ]);
            }
            if Instant::now() >= deadline {
                bail!("关闭请求后服务在 {} 秒内仍可访问", SHUTDOWN_TIMEOUT.as_secs());
            }
            sleep(POLL_INTERVAL).await;
        }
    })
}
