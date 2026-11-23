//! 边界压力用例（P2.2-009 / P2.2-010）：`stress.boundary`。
//!
//! 目标：把服务端压成「一次只跑一个 Run」的串行机器，让排队时长变成可由单次耗时线性推算的量，
//! 再把客户端数量精确摆到「刚好不会失败」的那一格上（公式与推导见 [`crate::util::stress`]）。
//!
//! 三个阶段：
//!
//! 1. **① 标定**：先跑 1 轮预热丢弃冷启动，再让 1 个客户端重复若干次「发消息 → 终态」，
//!    取**最大**耗时记作 `L0`（保守估计，不是平均值）。只有这一段被 `max_concurrent_runs`
//!    串行化；注册 / 会话 / 占用 / 订阅是每个客户端各自的准备开销，计进 `L0` 会把边界算小
//!    （实测过一次，`2N` 全部完成）；
//! 2. **② 内边界**：`N = floor(T / L0) + 1` 个客户端同时发起，必须全部 `run_completed`；
//!    且末个终态不晚于 `T + L0`（末个 Run 拿到许可后还要自己跑完一个串行段）；
//! 3. **③ 外边界**：`2N` 个客户端同时发起，必须出现排队超时——门槛因此自证存在。
//!
//! 数据口径（G-1-6）：全部来自内存——客户端收到的 `RunEvent`、测试进程计时、服务端快照
//! （`GET api/probe/metrics`）。本文件不读日志、不读 `server.log`。
//!
//! 收尾口径（D-P2.2-011）：不逐个 `client_delete`。每条断连都会触发清理链，几百条清理链会主导
//! 收尾时间且与「测容量边界」无关；下行流保持到用例结束，服务端随用例整体停止。

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use futures_util::future::join_all;
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiClient, MessageSendPO, RunEventKind, RunStatus};
use manager_agent::runtime::metrics::MetricsSnapshot;

use crate::case::{Case, CaseFuture, CaseKind};
use crate::mock::llm::MockServer;
use crate::runner;
use crate::util::context::TestContext;
use crate::util::flow::{self, Occupied};
use crate::util::server::{self, Server, config};
use crate::util::stress::{self, Distribution};

/// 排队超时的失败文案（与 `manager-agent` 的固定文本逐字一致）。
const QUEUE_TIMEOUT_REASON: &str = "执行排队超时";
/// ① 标定阶段的标签。
const CALIBRATION_TAG: &str = "① 标定";
/// ② 内边界阶段的标签。
const INNER_TAG: &str = "② 内边界";
/// ③ 外边界阶段的标签。
const OUTER_TAG: &str = "③ 外边界";

/// 压力用例：只有一条，且它自己决定服务端的配置与启停时机。
///
/// 用 `SelfServed` 而不是 `Isolated`：私有服务端必须带 `[agent]` 配置（并行度与等待预算），
/// 而 `Isolated` 的配置由调度层固定，注入不了这两个参数（D-P2.2-009 的前置条件）。
/// 与 `run.concurrency` / `run.queue_timeout` 同一形态，并且同样自起私有 `MockServer`。
pub(crate) fn cases() -> Vec<Case> {
    vec![Case {
        name: "stress.boundary",
        kind: CaseKind::SelfServed,
        prep: &[],
        run: boundary,
    }]
}

/// 用例自带的服务端配置：并行度压到 `SERIAL_RUNS`，等待预算压到 `QUEUE_BUDGET`。
fn config_text(mock_base_url: &str) -> String {
    let extra = format!(
        "[agent]\nmax_concurrent_runs = {}\nrun_queue_timeout_ms = {}\n",
        stress::SERIAL_RUNS,
        stress::QUEUE_BUDGET.as_millis()
    );
    config(mock_base_url, &extra)
}

/// 边界压力用例：私有 Mock + 私有服务端，三段跑完再收尾。
fn boundary(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        // 私有 Mock：节奏与 `/mock/stats` 观测必须私有，不复用进程级共享实例。
        let mock = MockServer::start().await?;
        let text = config_text(mock.base_url());
        let server = Server::start_with(context.port()?, context.home().to_path_buf(), &text).await?;

        let result = boundary_body(context).await;
        // 快照必须在停止之前取：`Server::stop` 走 `start_kill`，进程一退就再也取不到（P2.2-005）。
        context.observe_metrics(server.metrics().await);
        let stopping = Instant::now();
        let stopped = server.stop().await;
        context.measure("服务端停止耗时", stress::millis(stopping.elapsed()));

        match (result, stopped) {
            (Ok(notes), Ok(())) => Ok(notes),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), _) => Err(error),
        }
    })
}

/// 三段主体：标定 → 内边界 → 外边界，并把每一步的数字写进报告。
async fn boundary_body(context: &TestContext) -> Result<Vec<String>> {
    let budget = stress::QUEUE_BUDGET;
    context.measure(
        "生效配置",
        format!(
            "max_concurrent_runs = {}、run_queue_timeout_ms = {}",
            stress::SERIAL_RUNS,
            budget.as_millis()
        ),
    );

    // ① 标定：量的是「发消息 → 终态」的串行段（先跑 1 轮预热），`N` 由它算出。
    let calibration = calibrate(context).await?;
    let l0 = calibration.distribution.max;
    context.measure("① 标定原始值", calibration.render());
    context.measure("① L0（取最大值）", stress::precise(l0));

    let Some(inner) = stress::boundary(l0, budget) else {
        bail!(
            "阶段 {CALIBRATION_TAG}：参数不足以形成边界——L0 = {}、T = {} → N = floor(T / L0) + 1 = {}，\
             低于最小客户端数 {}。这说明本机单次串行耗时太大（或等待预算太小），不是服务端缺陷；\
             按 D-P2.2-009 不允许在公式外放水。",
            stress::precise(l0),
            stress::millis(budget),
            stress::count(l0, budget),
            stress::MIN_CLIENTS
        );
    };
    let outer = stress::outer(inner);
    context.measure(
        "① 公式",
        format!(
            "N = floor(T / L0) + 1，T = {}、L0 = {} → N = {inner}；N_bad = {}",
            stress::millis(budget),
            stress::precise(l0),
            inner + 1
        ),
    );
    context.measure("② 客户端数 N", inner.to_string());
    context.measure("③ 客户端数 2N", outer.to_string());

    // 死线核算（P2.2-010 第 2 条）：三段上限必须落在调度超时之内，超了就在开跑前说清楚。
    let bound = stress::budget_upper_bound(outer);
    ensure!(
        bound < runner::CASE_TIMEOUT,
        "用例时间预算不通过：三段合计上限 {}，超过调度超时 {}（N = {inner}、2N = {outer}）。\
         缩小 T 或加快 Mock 节奏需要独立决策（D-P2.2-009 / D-P2.2-012）。",
        stress::millis(bound),
        stress::millis(runner::CASE_TIMEOUT)
    );
    context.measure(
        "死线核算",
        format!(
            "三段合计上限 {}（标定 + 两段准备与收敛 + 启停），调度上限 {}",
            stress::millis(bound),
            stress::millis(runner::CASE_TIMEOUT)
        ),
    );

    // ② 内边界：`N` 个客户端必须全部成功，且落在预算推得的上界内。
    let inner_phase = run_phase(context, inner, INNER_TAG).await?;
    assert_inner(&inner_phase, budget, l0)?;
    let inner_total = phase_total(&inner_phase)?;
    let inner_wait = inner_phase.snapshot.limit.wait_max_millis;
    context.measure("② 实测总耗时", stress::millis(inner_total));
    // 余量按「最长排队」算：那才是被等待预算约束的量；总耗时还要加上最后那个 Run 自己跑的一段。
    context.measure(
        "② 排队余量",
        format!(
            "{}（最长排队 {inner_wait}ms / T {}）",
            stress::millis(budget.saturating_sub(Duration::from_millis(inner_wait))),
            stress::millis(budget)
        ),
    );
    context.measure("② 单次耗时分布", phase_distribution(&inner_phase)?.render());
    context.measure("② 快照 limit", inner_phase.limit_line());
    context.measure("② 门槛峰值", inner_phase.peak_line());

    // ③ 外边界：`2N` 个客户端必须出现排队超时，且原因逐字对得上。
    let outer_phase = run_phase(context, outer, OUTER_TAG).await?;
    let timeouts = outer_phase.timeouts();
    ensure!(
        !timeouts.is_empty(),
        "阶段 {OUTER_TAG}：期望 {outer} 个客户端里至少一条落「{QUEUE_TIMEOUT_REASON}」，\
         实际 0 条（run_completed {}、其他失败 {}、未抵终态 {}）；{}",
        outer_phase.completed(),
        outer_phase.other_failures(),
        outer_phase.unfinished(),
        outer_phase.limit_line()
    );
    let first_timeout = timeouts
        .iter()
        .map(|finished_at| finished_at.saturating_duration_since(outer_phase.started))
        .min()
        .context("阶段 ③：排队超时的时刻不可用")?;
    let outer_total = phase_total(&outer_phase)?;
    context.measure(
        "③ 超时条数",
        format!(
            "{} / {outer}，首批出现在 +{}（等待预算 {}）",
            timeouts.len(),
            stress::millis(first_timeout),
            stress::millis(budget)
        ),
    );
    context.measure("③ 实测总耗时", stress::millis(outer_total));
    context.measure("③ 单次耗时分布", phase_distribution(&outer_phase)?.render());
    context.measure("③ 快照 limit", outer_phase.limit_line());
    context.measure(
        "收尾",
        format!(
            "不逐个 client_delete（D-P2.2-011），{} 条下行流保持到用例结束",
            inner_phase.open_streams() + outer_phase.open_streams()
        ),
    );

    Ok(vec![
        format!("① 标定：{}", calibration.render()),
        format!(
            "② N = {} 个客户端全部 run_completed，总耗时 {} ≤ T + L0 = {}；最长排队 {inner_wait}ms < T = {}",
            inner_phase.clients,
            stress::millis(inner_total),
            stress::millis(budget.saturating_add(l0)),
            stress::millis(budget)
        ),
        format!("② 服务端快照：{}", inner_phase.limit_line()),
        format!(
            "③ 2N = {outer} 个客户端：{} 条「{QUEUE_TIMEOUT_REASON}」，首批出现在 +{}",
            timeouts.len(),
            stress::millis(first_timeout)
        ),
        format!("③ 服务端快照：{}", outer_phase.limit_line()),
        "数据来源：客户端 RunEvent + 测试进程计时 + 服务端快照，不读日志".to_string(),
    ])
}

/// 标定结果：完整链路耗时（参考）、串行段耗时（公式用）与串行段的分布。
struct Calibration {
    /// 每轮「注册 → 会话 → 占用 → 订阅 → 发消息 → 终态」的全长，只用于报告对照。
    chain: Vec<Duration>,
    /// 每轮「发消息 → 终态」的耗时：串行队列里真正被占住的那一段，`L0` 取自它。
    serialized: Vec<Duration>,
    /// 串行段样本的分布。
    distribution: Distribution,
}

impl Calibration {
    /// 报告口径：`完整链路 210ms｜215ms｜250ms；串行段 55ms｜61ms｜58ms（…分布…）`。
    fn render(&self) -> String {
        format!(
            "完整链路 {}；串行段 {}（{}）",
            join_millis(&self.chain),
            join_millis(&self.serialized),
            self.distribution.render()
        )
    }
}

/// 按测得顺序拼接耗时样本：`16ms｜17ms｜41ms`。
fn join_millis(samples: &[Duration]) -> String {
    samples
        .iter()
        .map(|sample| stress::millis(*sample))
        .collect::<Vec<_>>()
        .join("｜")
}

/// ① 标定：先跑 [`stress::WARMUP_ROUNDS`] 轮预热（丢弃），再跑 [`stress::CALIBRATION_ROUNDS`] 轮，
/// 取**串行段**耗时的最大值作为 `L0`。
///
/// 串行段 = 「发消息 → 终态」：它才是 `max_concurrent_runs = 1` 时被许可串行化的部分。
/// 注册 / 建会话 / 占用 / 订阅是每个客户端各自的准备开销，**不进**串行队列；把它们计进 `L0`
/// 会把 `L0` 放大成实际的两倍，`N` 随之缩水、`2N` 压不到门槛上（实测过：`2N` 全部完成，
/// `wait_max` 远低于预算）。完整链路耗时仍然取一份写进报告，便于人工对照。
///
/// 预热轮同样不能省：第一轮要付建库、建立 Provider 连接的冷启动开销，把它算进 `L0` 会让
/// `L0` 偏大近一倍——`②` 于是白跑、`③` 又压不到门槛上（两个方向都实测过）。丢弃预热后
/// `L0` 才是 `②` / `③` 实际会遇到的那个代价。
async fn calibrate(context: &TestContext) -> Result<Calibration> {
    let deadline = Instant::now() + stress::CALIBRATION_DEADLINE;
    let rounds = stress::WARMUP_ROUNDS + stress::CALIBRATION_ROUNDS;
    let mut chain = Vec::with_capacity(stress::CALIBRATION_ROUNDS);
    let mut serialized = Vec::with_capacity(stress::CALIBRATION_ROUNDS);

    for round in 0..rounds {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "标定在 {} 内没有跑完 {} 轮（已得 {} 个有效样本）",
            stress::millis(stress::CALIBRATION_DEADLINE),
            rounds,
            serialized.len()
        );

        let chain_started = Instant::now();
        let title = format!("{CALIBRATION_TAG} #{round} {}", context.token());
        let measured = tokio::time::timeout(remaining, async {
            let mut talk = Occupied::start(context, &title).await?;
            let serialized_started = Instant::now();
            let content = format!("{CALIBRATION_TAG} #{round}");
            let (_sent, drained) = talk.say(context, &content).await?;
            ensure!(
                drained.terminal.event == RunEventKind::RunCompleted,
                "标定第 #{round} 轮应以 run_completed 收尾，实际 {:?}（错误 {:?}）",
                drained.terminal.event,
                drained.terminal.error
            );
            Ok::<Duration, anyhow::Error>(serialized_started.elapsed())
        })
        .await;

        match measured {
            Ok(Ok(one_round)) => {
                // 预热轮只付冷启动，不进样本。
                if round >= stress::WARMUP_ROUNDS {
                    serialized.push(one_round);
                    chain.push(chain_started.elapsed());
                }
            }
            Ok(Err(error)) => return Err(error),
            Err(_) => bail!(
                "标定第 #{round} 轮超过标定合计死线 {}（已得 {} 个有效样本）",
                stress::millis(stress::CALIBRATION_DEADLINE),
                serialized.len()
            ),
        }
    }

    let distribution = Distribution::of(&serialized).context("标定没有任何样本")?;
    Ok(Calibration {
        chain,
        serialized,
        distribution,
    })
}

/// 一个客户端的终态观测。
enum Outcome {
    /// 在死线内抵终态。
    Finished {
        /// 终态事件类型。
        terminal: RunEventKind,
        /// 终态状态。
        status: Option<RunStatus>,
        /// 失败原因（`run_failed` 才有）。
        error: Option<String>,
        /// 收到终态事件的时刻。
        finished_at: Instant,
    },
    /// 没在死线内抵终态。
    Unfinished {
        /// 原因：发消息失败、下行流提前结束、或超过阶段死线。
        reason: String,
    },
}

/// 一个阶段的实测结果。
struct Phase {
    /// 本阶段发起消息的客户端数。
    clients: usize,
    /// 对齐后统一发起消息的时刻。
    started: Instant,
    /// 每个客户端的终态观测，顺序与发起顺序一致。
    outcomes: Vec<Outcome>,
    /// 阶段结束时的服务端快照。
    snapshot: MetricsSnapshot,
    /// 保持打开的下行流：不主动断开，避免断连清理链与下一阶段的计时抢 CPU（D-P2.2-011）。
    held: Vec<Occupied>,
}

impl Phase {
    /// 抵终态的耗时（按客户端顺序）。
    fn finished(&self) -> Vec<Duration> {
        self.outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                Outcome::Finished { finished_at, .. } => Some(finished_at.saturating_duration_since(self.started)),
                Outcome::Unfinished { .. } => None,
            })
            .collect()
    }

    /// 最后一个终态的时刻。
    fn last_finished(&self) -> Option<Instant> {
        self.outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                Outcome::Finished { finished_at, .. } => Some(*finished_at),
                Outcome::Unfinished { .. } => None,
            })
            .max()
    }

    /// `run_completed` 的条数。
    fn completed(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| {
                matches!(
                    outcome,
                    Outcome::Finished {
                        terminal: RunEventKind::RunCompleted,
                        ..
                    }
                )
            })
            .count()
    }

    /// 排队超时（`run_failed` + 「执行排队超时」）的终态时刻。
    fn timeouts(&self) -> Vec<Instant> {
        self.outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                Outcome::Finished {
                    terminal,
                    status,
                    error,
                    finished_at,
                } if *terminal == RunEventKind::RunFailed
                    && *status == Some(RunStatus::Failed)
                    && error.as_deref() == Some(QUEUE_TIMEOUT_REASON) =>
                {
                    Some(*finished_at)
                }
                _ => None,
            })
            .collect()
    }

    /// 抵了终态但不是 `run_completed`、也不是排队超时的条数。
    fn other_failures(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| match outcome {
                Outcome::Finished { terminal, .. } => *terminal != RunEventKind::RunCompleted,
                Outcome::Unfinished { .. } => false,
            })
            .count()
            - self.timeouts().len()
    }

    /// 没在死线内抵终态的条数。
    fn unfinished(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Outcome::Unfinished { .. }))
            .count()
    }

    /// 仍保持打开的下行流条数。
    fn open_streams(&self) -> usize {
        self.held.len()
    }

    /// 快照 `limit` 段的关键字段，供断言与失败诊断共用。
    fn limit_line(&self) -> String {
        let limit = self.snapshot.limit;
        format!(
            "快照 limit：queued {}、queued_max {}、running_max {}、wait_max {}ms、timeouts {}、cancelled {}",
            limit.queued, limit.queued_max, limit.running_max, limit.wait_max_millis, limit.timeouts, limit.cancelled
        )
    }

    /// 门槛峰值：在跑 + 排队的最大同时在册数，对照 `N - 1` 与容差后的要求。
    fn peak_line(&self) -> String {
        let limit = self.snapshot.limit;
        let expected = self.clients.saturating_sub(1);
        let required = expected.saturating_sub(stress::QUEUE_PEAK_SLACK);
        format!(
            "running_max {} + queued_max {} = {}（N - 1 = {expected}，含 {} 个进队错峰容差的要求 {required}）",
            limit.running_max,
            limit.queued_max,
            limit.running_max + limit.queued_max,
            stress::QUEUE_PEAK_SLACK
        )
    }
}

/// 跑一个阶段：全部客户端先完成注册 → 会话 → 占用 → 订阅，再在同一时刻发消息并收终态。
async fn run_phase(context: &TestContext, clients: usize, tag: &str) -> Result<Phase> {
    let setup = stress::setup_deadline(clients);
    let ready = join_all((0..clients).map(|index| {
        let title = format!("{tag} #{index} {}", context.token());
        async move {
            match tokio::time::timeout(setup, Occupied::start(context, &title)).await {
                Ok(ready) => ready,
                Err(_) => bail!(
                    "客户端 #{index} 在 {} 内没有完成注册 / 占用 / 订阅",
                    stress::millis(setup)
                ),
            }
        }
    }))
    .await;

    let mut talks = Vec::with_capacity(clients);
    let mut not_ready = Vec::new();
    for (index, one) in ready.into_iter().enumerate() {
        match one {
            Ok(talk) => talks.push(talk),
            Err(error) => not_ready.push(format!("#{index}：{error:#}")),
        }
    }
    ensure!(
        not_ready.is_empty(),
        "阶段 {tag}：期望 {clients} 个客户端全部就绪，实际 {} 个未就绪：{}",
        not_ready.len(),
        not_ready.join("；")
    );

    // 对齐起跑：所有客户端睡到同一时刻再发消息，把「起始时刻」压成一个点。
    let started = Instant::now() + stress::START_ALIGN_DELAY;
    let settle = stress::settle_deadline();
    let collected = join_all(talks.into_iter().enumerate().map(|(index, mut talk)| async move {
        tokio::time::sleep_until(tokio::time::Instant::from_std(started)).await;
        let content = format!("{tag} #{index}");
        let sent = context
            .client()
            .message_create(MessageSendPO {
                session_id: talk.session.id.clone(),
                content,
            })
            .await;
        let outcome = match sent {
            Err(error) => Outcome::Unfinished {
                reason: format!("发消息失败：{}", ClientError::from_call(error)),
            },
            Ok(sent) => match tokio::time::timeout(settle, flow::drain_run(&mut talk.stream, &sent.run.id)).await {
                Ok(Ok(drained)) => Outcome::Finished {
                    terminal: drained.terminal.event,
                    status: drained.terminal.status,
                    error: drained.terminal.error,
                    finished_at: Instant::now(),
                },
                Ok(Err(error)) => Outcome::Unfinished {
                    reason: format!("{error:#}"),
                },
                Err(_) => Outcome::Unfinished {
                    reason: format!("{} 内没有终态", stress::millis(settle)),
                },
            },
        };
        (outcome, talk)
    }))
    .await;

    let mut outcomes = Vec::with_capacity(clients);
    let mut held = Vec::with_capacity(clients);
    for (outcome, talk) in collected {
        outcomes.push(outcome);
        held.push(talk);
    }

    let snapshot = capture(context).await?;
    Ok(Phase {
        clients,
        started,
        outcomes,
        snapshot,
        held,
    })
}

/// 取服务端快照：拿不到就让用例失败并点明「用 `make test` 构建」。
async fn capture(context: &TestContext) -> Result<MetricsSnapshot> {
    match server::capture(context.base_url()).await {
        Ok(metrics) => Ok(metrics),
        Err(reason) => bail!(
            "未采集服务端快照（{reason}）：边界断言只能来自快照，请用 `make test` 构建服务端（带 `metrics` 特性）后重跑"
        ),
    }
}

/// 阶段的实测总耗时：从对齐起点到最后一个终态。
fn phase_total(phase: &Phase) -> Result<Duration> {
    let last = phase.last_finished().context("阶段没有任何客户端在死线内抵终态")?;
    Ok(last.saturating_duration_since(phase.started))
}

/// 阶段的单次耗时分布。
fn phase_distribution(phase: &Phase) -> Result<Distribution> {
    Distribution::of(&phase.finished()).context("阶段没有任何客户端在死线内抵终态，无法给出耗时分布")
}

/// 内边界断言（README §4.1）：全部完成、总耗时不超过「预算 + 一个串行段」、
/// `running_max == 1`、在跑 + 排队的峰值够深、`limit.timeouts == 0`。
///
/// 总耗时的上界是 `T + L0` 而不是 `T`：被排队预算约束的是**等许可**那一段（最长排队 ≤ `T`），
/// 最后一个 Run 拿到许可后还要自己跑完一个串行段，因此末个终态必然落在 `T` 之后最多 `L0` 处。
/// 按 `≤ T` 断言与 `N = floor(T / L0) + 1` 的公式自相矛盾（`N × L0 > T`）。
fn assert_inner(phase: &Phase, budget: Duration, per_run: Duration) -> Result<()> {
    let limit = phase.limit_line();
    for (index, outcome) in phase.outcomes.iter().enumerate() {
        match outcome {
            Outcome::Unfinished { reason } => bail!(
                "阶段 {INNER_TAG}：期望 {} 个 Run 全部抵终态，实际完成 {}/{}；第 #{index} 个未结束：{reason}；{limit}",
                phase.clients,
                phase.completed(),
                phase.clients
            ),
            Outcome::Finished { terminal, error, .. } => ensure!(
                *terminal == RunEventKind::RunCompleted,
                "阶段 {INNER_TAG}：期望全部 run_completed，实际第 #{index} 个为 {terminal:?}（错误 {error:?}）；{limit}"
            ),
        }
    }

    let total = phase_total(phase)?;
    let ceiling = budget.saturating_add(per_run);
    ensure!(
        total <= ceiling,
        "阶段 {INNER_TAG}：期望最后一个终态不晚于 T + L0 = {}（T = {}、L0 = {}），实际 {}（超出 {}）；{limit}",
        stress::millis(ceiling),
        stress::millis(budget),
        stress::millis(per_run),
        stress::millis(total),
        stress::millis(total.saturating_sub(ceiling))
    );

    let snapshot = phase.snapshot.limit;
    ensure!(
        snapshot.running_max == 1,
        "阶段 {INNER_TAG}：期望 running_max == 1（并行度压到 {}），实际 {}；{limit}",
        stress::SERIAL_RUNS,
        snapshot.running_max
    );
    // 排队证据：N - 1 个 Run 应当同时排在门槛上（1 个在跑 + 其余在排队）。
    //
    // 严格按 `queued_max >= N - 1` 核对会被「进队错峰」打掉几个：N 条 `message/create` 是并发
    // 到达的，但 Run 是在服务端各自的执行任务里进队，最后一个可能比第一个晚一个 `L0` 量级才进来，
    // 而此刻第一个已经把许可交出去了（实测 5 次：峰值 = N、N、N-1、N、N-2）。因此按「在跑 + 排队」
    // 的峰值核对并留 `QUEUE_PEAK_SLACK` 的容差——这是诊断断言的容差，不是把 N 调小。
    let peak = snapshot.queued_max + snapshot.running_max;
    let required = phase.clients.saturating_sub(1).saturating_sub(stress::QUEUE_PEAK_SLACK) as i64;
    ensure!(
        peak >= required,
        "阶段 {INNER_TAG}：期望在跑 + 排队的峰值 >= {required}（N - 1 - {} 的进队错峰容差），实际 {peak}（running_max {}、queued_max {}）；{limit}",
        stress::QUEUE_PEAK_SLACK,
        snapshot.running_max,
        snapshot.queued_max
    );
    ensure!(
        snapshot.timeouts == 0,
        "阶段 {INNER_TAG}：期望 limit.timeouts == 0，实际 {}（内边界不该出现排队超时）；{limit}",
        snapshot.timeouts
    );
    Ok(())
}
