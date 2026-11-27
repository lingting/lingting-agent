//! `resume.history`（P6A-013 / 验收 `I-2`、`E-5`）：`api/run/history` 的正常 / 空 / 404 三种响应。
//!
//! 断言只引用接口响应：步骤按 `seqNo` 升序、字段是白名单（没有 `payload` 原文）、
//! 没有检查点的 Run 返回空数组、未知 Run 是 404。**不读日志、不读事件文件**。

use anyhow::{Result, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiClient, RunHistory, RunHistoryParams, RunStatus};

use crate::case::CaseFuture;
use crate::util::context::TestContext;
use crate::util::flow::{self, Occupied};

/// 用例入口。
pub(super) fn case(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { require(context).await })
}

/// 三段：正常 Run、没有检查点的 Run、未知 Run。
async fn require(context: &TestContext) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    notes.extend(normal(context).await?);
    notes.extend(without_checkpoints(context).await?);
    notes.extend(unknown(context).await?);
    Ok(notes)
}

/// 一、正常 Run：步骤从 1 连续递增，字段只有 `seqNo` / `phase` / `toolRound` / `createdAtMs`。
async fn normal(context: &TestContext) -> Result<Vec<String>> {
    let mut occupied = Occupied::start(context, "步骤历史").await?;
    let (sent, drained) = occupied.say(context, &format!("普通回复 {}", context.token())).await?;
    ensure!(
        drained.terminal.error.is_none(),
        "正常 Run 不应带失败原因：{:?}",
        drained.terminal.error
    );

    let history = history_of(context, &sent.run.id).await?;
    ensure!(
        history.run.id == sent.run.id,
        "历史里的 Run 应是同一个：{} != {}",
        history.run.id,
        sent.run.id
    );
    ensure!(
        history.run.status == RunStatus::Completed,
        "读完历史时 Run 应已 COMPLETED，实际 {:?}",
        history.run.status
    );
    ensure!(
        history.resume_count == 0 && history.interrupted_reason.is_none(),
        "未恢复过的 Run 不应有恢复次数与中断原因：{history:?}"
    );

    let sequence = history.steps.iter().map(|step| step.seq_no).collect::<Vec<_>>();
    let expected = (1..=i64::try_from(history.steps.len()).unwrap_or(0)).collect::<Vec<_>>();
    ensure!(!sequence.is_empty(), "执行完成的 Run 应有检查点步骤：{history:?}");
    ensure!(sequence == expected, "steps 应按 seqNo 升序且从 1 连续：{sequence:?}");

    // 字段白名单：接口只给派生字段，`payload` 原文不外泄（验收 I-2）。
    let first = history.steps.first().map(serde_json::to_value).transpose()?;
    let value = first.unwrap_or(serde_json::Value::Null);
    let keys = sorted_keys(&value)?;
    ensure!(
        keys == ["createdAtMs", "phase", "seqNo", "toolRound"],
        "步骤字段应为白名单（不含 payload）：{value}"
    );

    occupied.release(context).await?;
    Ok(vec![
        format!("api/run/history 返回 {} 条步骤，seqNo 从 1 连续", history.steps.len()),
        format!("步骤字段只有 {keys:?}（没有 payload 原文）"),
    ])
}

/// 二、没有检查点的 Run：`steps` 是空数组，不报错（升级前的旧 Run 同形）。
async fn without_checkpoints(context: &TestContext) -> Result<Vec<String>> {
    let session = flow::create_session(context, "无检查点").await?;
    // 不建立占用：Run 停在 QUEUED，一条检查点都没有。
    let sent = flow::send_message(context, &session.id, "只排队不执行").await?;
    ensure!(
        sent.run.status == RunStatus::Queued,
        "未占用时新 Run 应停在 QUEUED，实际 {:?}",
        sent.run.status
    );

    let history = history_of(context, &sent.run.id).await?;
    ensure!(history.steps.is_empty(), "没有检查点的 Run 应返回空数组：{history:?}");
    ensure!(
        history.run.status == RunStatus::Queued && history.resume_count == 0,
        "空历史的 Run 状态应原样返回：{:?}",
        history.run
    );
    Ok(vec!["没有检查点的 Run：steps = []，status = QUEUED，不报错".to_owned()])
}

/// 三、未知 Run：404 + 统一错误契约。
async fn unknown(context: &TestContext) -> Result<Vec<String>> {
    let result = context
        .client()
        .run_history(RunHistoryParams {
            run_id: format!("missing-{}", context.token()),
        })
        .await;
    let error = ClientError::from_call(result.expect_err("未知 Run 的步骤历史应返回错误"));
    ensure!(
        matches!(error, ClientError::Status { code: 404, .. }),
        "未知 Run 应返回 404，实际：{error}"
    );
    Ok(vec!["未知 runId：404（与 api/run/get 同一口径）".to_owned()])
}

/// `api/run/history`。
async fn history_of(context: &TestContext, run_id: &str) -> Result<RunHistory> {
    context
        .client()
        .run_history(RunHistoryParams {
            run_id: run_id.to_owned(),
        })
        .await
        .map_err(ClientError::from_call)
        .map_err(Into::into)
}

/// 对象字段名排序后返回：白名单比对与字段顺序无关。
fn sorted_keys(value: &serde_json::Value) -> Result<Vec<String>> {
    let object = value.as_object().ok_or_else(|| anyhow::anyhow!("步骤应是 JSON 对象"))?;
    let mut keys = object.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    Ok(keys)
}
