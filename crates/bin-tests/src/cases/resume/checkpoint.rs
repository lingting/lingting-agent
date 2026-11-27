//! `resume.checkpoint` 的运行时半边（P6A-005 / P6A-006）：三个已提交边界都有检查点，
//! 检查点写失败即 `FAILED`，工具调用的开始标记在执行前落库。
//!
//! 证据只有两类：接口响应（`run/get` / `message/list` / `tool/audit/list`）与库内行。
//! 库内行经产品自己的 `Repository` 读回，不复制投影与解码逻辑，也不读日志与事件文件。

use agent_core::session::{CheckpointPhase, ToolCallStatus};
use anyhow::{Context, Result, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{
    ApiClient, Message, MessageListParams as SdkMessageListParams, MessageRole, MessageSendPO as SdkMessageSendPO,
    Run as SdkRun, RunEventKind, RunGetParams as SdkRunGetParams, RunStatus,
};

use super::super::tool::{prompt, serve_with, wait_confirm};
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::flow::Occupied;
use crate::util::store;

/// 检查点写失败的固定文案（与 `agent-runtime` 的 `error::CHECKPOINT` 逐字一致）。
const CHECKPOINT_ERROR: &str = "检查点写入失败";

/// 运行时半边：起一次私有服务端，三段断言都在同一个 `AGENT_HOME` 上完成。
pub(super) async fn require(context: &TestContext) -> Result<Vec<String>> {
    let mock = MockServer::start().await?;
    // `file_search` 改成需确认：挂起段需要一条「有开始、无结束」的审计行。
    // 它对 `calculator` 段的工具清单与执行没有影响。
    let extra = "[tools.permissions]\nfile_search = \"confirm\"\n";
    serve_with(context, &mock, extra, async {
        let mut notes = Vec::new();
        notes.extend(boundaries(context).await?);
        notes.extend(write_failure(context).await?);
        notes.extend(start_marker(context).await?);
        Ok(notes)
    })
    .await
}

/// 一、两轮工具 + 正常完成：起点 + 每轮模型边界 + 每轮工具边界 = 5 条，序号连续（P6A-005）。
async fn boundaries(context: &TestContext) -> Result<Vec<String>> {
    let mut occupied = Occupied::start(context, "resume.checkpoint").await?;
    let (sent, drained) = occupied.say(context, &prompt(two_rounds())).await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCompleted,
        "两轮工具 Run 应以 run_completed 收尾，实际 {:?}（{}）",
        drained.terminal.event,
        drained.terminal.error.clone().unwrap_or_default()
    );

    let steps = store::checkpoints(context, &sent.run.id).await?;
    let shape = steps
        .iter()
        .map(|item| (item.seq_no, item.phase, item.tool_round))
        .collect::<Vec<_>>();
    ensure!(
        shape
            == vec![
                (1, CheckpointPhase::ModelDone, 0),
                (2, CheckpointPhase::ModelDone, 1),
                (3, CheckpointPhase::ToolsDone, 1),
                (4, CheckpointPhase::ModelDone, 2),
                (5, CheckpointPhase::ToolsDone, 2),
            ],
        "检查点应是「起点 + 每轮模型边界 + 每轮工具边界」共 5 条且序号连续：{shape:?}"
    );
    // 起点与模型边界没有工具组；工具边界的载荷就是这一批调用的摘要，状态是执行后的最终状态。
    for item in &steps {
        match item.phase {
            CheckpointPhase::ModelDone => ensure!(
                item.payload.tool_calls.is_empty(),
                "模型边界不应携带工具组摘要（seq {}）：{:?}",
                item.seq_no,
                item.payload
            ),
            CheckpointPhase::ToolsDone => {
                ensure!(
                    item.payload.tool_calls.len() == 1,
                    "每轮工具批次应有一条摘要（seq {}）：{:?}",
                    item.seq_no,
                    item.payload
                );
                let digest = &item.payload.tool_calls[0];
                ensure!(
                    digest.status == ToolCallStatus::Ok,
                    "摘要记录的是执行后的状态（seq {}）：{digest:?}",
                    item.seq_no
                );
            }
            CheckpointPhase::AwaitingConfirm => {
                // 本段没有确认点。
                anyhow::bail!("两轮工具 Run 不应出现 AWAITING_CONFIRM（seq {}）", item.seq_no);
            }
        }
        ensure!(
            item.last_message_seq > 0,
            "检查点应记录已提交消息的上界（seq {}）：{item:?}",
            item.seq_no
        );
    }

    // 接口侧的事实：指针跟到最后一条，未恢复过的 Run 没有恢复次数与中断原因。
    let run = run_of(context, &sent.run.id).await?;
    ensure!(
        run.last_checkpoint_seq == Some(5),
        "lastCheckpointSeq 应跟到最后一条：{:?}",
        run.last_checkpoint_seq
    );
    ensure!(
        run.resume_count == 0 && run.interrupted_reason.is_none(),
        "未恢复过的 Run 不应有恢复次数与中断原因：{run:?}"
    );

    occupied.release(context).await?;
    Ok(vec![
        format!(
            "两轮工具 + 正常完成共 {} 条检查点，序号 1..{} 连续，phase 依次为 MODEL_DONE / MODEL_DONE / TOOLS_DONE / MODEL_DONE / TOOLS_DONE",
            steps.len(),
            steps.len()
        ),
        format!(
            "run/get 的 lastCheckpointSeq = {:?}，resumeCount = {}，interruptedReason 为空",
            run.last_checkpoint_seq, run.resume_count
        ),
    ])
}

/// 二、检查点写失败：注入插入失败后 Run 落 `FAILED`、文案固定、无助手消息、库里没有检查点（C-7）。
async fn write_failure(context: &TestContext) -> Result<Vec<String>> {
    let pool = store::pool(context).await?;
    // 注入：只让 `run_checkpoints` 的插入失败，其余表与查询不受影响。
    sqlx::query(
        "CREATE TRIGGER inject_checkpoint_failure BEFORE INSERT ON run_checkpoints \
         BEGIN SELECT RAISE(ABORT, 'injected checkpoint failure'); END",
    )
    .execute(&pool)
    .await
    .context("创建注入触发器失败")?;

    let mut occupied = Occupied::start(context, "resume.checkpoint-fail").await?;
    let sent = context
        .client()
        .message_create(SdkMessageSendPO {
            session_id: occupied.session.id.clone(),
            content: "检查点写不进去的 Run".to_owned(),
        })
        .await
        .map_err(ClientError::from_call)?;
    let drained = crate::util::flow::drain_run(&mut occupied.stream, &sent.run.id).await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunFailed,
        "检查点写失败应落 run_failed，实际 {:?}",
        drained.terminal.event
    );
    ensure!(
        drained.terminal.error.as_deref() == Some(CHECKPOINT_ERROR),
        "run_failed 的文案应是「{CHECKPOINT_ERROR}」，实际 {:?}",
        drained.terminal.error
    );
    let run = run_of(context, &sent.run.id).await?;
    ensure!(
        run.status == RunStatus::Failed && run.error.as_deref() == Some(CHECKPOINT_ERROR),
        "Run 应落 FAILED 且 error 是固定文案：{run:?}"
    );
    ensure!(
        store::checkpoints(context, &sent.run.id).await?.is_empty(),
        "检查点写不进去时库里不应留下任何检查点"
    );
    let messages = messages_of(context, &occupied.session.id).await?;
    ensure!(
        messages.iter().all(|message| message.role != MessageRole::Assistant),
        "检查点写失败不应产生助手消息：{messages:?}"
    );

    // 解除注入：后面的段照常写检查点。
    sqlx::query("DROP TRIGGER inject_checkpoint_failure")
        .execute(&pool)
        .await
        .context("删除注入触发器失败")?;
    pool.close().await;
    occupied.release(context).await?;
    Ok(vec![
        format!("注入检查点插入失败后 Run 落 FAILED，run_failed 与 run/get 的 error 都是「{CHECKPOINT_ERROR}」"),
        format!("该 Run 的检查点 0 条、消息 {} 条且没有助手消息", messages.len()),
    ])
}

/// 三、开始标记：挂起时库里留下 `PENDING` 行且 `started_at_ms` 有值，查询接口不返回它（P6A-006 / D-P6-008）。
async fn start_marker(context: &TestContext) -> Result<Vec<String>> {
    let mut occupied = Occupied::start(context, "resume.checkpoint-pending").await?;
    let sent = context
        .client()
        .message_create(SdkMessageSendPO {
            session_id: occupied.session.id.clone(),
            content: prompt(file_search()),
        })
        .await
        .map_err(ClientError::from_call)?;
    let (confirm, _seen) = wait_confirm(&mut occupied.stream, &sent.run.id).await?;
    let tool_call_id = confirm.tool_call_id.clone().context("确认事件应带 toolCallId")?;

    // 库内事实：这一条是执行前写下的开始标记，没有结果。
    let records = store::tool_calls(context, &sent.run.id).await?;
    let pending = records
        .iter()
        .find(|record| record.tool_call_id == tool_call_id)
        .with_context(|| format!("库里应有这次调用的开始标记：{records:?}"))?;
    ensure!(
        pending.status == ToolCallStatus::Pending && pending.result.is_none(),
        "挂起的调用应是 PENDING 且没有结果：{pending:?}"
    );
    ensure!(
        pending.started_at_ms.is_some(),
        "开始标记必须带 started_at_ms：{pending:?}"
    );

    // 查询口径：`PENDING` 是过程量，不出现在 `api/tool/audit/list`（D-P6-008）。
    let visible = super::super::tool::audit(context, &sent.run.id).await?;
    ensure!(visible.is_empty(), "挂起期间审计查询不应返回 PENDING 行：{visible:?}");

    occupied.release(context).await?;
    Ok(vec![
        format!("挂起时库里留下 PENDING 开始标记（tool_call_id = {tool_call_id}，startedAtMs 有值）"),
        "同一时刻 api/tool/audit/list 返回 0 条（PENDING 不进入查询结果）".to_owned(),
    ])
}

/// 两轮工具脚本：第一轮 `1+1`，第二轮 `2+2`，第三轮回普通文本收尾。
fn two_rounds() -> serde_json::Value {
    serde_json::json!({
        "rounds": [
            {"calls": [{"name": "calculator", "arguments": {"expression": "1+1"}}]},
            {"calls": [{"name": "calculator", "arguments": {"expression": "2+2"}}]},
        ],
    })
}

/// 需确认的内置工具调用：工具不会被真正执行（用例不批准这次挂起）。
fn file_search() -> serde_json::Value {
    serde_json::json!({
        "calls": [{"name": "file_search", "arguments": {"query": "检查点"}}],
    })
}

/// `api/run/get`。
async fn run_of(context: &TestContext, run_id: &str) -> Result<SdkRun> {
    context
        .client()
        .run_get(SdkRunGetParams {
            run_id: run_id.to_owned(),
        })
        .await
        .map_err(ClientError::from_call)
        .map_err(Into::into)
}

/// `api/message/list`。
async fn messages_of(context: &TestContext, session_id: &str) -> Result<Vec<Message>> {
    context
        .client()
        .message_list(SdkMessageListParams {
            session_id: session_id.to_owned(),
        })
        .await
        .map_err(ClientError::from_call)
        .map_err(Into::into)
}
