//! 轮转用例：会话数与字节数超限、受保护目录与关闭维度的清理口径（RT-016）。
//!
//! 直接调用 `journal::rotate` 并自己造目录：轮转的判据是文件系统状态（目录数、字节数、
//! 修改时间），走定时任务只能看到终态，看不到「最旧先删」与「受保护只计不删」。

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use agent_core::session::SessionCreatePO;
use anyhow::{Context, Result, ensure};
use lib_core::time;
use service_core::holder;
use service_core::runtime::journal::{self, JournalConfig};
use service_core::runtime::{ClientRecord, RuntimeState};
use types_core::runtime::OccupancyCreatePO;

use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use crate::support::temp_directory;

/// 造目录时每个事件文件的字节数。
const FILE_BYTES: usize = 100;
/// 造目录的间隔：轮转按修改时间判最旧，间隔几毫秒避免同一时间戳让顺序不确定。
const CREATE_GAP: Duration = Duration::from_millis(10);

/// 轮转领域用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "runtime.rotation.count",
            kind: CaseKind::Shared,
            prep: &[],
            run: count_limit,
        },
        Case {
            name: "runtime.rotation.bytes",
            kind: CaseKind::Shared,
            prep: &[],
            run: byte_limit,
        },
        Case {
            name: "runtime.rotation.both",
            kind: CaseKind::Shared,
            prep: &[],
            run: both_limits,
        },
        Case {
            name: "runtime.rotation.protect",
            kind: CaseKind::Shared,
            prep: &[],
            run: protect_occupied,
        },
        Case {
            name: "runtime.rotation.defaults",
            kind: CaseKind::Shared,
            prep: &[],
            run: defaults,
        },
    ]
}

/// 会话数超限：最旧先删，删到限内为止（字节维度关闭）。
fn count_limit(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let root = temp_directory("rotation-count")?;
        create_sessions(&root, 5)?;

        let config = JournalConfig {
            max_sessions: 2,
            max_bytes: 0,
        };
        let summary = journal::rotate(&root, config, &HashSet::new()).context("轮转失败")?;
        ensure!(
            summary.sessions == 2 && summary.bytes == total_bytes(2),
            "清理后的统计应落在限内：{summary:?}"
        );
        ensure!(
            summary.removed == names(&[0, 1, 2]),
            "应最旧先删到限内，实际删除 {:?}",
            summary.removed
        );
        ensure!(
            summary.failed.is_empty(),
            "临时目录不应有删除失败：{:?}",
            summary.failed
        );
        ensure!(
            remaining(&root) == names(&[3, 4]),
            "应只剩最新的两个目录，实际 {:?}",
            remaining(&root)
        );

        std::fs::remove_dir_all(&root).ok();
        Ok(vec![
            "max_sessions 超限：5 个会话目录删到 2 个".to_string(),
            "删除顺序：最旧先删（session-0 → session-1 → session-2）".to_string(),
        ])
    })
}

/// 字节数超限：按最旧先删，删到总字节数在限内为止（会话数维度关闭）。
fn byte_limit(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let root = temp_directory("rotation-bytes")?;
        create_sessions(&root, 5)?;

        let config = JournalConfig {
            max_sessions: 0,
            max_bytes: 250,
        };
        let summary = journal::rotate(&root, config, &HashSet::new()).context("轮转失败")?;
        ensure!(
            summary.removed == names(&[0, 1, 2]),
            "应删到总字节数在限内，实际删除 {:?}",
            summary.removed
        );
        ensure!(
            summary.bytes == total_bytes(2),
            "剩余字节数应为 2 个文件，实际 {}",
            summary.bytes
        );
        ensure!(
            remaining(&root) == names(&[3, 4]),
            "应只剩最新的两个目录，实际 {:?}",
            remaining(&root)
        );

        std::fs::remove_dir_all(&root).ok();
        Ok(vec![
            "max_bytes 超限：500 B 删到 200 B（限 250 B）".to_string(),
            "删除顺序：最旧先删".to_string(),
        ])
    })
}

/// 两者同时超限：清理到两项都在限内为止。
fn both_limits(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let root = temp_directory("rotation-both")?;
        create_sessions(&root, 5)?;

        let config = JournalConfig {
            max_sessions: 4,
            max_bytes: 250,
        };
        let summary = journal::rotate(&root, config, &HashSet::new()).context("轮转失败")?;
        ensure!(
            summary.removed == names(&[0, 1, 2]),
            "应同时满足两个维度，实际删除 {:?}",
            summary.removed
        );
        ensure!(
            summary.sessions == 2 && summary.bytes == total_bytes(2),
            "两个维度都应落在限内：{summary:?}"
        );

        std::fs::remove_dir_all(&root).ok();
        Ok(vec![
            "两个维度同时超限：删到会话数与字节数都在限内".to_string(),
            "受保护判据不影响「满足任一即清理」的语义".to_string(),
        ])
    })
}

/// 保护规则：占用中的会话目录只计入统计、永不删除（保护判据只有占用表快照）。
fn protect_occupied(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let root = temp_directory("rotation-protect")?;
        let state = RuntimeState::with_events_root(&root);
        // 先建立占用：占用中的会话目录成为最旧的那个，必然排在删除候选队首。
        let fixture = Fixture::new(&state, "rotation-protect").await?;
        create_sessions(&root, 3)?;

        let protected = state.occupied_sessions();
        ensure!(
            protected.contains(&fixture.session_id),
            "占用中的会话应在保护集合里：{protected:?}"
        );
        let config = JournalConfig {
            max_sessions: 1,
            max_bytes: 0,
        };
        let summary = journal::rotate(&root, config, &protected).context("轮转失败")?;
        ensure!(
            summary.removed == names(&[0, 1, 2]),
            "受保护目录应被跳过，实际删除 {:?}",
            summary.removed
        );
        ensure!(
            summary.sessions == 1,
            "受保护目录应计入统计，实际剩 {} 个",
            summary.sessions
        );
        ensure!(
            journal::session_directory(&root, &fixture.session_id).is_dir(),
            "占用中的会话目录不应被删除"
        );

        state.release_occupancy(&fixture.client_id).await?;
        ensure!(
            journal::session_directory(&root, &fixture.session_id).is_dir(),
            "释放占用后目录仍然保留（文件保留，不删除）"
        );

        std::fs::remove_dir_all(&root).ok();
        Ok(vec![
            "占用中的会话目录在删除候选队首也不被删除".to_string(),
            "受保护目录计入会话数与字节数统计".to_string(),
        ])
    })
}

/// 配置口径：缺省取默认值，显式 `0` 表示该维度关闭，两者都为 `0` 时整个轮转不动作。
fn defaults(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let resolved = JournalConfig::resolve(None, None);
        ensure!(
            resolved == JournalConfig::default(),
            "缺省配置应取默认值，实际 {resolved:?}"
        );
        ensure!(
            resolved.max_sessions == journal::DEFAULT_MAX_SESSIONS && resolved.max_bytes == journal::DEFAULT_MAX_BYTES,
            "默认值应为 200 个会话 / 512 MiB，实际 {resolved:?}"
        );
        ensure!(!resolved.unlimited(), "默认值不应被当成「全部关闭」");

        let closed = JournalConfig::resolve(Some(0), None);
        ensure!(
            closed.max_sessions == 0 && closed.max_bytes == journal::DEFAULT_MAX_BYTES,
            "显式 0 只关闭对应维度，实际 {closed:?}"
        );
        ensure!(
            JournalConfig::resolve(Some(0), Some(0)).unlimited(),
            "两个维度都为 0 时不需要轮转"
        );

        // 两个维度都为 0：目录原样保留，不做任何统计之外的动作。
        let root = temp_directory("rotation-unlimited")?;
        create_sessions(&root, 3)?;
        let before = remaining(&root);
        let summary =
            journal::rotate(&root, JournalConfig::resolve(Some(0), Some(0)), &HashSet::new()).context("轮转失败")?;
        ensure!(
            summary.removed.is_empty() && summary.failed.is_empty(),
            "关闭限制时不应删除任何目录：{summary:?}"
        );
        ensure!(remaining(&root) == before, "关闭限制时目录应原样保留");

        std::fs::remove_dir_all(&root).ok();
        Ok(vec![
            format!(
                "缺省值：max_sessions = {}，max_bytes = {}",
                journal::DEFAULT_MAX_SESSIONS,
                journal::DEFAULT_MAX_BYTES
            ),
            "显式 0：只关闭对应维度；两者都为 0 时整个轮转不运行".to_string(),
        ])
    })
}

/// 一条「客户端 + 会话 + 占用」的固定搭配：用来制造「占用中的会话目录」。
struct Fixture {
    client_id: String,
    session_id: String,
}

impl Fixture {
    /// 注册客户端、建会话并建立占用（事件文件随之创建）。
    async fn new(state: &RuntimeState, client_id: &str) -> Result<Self> {
        state.register(ClientRecord::connected(client_id));
        let session = holder::session()
            .create_session(SessionCreatePO {
                title: Some(format!("rotation-{}", time::now_millis())),
                model: None,
            })
            .context("创建会话失败")?;
        state
            .acquire(OccupancyCreatePO {
                client_id: client_id.to_string(),
                session_id: session.id.clone(),
            })
            .await
            .map_err(anyhow::Error::from)?;
        Ok(Self {
            client_id: client_id.to_string(),
            session_id: session.id,
        })
    }
}

/// 会话目录名：`session-<序号>`，序号越小越旧。
fn session_name(index: usize) -> String {
    format!("session-{index}")
}

/// 一组会话目录名。
fn names(indexes: &[usize]) -> Vec<String> {
    indexes.iter().copied().map(session_name).collect()
}

/// `count` 个文件的字节数之和。
fn total_bytes(count: usize) -> u64 {
    (count * FILE_BYTES) as u64
}

/// 按顺序造 `count` 个会话目录：每个目录一个事件文件，修改时间依次递增。
fn create_sessions(root: &Path, count: usize) -> Result<()> {
    for index in 0..count {
        let directory = journal::session_directory(root, &session_name(index));
        std::fs::create_dir_all(&directory).with_context(|| format!("创建目录失败：{}", directory.display()))?;
        std::fs::write(directory.join("runtime.jsonl"), vec![b'e'; FILE_BYTES])
            .with_context(|| format!("写事件文件失败：{}", directory.display()))?;
        std::thread::sleep(CREATE_GAP);
    }
    Ok(())
}

/// 根目录下现存的会话目录名，按名称排序。
fn remaining(root: &Path) -> Vec<String> {
    let mut names = std::fs::read_dir(root)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    names.sort();
    names
}
