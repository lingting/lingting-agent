//! 关闭流程用例：全部客户端按断连链收尾，运行时会话以 `server_shutdown` 结束并 flush（RT-017）。
//!
//! 直接在进程内驱动 `RuntimeState` + `handler::shutdown`：用例要断言事件文件的终态与 seq，
//! 走进程信号只能看到进程退出，看不到「终态事件已落盘」。

use std::path::{Path, PathBuf};

use agent_core::session::{RunEvent, RunEventKind, SessionCreatePO};
use anyhow::{Context, Result, ensure};
use lib_core::time;
use manager_agent::runtime::journal;
use manager_agent::runtime::{ClientRecord, RuntimeState, handler};
use types_core::runtime::OccupancyCreatePO;

use crate::case::{Case, CaseFuture, CaseKind, CaseOutcome};
use crate::context::TestContext;
use crate::support::{runtime_state, session_service, temp_directory, wait_until};

/// 关闭流程用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![Case {
        name: "runtime.shutdown",
        kind: CaseKind::Shared,
        prep: &[],
        run: shutdown,
    }]
}

/// 关闭流程：客户端反注册、占用撤回、运行时会话以 `server_shutdown` 收尾并 flush。
fn shutdown(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move { shutdown_body().await })
}

/// 用例主体：独立成 async fn，避免整块 future 在 `dyn Future + Send` 上做高阶生命周期推导。
async fn shutdown_body() -> Result<CaseOutcome> {
    let root = temp_directory("shutdown")?;
    let state = runtime_state(&root).await?;

    let mut fixtures = Vec::new();
    for index in 0..2 {
        let fixture = Fixture::new(&state, &format!("shutdown-{index}"), &root).await?;
        // 每个会话先产出一条事件：终态事件必须接在它后面，证明关闭前的内容不会被丢弃。
        let producer = state
            .producer(&fixture.session_id)
            .context("占用建立后应有运行时会话")?;
        producer.send(fixture.event(RunEventKind::RunStarted)).await?;
        drop(producer);
        ensure!(
            wait_until(|| fixture.events().len() == 1).await,
            "起手事件应已落盘：{}",
            fixture.path.display()
        );
        fixtures.push(fixture);
    }

    // 补偿路径：占用记录被提前清掉、运行时会话还在，关闭流程仍要停掉它并 flush。
    let leaked = fixtures.pop().context("应有两个用例会话")?;
    state.release(&leaked.client_id);

    let handled = handler::shutdown(&state).await;
    ensure!(handled == 2, "关闭流程应处理全部客户端，实际 {handled}");

    for fixture in fixtures.iter().chain(std::iter::once(&leaked)) {
        ensure!(
            state.client(&fixture.client_id).is_none(),
            "客户端应已反注册：{}",
            fixture.client_id
        );
        ensure!(
            state.occupancy_of_client(&fixture.client_id).is_none(),
            "占用记录应已撤回：{}",
            fixture.client_id
        );
        ensure!(
            state.producer(&fixture.session_id).is_none(),
            "运行时会话应已停止：{}",
            fixture.session_id
        );
        let events = fixture.events();
        ensure!(
            events.len() == 2,
            "应只有一条终态事件，实际 {} 条：{}",
            events.len(),
            fixture.path.display()
        );
        let terminal = &events[1];
        ensure!(
            terminal.event == RunEventKind::ServerShutdown,
            "关闭导致的终态事件应为 server_shutdown，实际 {:?}",
            terminal.event
        );
        ensure!(
            terminal.error.as_deref() == Some("服务正在关闭"),
            "终止原因应为固定文本，实际 {:?}",
            terminal.error
        );
        ensure!(
            terminal.seq == 1,
            "终态事件的 seq 应接在最后一条之后，实际 {}",
            terminal.seq
        );
    }

    ensure!(state.take_marks().is_empty(), "关闭后不应残留断连标记");
    ensure!(handler::shutdown(&state).await == 0, "重复关闭不应再处理客户端");

    std::fs::remove_dir_all(&root).ok();
    Ok(vec![
        "关闭流程处理全部客户端：反注册 → 停止运行时会话 → 撤回占用".to_string(),
        "终态事件：server_shutdown / 服务正在关闭，且已 flush".to_string(),
        "占用记录缺失的运行时会话由关闭流程兜底停止".to_string(),
    ])
}

/// 一条「客户端 + 会话 + 占用」的固定搭配。
struct Fixture {
    client_id: String,
    session_id: String,
    path: PathBuf,
}

impl Fixture {
    /// 注册客户端、建会话并建立占用（事件文件随之创建）。
    async fn new(state: &RuntimeState, client_id: &str, root: &Path) -> Result<Self> {
        state.register(ClientRecord::connected(client_id));
        let service = session_service().await?;
        let session = service
            .create_session(SessionCreatePO {
                title: Some(format!("shutdown-{}", time::now_millis())),
                model: None,
            })
            .await
            .context("创建会话失败")?;
        let occupancy = state
            .acquire(OccupancyCreatePO {
                client_id: client_id.to_string(),
                session_id: session.id.clone(),
            })
            .await
            .map_err(anyhow::Error::from)?;
        let path =
            journal::session_directory(root, &session.id).join(format!("{}.jsonl", occupancy.runtime_session_id));
        Ok(Self {
            client_id: client_id.to_string(),
            session_id: session.id,
            path,
        })
    }

    /// 构造一条事件；`seq` 由写者覆写，这里只填必需字段。
    fn event(&self, kind: RunEventKind) -> RunEvent {
        RunEvent {
            event: kind,
            seq: 0,
            run_id: format!("run-{}", self.session_id),
            session_id: self.session_id.clone(),
            created_at_ms: time::now_millis(),
            delta: None,
            message_id: None,
            status: None,
            error: None,
            input_tokens: None,
            output_tokens: None,
        }
    }

    /// 读出文件里已经写完整的事件行；写者可能正写到半行，因此只保留可解析的行。
    fn events(&self) -> Vec<RunEvent> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|line| serde_json::from_str::<RunEvent>(line).ok())
            .collect()
    }
}
