//! `resume.*` 用例域（P6A-016）：检查点、崩溃恢复、触发顺序、幂等、中断通知、损坏、终态、
//! 接口、可观测与两端客户端。
//!
//! 检查点有 HTTP 接口之后（`api/run/history`，P6A-013），运行期事实优先从接口读：
//! `run/get` / `run/history` / `message/list` / `tool/audit/list` / `probe/metrics`。
//! 只有「接口还没有暴露」的过程量（检查点原始列、注入损坏、`PENDING` 审计行）才经
//! 产品自己的 `Repository` 或裸连接池读回。**不读日志、不读事件文件**。
//!
//! 存储半边四段：
//!
//! - 序号与载荷：序号从 1 连续、最近一条正确、指针同步、载荷是白名单字段且不超字节上界；
//! - 终态保留策略：未结束的 Run 一条不删，终态后只保留最近 `CHECKPOINT_KEEP_LAST` 条；
//! - 不可解释的两类：缺字段 / 版本为 0 判 `Corrupt`，版本高于当前判 `Unsupported`；
//! - 指针同事务：注入插入失败后 `lastCheckpointSeq` 保持原值，且不留半行。
//!
//! 运行时半边三段（P6A-005 / P6A-006）在 [`checkpoint`] 子模块：
//!
//! - 边界清单：两轮工具 + 正常完成只有 5 条（起点 + 每轮模型边界 + 每轮工具边界），序号连续；
//! - 写失败：注入式失败后 Run 落 `FAILED`、文案固定、无助手消息、库里没有检查点；
//! - 开始标记：挂起的调用在库里是 `PENDING` 且 `started_at_ms` 有值，但查询接口不返回它。
use std::sync::Arc;

use agent_core::session::{
    CHECKPOINT_FORMAT_VERSION, CHECKPOINT_KEEP_LAST, CheckpointDecodeError, CheckpointPhase, MessageSendPO,
    RunCheckpoint, RunGetParams, RunStatus, SessionCreatePO, ToolCallDigest, ToolCallStatus,
};
use anyhow::{Context, Result, ensure};
use lib_core::time;
use lib_db::{DATABASE_FILE, Database, DbConfig};
use service_core::observability;
use service_core::repository::Repository;
use service_core::session::{CheckpointError, SessionService};
use sqlx::SqlitePool;

use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;
use crate::util::flow::{self, Reopened};
use crate::util::server::Server;
use crate::util::support::temp_directory;

mod broken;
mod checkpoint;
mod crash;
mod history;
mod idempotent;
mod metrics;
mod notify;
mod retry;
mod terminal;
mod trigger;
mod tui;
mod web;

/// 单条载荷的字节上界（验收 `C-4`）：载荷只放最小事实，正常形态远小于这个值。
const PAYLOAD_LIMIT: usize = 2048;

/// 检查点与终态收口用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "resume.checkpoint",
            // 运行时半边要自己起服务端（`server::serve_once`），因此不用入口的共享服务端。
            kind: CaseKind::SelfServed,
            prep: &[],
            run: all,
        },
        Case {
            name: "resume.terminal",
            // 强杀 / 优雅关闭都按用例自己的节奏发生，服务端必须由用例启停。
            kind: CaseKind::SelfServed,
            prep: &[],
            run: terminal,
        },
        // 强杀 / 重启都按用例自己的节奏发生：服务端一律由用例启停（P6A-016）。
        Case {
            name: "resume.crash",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: crash::case,
        },
        Case {
            name: "resume.trigger",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: trigger::case,
        },
        Case {
            name: "resume.idempotent",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: idempotent::case,
        },
        Case {
            name: "resume.notify",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: notify::case,
        },
        Case {
            name: "resume.broken",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: broken::case,
        },
        // Run 级重试要独立的 Mock 观测记录与快照：服务端由用例自己启停（P6A-017）。
        Case {
            name: "resume.retry",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: retry::case,
        },
        // 只读接口：隔离服务端足够，不需要自己启停。
        Case {
            name: "resume.history",
            kind: CaseKind::Isolated,
            prep: &[],
            run: history::case,
        },
        Case {
            name: "resume.metrics",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: metrics::case,
        },
        Case {
            name: "resume.tui",
            kind: CaseKind::SelfServed,
            prep: &[],
            run: tui::case,
        },
        // 客户端用例只跑前端工程与静态核对：不需要服务端，复用共享服务端即可。
        Case {
            name: "resume.web",
            kind: CaseKind::Shared,
            prep: &[],
            run: web::case,
        },
    ]
}

/// 起一次用例自己的服务端（同一 `AGENT_HOME` 与端口）。
pub(super) async fn start(context: &TestContext) -> Result<Server> {
    Server::start(context.port()?, context.home().to_path_buf())
        .await
        .with_context(|| format!("启动服务端失败：{}", context.home().display()))
}

/// 强杀后在同一 `AGENT_HOME` 与端口上重启：进程内的执行状态随进程一起丢失。
pub(super) async fn crash(context: &TestContext, server: Server) -> Result<Server> {
    let home = server.log_dir().to_path_buf();
    server.stop().await.context("强杀服务端失败")?;
    Server::start(context.port()?, home)
        .await
        .with_context(|| format!("重启服务端失败：{}", context.home().display()))
}

/// 一个被强杀的 Run 及其会话。
pub(super) struct PausedRun {
    /// 所属会话。
    pub(super) session_id: String,
    /// 被强杀的 Run。
    pub(super) run_id: String,
}

/// 起一批「执行中被强杀」的 Run：每个会话一个客户端与一条慢速消息，等到首段增量后一起强杀重启。
///
/// 一批一起被杀：`RUNNING → PAUSED` 的自愈在一次重启里把它们全部处理完，省掉反复启停。
pub(super) async fn interrupt_batch(
    context: &TestContext,
    server: Server,
    tag: &str,
    count: usize,
) -> Result<(Server, Vec<PausedRun>)> {
    let mut runs = Vec::new();
    let mut links = Vec::new();
    for index in 0..count {
        let session = flow::create_session(context, &format!("{tag}-{index}")).await?;
        let mut link = Reopened::open(context, &session.id).await?;
        let content = format!("mock:slow {tag} {}", context.token());
        let sent = flow::send_message(context, &session.id, &content).await?;
        flow::wait_first_delta(&mut link.stream, &sent.run.id).await?;
        links.push(link);
        runs.push(PausedRun {
            session_id: session.id,
            run_id: sent.run.id,
        });
    }
    // 先杀进程再丢链路：客户端断连会触发该会话的 Run 收尾（断连清理链），
    // 那会把「执行中」变成 `CANCELLED`，自愈就没有可恢复的残留了。
    let server = crash(context, server).await?;
    drop(links);
    for run in &runs {
        let paused = flow::run_get(context, &run.run_id).await?;
        ensure!(
            paused.status == lib_bin_cli::sdk::RunStatus::Paused,
            "强杀重启后 Run 应是 PAUSED，实际 {:?}",
            paused.status
        );
    }
    Ok((server, runs))
}

/// 造一个「执行中被强杀」的中断态：占用 → `mock:slow` → 等到首段增量 → 强杀 → 重启。
///
/// 判定动作是「等到首段增量」而不是 sleep：增量说明模型流已经开始、检查点已经写好了
/// 起点边界，此时强杀留下的一定是「执行中」的残留（`RK-6A-09`）。
pub(super) async fn interrupt_running(
    context: &TestContext,
    server: Server,
    session_id: &str,
) -> Result<(Server, String)> {
    let mut link = Reopened::open(context, session_id).await?;
    let content = format!("mock:slow 中断 {}", context.token());
    let sent = flow::send_message(context, session_id, &content).await?;
    flow::wait_first_delta(&mut link.stream, &sent.run.id).await?;
    // 先杀进程再丢链路：断连本身会取消该会话的 Run（见 `interrupt_batch`）。
    let server = crash(context, server).await?;
    drop(link);
    Ok((server, sent.run.id))
}

/// 终态唯一收口（P6A-012）：取消 / 优雅关闭 / 恢复中再次中断 / 竞态。
fn terminal(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { terminal::require(context).await })
}

/// 存储半边四段 + 运行时半边：前者每段一个独立临时库，后者起一次私有服务端，注入互不影响。
fn all(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();
        notes.extend(sequence_and_payload().await?);
        notes.extend(prune_policy().await?);
        notes.extend(decode_rules().await?);
        notes.extend(pointer_transaction().await?);
        notes.extend(checkpoint::require(context).await?);
        Ok(notes)
    })
}

/// 一、序号从 1 连续、最近一条正确、指针同步；载荷是白名单字段且不超上界。
async fn sequence_and_payload() -> Result<Vec<String>> {
    let (_database, service) = fixture("resume-checkpoint-sequence").await?;
    let run_id = new_run(&service, "检查点序号").await?;

    let phases = [
        CheckpointPhase::ModelDone,
        CheckpointPhase::ToolsDone,
        CheckpointPhase::AwaitingConfirm,
    ];
    let mut assigned = Vec::new();
    for (index, phase) in phases.iter().enumerate() {
        let round = u32::try_from(index).unwrap_or_default();
        assigned.push(
            service
                .append_checkpoint(&run_id, &checkpoint(*phase, round, i64::from(round)))
                .await?,
        );
    }
    ensure!(
        assigned == [1, 2, 3],
        "同一 Run 的序号应从 1 开始连续递增：{assigned:?}"
    );

    let latest = service
        .latest_checkpoint(&run_id)
        .await?
        .context("追加之后应有最近一条检查点")?;
    ensure!(
        latest.seq_no == 3 && latest.phase == CheckpointPhase::AwaitingConfirm,
        "最近一条应为 seq 3 / AWAITING_CONFIRM：{latest:?}"
    );
    ensure!(
        latest.format_version == CHECKPOINT_FORMAT_VERSION
            && latest.payload.format_version == CHECKPOINT_FORMAT_VERSION,
        "格式版本应为列与载荷同一取值：{latest:?}"
    );

    let all = service.checkpoints_of_run(&run_id).await?;
    let sequence = all.iter().map(|item| item.seq_no).collect::<Vec<_>>();
    ensure!(sequence == [1, 2, 3], "读回应按序号升序且无缺口：{sequence:?}");

    // 指针跟到最后一条；未恢复过的 Run 不应有恢复次数与中断原因。
    let run = run_of(&service, &run_id).await?;
    ensure!(
        run.last_checkpoint_seq == Some(3),
        "lastCheckpointSeq 应跟到最后一条：{:?}",
        run.last_checkpoint_seq
    );
    ensure!(
        run.resume_count == 0 && run.interrupted_reason.is_none(),
        "未恢复过的 Run 不应有恢复次数与中断原因：{run:?}"
    );

    // 载荷只允许最小事实：顶层是「版本 + 工具组摘要」，摘要里是「调用标识 + 状态」。
    let payload = serde_json::to_string(&latest.payload).context("序列化载荷失败")?;
    let value: serde_json::Value = serde_json::from_str(&payload).context("解析载荷失败")?;
    ensure!(
        sorted_keys(&value)? == ["format_version", "tool_calls"],
        "载荷顶层字段应为白名单：{payload}"
    );
    let groups = value
        .get("tool_calls")
        .and_then(serde_json::Value::as_array)
        .context("载荷缺少 tool_calls 数组")?;
    let digest = groups.first().context("载荷应有工具组摘要")?;
    ensure!(
        sorted_keys(digest)? == ["status", "tool_call_id"],
        "工具组摘要字段应为白名单：{payload}"
    );
    ensure!(
        payload.len() <= PAYLOAD_LIMIT,
        "单条载荷应不超过 {PAYLOAD_LIMIT} 字节，实际 {}",
        payload.len()
    );
    for forbidden in ["api_key", "AGENTS.md", "系统提示词"] {
        ensure!(!payload.contains(forbidden), "载荷不应包含 {forbidden}：{payload}");
    }

    Ok(vec![
        "序号连续 1..3，最近一条 phase = AWAITING_CONFIRM，lastCheckpointSeq = 3".to_owned(),
        format!(
            "载荷是白名单字段（{} 字节，≤ {PAYLOAD_LIMIT}）：{payload}",
            payload.len()
        ),
    ])
}

/// 二、终态保留策略：未结束时不清理，落终态后只保留最近 `CHECKPOINT_KEEP_LAST` 条。
async fn prune_policy() -> Result<Vec<String>> {
    let (_database, service) = fixture("resume-checkpoint-prune").await?;
    let run_id = new_run(&service, "检查点清理").await?;
    // 写放大口径（P6A-018 / 验收 `P-5`）：检查点写入与清理都在 `service-core` 的同一处计数。
    let before = observability::snapshot();
    let total = u32::try_from(CHECKPOINT_KEEP_LAST).unwrap_or_default() + 5;
    for round in 0..total {
        service
            .append_checkpoint(
                &run_id,
                &checkpoint(CheckpointPhase::ToolsDone, round, i64::from(round)),
            )
            .await?;
    }
    let written = service.checkpoints_of_run(&run_id).await?.len();
    ensure!(
        written == usize::try_from(total).unwrap_or_default(),
        "应写入 {total} 条检查点，实际 {written}"
    );

    // 未结束的 Run 一条都不删：检查点正是它恢复的依据。
    ensure!(
        service.prune_checkpoints(&run_id).await? == 0,
        "未结束的 Run 不应清理检查点"
    );
    ensure!(
        service.checkpoints_of_run(&run_id).await?.len() == written,
        "未结束的 Run 清理后条数不应变化"
    );

    ensure!(service.fail_run(&run_id, "用例收尾").await?, "Run 应能被落成 FAILED");
    let removed = service.prune_checkpoints(&run_id).await?;
    ensure!(removed == 5, "应删除最早的 5 条，实际 {removed}");
    let remaining = service.checkpoints_of_run(&run_id).await?;
    ensure!(
        remaining.len() == CHECKPOINT_KEEP_LAST,
        "终态后应保留 {CHECKPOINT_KEEP_LAST} 条，实际 {}",
        remaining.len()
    );
    ensure!(
        remaining.first().map(|item| item.seq_no) == Some(6),
        "应保留最近的一批（从 seq 6 起）：{:?}",
        remaining.first().map(|item| item.seq_no)
    );
    ensure!(
        service.prune_checkpoints(&run_id).await? == 0,
        "重复清理不应再删除任何行"
    );
    // 快照口径（`P-5`）：写入与删除都在 `service-core` 的同一处计数。这里是进程内共享计数器，
    // 同进程并行跑的其它存储半段也会累加，因此只断言「至少包含本次的量」；「增量与删除条数逐值
    // 相等」放在隔离服务端的 `resume.terminal` 里断言（那里没有并行用例干扰）。
    let after = observability::snapshot();
    let written_delta = after.checkpoints_written.saturating_sub(before.checkpoints_written);
    let pruned_delta = after.checkpoints_pruned.saturating_sub(before.checkpoints_pruned);
    ensure!(
        written_delta >= u64::from(total),
        "checkpointsWritten 应至少增长 {total}，实际 {written_delta}"
    );
    ensure!(
        pruned_delta >= u64::try_from(removed).unwrap_or_default(),
        "checkpointsPruned 应至少增长 {removed}，实际 {pruned_delta}"
    );

    Ok(vec![
        format!("写入 {written} 条后未终态时清理 0 条，落 FAILED 后删除 {removed} 条、保留 {CHECKPOINT_KEEP_LAST} 条"),
        "再次清理返回 0（保留策略是幂等的）".to_owned(),
        format!(
            "快照口径：checkpointsWritten 至少 +{written_delta}、checkpointsPruned 至少 +{pruned_delta}（逐值相等见 resume.terminal）"
        ),
    ])
}

/// 三、不可解释的两类：缺字段 / 版本为 0 判 `Corrupt`，版本高于当前判 `Unsupported`。
async fn decode_rules() -> Result<Vec<String>> {
    let (database, service) = fixture("resume-checkpoint-decode").await?;
    let run_id = new_run(&service, "检查点解码").await?;
    for (index, phase) in [
        CheckpointPhase::ModelDone,
        CheckpointPhase::ToolsDone,
        CheckpointPhase::AwaitingConfirm,
    ]
    .iter()
    .enumerate()
    {
        let round = u32::try_from(index).unwrap_or_default();
        service
            .append_checkpoint(&run_id, &checkpoint(*phase, round, i64::from(round)))
            .await?;
    }
    let stored = payload_text(database.pool(), &run_id, 3).await?;

    // 缺字段：严格解码报错，不猜默认值。
    update(database.pool(), &run_id, 3, "payload", "{}").await?;
    let error = service
        .latest_checkpoint(&run_id)
        .await
        .err()
        .context("缺字段的载荷应被判为损坏")?;
    ensure!(
        matches!(error, CheckpointError::Decode(CheckpointDecodeError::Corrupt)),
        "缺字段应判 Corrupt：{error:?}"
    );

    // 版本高于当前代码：判不可恢复，不降级成从头执行。
    update(database.pool(), &run_id, 3, "payload", &stored).await?;
    sqlx::query("UPDATE run_checkpoints SET format_version = 99 WHERE run_id = ? AND seq_no = 2")
        .bind(&run_id)
        .execute(database.pool())
        .await
        .context("注入未知版本失败")?;
    let error = service
        .checkpoints_of_run(&run_id)
        .await
        .err()
        .context("未知版本的检查点应被判为不可解释")?;
    ensure!(
        matches!(
            error,
            CheckpointError::Decode(CheckpointDecodeError::Unsupported { found: 99, .. })
        ),
        "未知版本应判 Unsupported：{error:?}"
    );

    // 版本 0：与缺字段同类，判损坏。
    update(database.pool(), &run_id, 2, "format_version", "0").await?;
    let error = service
        .checkpoints_of_run(&run_id)
        .await
        .err()
        .context("版本 0 的检查点应被判为损坏")?;
    ensure!(
        matches!(error, CheckpointError::Decode(CheckpointDecodeError::Corrupt)),
        "版本 0 应判 Corrupt：{error:?}"
    );

    // 复原后仍能按当前版本严格解码：证明上面的失败只由注入造成。
    update(database.pool(), &run_id, 2, "format_version", "1").await?;
    ensure!(
        service.checkpoints_of_run(&run_id).await?.len() == 3,
        "复原版本后应能读回全部 3 条"
    );

    Ok(vec![
        "缺字段 / 版本 0 → Corrupt；版本 99 → Unsupported（两类都不进入执行路径）".to_owned(),
        "复原为当前版本后按严格规则读回 3 条".to_owned(),
    ])
}

/// 四、指针同事务：注入插入失败后指针保持原值、不留半行；解除注入后序号接着续写。
async fn pointer_transaction() -> Result<Vec<String>> {
    let (database, service) = fixture("resume-checkpoint-pointer").await?;
    let run_id = new_run(&service, "检查点指针").await?;
    ensure!(
        service
            .append_checkpoint(&run_id, &checkpoint(CheckpointPhase::ModelDone, 0, 1))
            .await?
            == 1,
        "第一条检查点的序号应为 1"
    );

    // 注入：插行一定失败。它发生在「算序号之后、更新指针之前」，因此这条断言同时证明两者同事务。
    sqlx::query(
        "CREATE TRIGGER inject_checkpoint_failure BEFORE INSERT ON run_checkpoints \
         BEGIN SELECT RAISE(ABORT, 'injected checkpoint failure'); END",
    )
    .execute(database.pool())
    .await
    .context("创建注入触发器失败")?;
    let error = service
        .append_checkpoint(&run_id, &checkpoint(CheckpointPhase::ToolsDone, 1, 2))
        .await
        .err()
        .context("注入后追加检查点应失败")?;
    ensure!(
        matches!(error, CheckpointError::Storage(_)),
        "追加失败应归为存储错误：{error:?}"
    );
    let run = run_of(&service, &run_id).await?;
    ensure!(
        run.last_checkpoint_seq == Some(1),
        "插入失败时指针应保持原值：{:?}",
        run.last_checkpoint_seq
    );
    ensure!(count(&database, &run_id).await? == 1, "失败的事务不应留下半行检查点");

    // 解除注入：序号接着上一次（不重置），指针随之前移。
    sqlx::query("DROP TRIGGER inject_checkpoint_failure")
        .execute(database.pool())
        .await
        .context("删除注入触发器失败")?;
    ensure!(
        service
            .append_checkpoint(&run_id, &checkpoint(CheckpointPhase::ToolsDone, 1, 2))
            .await?
            == 2,
        "解除注入后应接着写 seq 2"
    );
    let run = run_of(&service, &run_id).await?;
    ensure!(
        run.last_checkpoint_seq == Some(2),
        "成功追加后指针应跟到 2：{:?}",
        run.last_checkpoint_seq
    );

    Ok(vec![
        "注入插入失败：追加返回存储错误，lastCheckpointSeq 保持 1，库内仍只有 1 行".to_owned(),
        "解除注入后接着写 seq 2，指针前移到 2（序号不重置）".to_owned(),
    ])
}

/// 独立临时库 + 会话服务：迁移由 `Database::open` 完成，与产品启动路径同源。
async fn fixture(name: &str) -> Result<(Arc<Database>, SessionService)> {
    let home = temp_directory(name)?;
    let database = Arc::new(
        Database::open(home.join(DATABASE_FILE), DbConfig::default())
            .await
            .with_context(|| format!("打开临时数据库失败：{}", home.display()))?,
    );
    let service = SessionService::new(Arc::new(Repository::new(Arc::clone(&database))));
    Ok((database, service))
}

/// 建会话并发出消息，得到一个 `QUEUED` 的 Run：检查点存储不要求 Run 正在执行。
async fn new_run(service: &SessionService, title: &str) -> Result<String> {
    let session = service
        .create_session(SessionCreatePO {
            title: Some(title.to_owned()),
            model: None,
        })
        .await
        .with_context(|| format!("创建会话失败：{title}"))?;
    let sent = service
        .send_message(MessageSendPO {
            session_id: session.id,
            content: format!("{title} 的消息"),
        })
        .await
        .with_context(|| format!("发送消息失败：{title}"))?;
    ensure!(
        sent.run.status == RunStatus::Queued,
        "新 Run 应为 Queued，实际 {:?}",
        sent.run.status
    );
    Ok(sent.run.id)
}

/// 一条检查点：载荷里只有调用标识与状态，与运行时将来写入的形态一致。
fn checkpoint(phase: CheckpointPhase, round: u32, last_message_seq: i64) -> RunCheckpoint {
    RunCheckpoint::new(
        phase,
        round,
        last_message_seq,
        vec![ToolCallDigest {
            tool_call_id: format!("call-{round}"),
            status: ToolCallStatus::Pending,
        }],
        time::now_millis(),
    )
}

/// 取 Run。
async fn run_of(service: &SessionService, run_id: &str) -> Result<agent_core::session::Run> {
    service
        .get_run(RunGetParams {
            run_id: run_id.to_owned(),
        })
        .await
        .with_context(|| format!("查询 Run 失败：{run_id}"))
}

/// 注入：把某条检查点的某一列改成给定文本。
async fn update(pool: &SqlitePool, run_id: &str, seq_no: i64, column: &str, value: &str) -> Result<()> {
    let sql = format!("UPDATE run_checkpoints SET {column} = ? WHERE run_id = ? AND seq_no = ?");
    sqlx::query(&sql)
        .bind(value)
        .bind(run_id)
        .bind(seq_no)
        .execute(pool)
        .await
        .with_context(|| format!("注入 {column} 失败"))?;
    Ok(())
}

/// 某条检查点的载荷原文（用于篡改后复原）。
async fn payload_text(pool: &SqlitePool, run_id: &str, seq_no: i64) -> Result<String> {
    sqlx::query_scalar("SELECT payload FROM run_checkpoints WHERE run_id = ? AND seq_no = ?")
        .bind(run_id)
        .bind(seq_no)
        .fetch_one(pool)
        .await
        .with_context(|| format!("读取 seq {seq_no} 的载荷失败"))
}

/// 某 Run 的检查点行数。
async fn count(database: &Database, run_id: &str) -> Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM run_checkpoints WHERE run_id = ?")
        .bind(run_id)
        .fetch_one(database.pool())
        .await
        .context("统计检查点行数失败")
}

/// 对象字段名排序后返回：白名单比对与字段顺序无关。
fn sorted_keys(value: &serde_json::Value) -> Result<Vec<String>> {
    let object = value.as_object().context("载荷片段应为 JSON 对象")?;
    let mut keys = object.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    Ok(keys)
}
