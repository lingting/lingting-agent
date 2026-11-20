//! 事件用例：事件按序落盘、无下行通道也落盘、慢客户端不阻塞落盘（RT-015）。
//!
//! 全部在进程内直接驱动 `RuntimeState` 与运行时会话：用例需要断言文件内容与序号，
//! 走 HTTP 只能看到终态，看不到「先落盘、再转发」的写序。

use std::path::{Path, PathBuf};

use agent_core::session::{RunEvent, RunEventKind, SessionCreatePO, SessionSummary};
use anyhow::{Context, Result, ensure};
use lib_core::time;
use service_core::holder;
use service_core::runtime::{RuntimeState, journal};
use tokio::sync::mpsc;
use types_core::runtime::{ClientCloseReason, OccupancyCreatePO, OccupancyVO};

use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use crate::support::{temp_directory, wait_until};

/// 事件领域用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![Case {
        name: "runtime.journal",
        kind: CaseKind::Shared,
        prep: &[],
        run: journal_order,
    }]
}

/// 写序与终态：seq 连续、顺序等于生产顺序、终止后以终态事件收尾（RT-015）。
fn journal_order(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let root = temp_directory("journal")?;
        let state = RuntimeState::with_events_root(&root);

        // 一、没有下行通道：事件仍然全部落盘（文件是唯一事实源）。
        let quiet = Fixture::new(&state, "journal-quiet", "quiet").await?;
        let producer = quiet.producer(&state)?;
        for index in 0..3 {
            producer
                .send(event(
                    &quiet,
                    RunEventKind::MessageDelta,
                    Some(format!("delta-{index}")),
                ))
                .await?;
        }
        drop(producer);
        let path = quiet.path(&root);
        ensure!(
            wait_until(|| read_events(&path).len() == 3).await,
            "没有下行通道时事件也应全部落盘"
        );
        let written = read_events(&path);
        ensure!(
            written.iter().map(|event| event.seq).collect::<Vec<_>>() == vec![0, 1, 2],
            "文件内 seq 应从 0 连续递增：{:?}",
            written.iter().map(|event| event.seq).collect::<Vec<_>>()
        );
        ensure!(
            written
                .iter()
                .filter_map(|event| event.delta.clone())
                .collect::<Vec<_>>()
                == vec!["delta-0", "delta-1", "delta-2"],
            "文件内顺序应等于生产顺序"
        );

        // 释放占用：写者追加终态事件并 flush，文件不再增长。
        state.release_occupancy(&quiet.client_id).await?;
        let written = read_events(&path);
        let terminal = written.last().context("释放后应有一条终态事件")?;
        ensure!(
            terminal.event == RunEventKind::RunCancelled,
            "释放占用的终态事件应为 run_cancelled，实际 {:?}",
            terminal.event
        );
        ensure!(
            terminal.seq == 3,
            "终态事件的 seq 应接在最后一条之后，实际 {}",
            terminal.seq
        );
        ensure!(
            terminal.error.as_deref() == Some("会话占用已释放"),
            "取消原因应为固定文本，实际 {:?}",
            terminal.error
        );

        // 二、慢客户端：下行通道塞满不阻塞落盘，该客户端被判 StreamClosed。
        let slow = Fixture::new(&state, "journal-slow", "slow").await?;
        let receiver = state
            .open_stream(&slow.client_id)
            .expect("已注册的客户端应能建立下行流");
        let producer = slow.producer(&state)?;
        let total = 80;
        for index in 0..total {
            producer
                .send(event(&slow, RunEventKind::MessageDelta, Some(format!("slow-{index}"))))
                .await?;
        }
        drop(producer);
        let path = slow.path(&root);
        ensure!(
            wait_until(|| read_events(&path).len() == total as usize).await,
            "慢客户端不应阻塞落盘：{} 行 != {total}",
            read_events(&path).len()
        );
        let marks = state.take_marks();
        ensure!(
            marks
                .iter()
                .any(|(client_id, reason)| client_id == &slow.client_id && *reason == ClientCloseReason::StreamClosed),
            "队列塞满的客户端应被标记 StreamClosed：{marks:?}"
        );
        // 接收端仍然存活：标记只来自 `try_send` 失败，而不是对端断开。
        drop(receiver);
        state.release_occupancy(&slow.client_id).await?;
        let written = read_events(&path);
        ensure!(
            written.len() == total as usize + 1,
            "终态事件应只追加一条，实际 {} 行",
            written.len()
        );
        ensure!(
            written
                .iter()
                .enumerate()
                .all(|(index, event)| event.seq == index as i64),
            "全部事件的 seq 应与写入顺序一致"
        );

        std::fs::remove_dir_all(&root).ok();
        Ok(vec![
            format!("路径：<root>/<sessionId>/<runtimeSessionId>.jsonl（{root:?}）"),
            "无下行通道时事件全部落盘，seq 从 0 连续".to_string(),
            format!("慢客户端：{total} 条事件全部落盘，客户端被判 StreamClosed"),
            "释放占用后追加一条 run_cancelled 终态事件并 flush".to_string(),
        ])
    })
}

/// 一条「客户端 + 会话 + 占用」的固定搭配。
/// 一条「客户端 + 会话 + 占用」的固定搭配。
struct Fixture {
    client_id: String,
    session: SessionSummary,
    occupancy: OccupancyVO,
}

impl Fixture {
    /// 注册客户端、建会话并建立占用（事件文件随之创建）。
    async fn new(state: &RuntimeState, client_id: &str, title: &str) -> Result<Self> {
        state.register(service_core::runtime::ClientRecord::connected(client_id));
        let session = holder::session()
            .create_session(SessionCreatePO {
                title: Some(format!("{title}-{}", time::now_millis())),
                model: None,
            })
            .context("创建会话失败")?;
        let occupancy = state
            .acquire(OccupancyCreatePO {
                client_id: client_id.to_string(),
                session_id: session.id.clone(),
            })
            .await
            .map_err(anyhow::Error::from)?;
        Ok(Self {
            client_id: client_id.to_string(),
            session,
            occupancy,
        })
    }

    /// 事件生产方的写入端。
    fn producer(&self, state: &RuntimeState) -> Result<mpsc::Sender<RunEvent>> {
        state.producer(&self.session.id).context("占用建立后应有运行时会话")
    }

    /// 本次运行时会话的事件文件路径。
    fn path(&self, root: &Path) -> PathBuf {
        journal::session_directory(root, &self.session.id).join(format!("{}.jsonl", self.occupancy.runtime_session_id))
    }
}

/// 构造一条事件；`seq` 由写者覆写，这里只填必需字段。
fn event(fixture: &Fixture, kind: RunEventKind, delta: Option<String>) -> RunEvent {
    RunEvent {
        event: kind,
        seq: 0,
        run_id: format!("run-{}", fixture.occupancy.runtime_session_id),
        session_id: fixture.session.id.clone(),
        created_at_ms: time::now_millis(),
        delta,
        message_id: None,
        status: None,
        error: None,
        input_tokens: None,
        output_tokens: None,
    }
}

/// 读出文件里已经写完整的事件行；写者可能正写到半行，因此只保留可解析的行。
fn read_events(path: &Path) -> Vec<RunEvent> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<RunEvent>(line).ok())
        .collect()
}
