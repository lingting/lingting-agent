//! 事件文件：独占打开、按序追加、flush 与轮转清理（RT-015、RT-016）。
//!
//! 一条运行时会话一份文件：`<根目录>/<sessionId>/<runtimeSessionId>.jsonl`（RT-003）。
//! 文件是唯一事实源：一行一条 `RunEvent` 的 JSON，`seq` 由写者持有并覆写，写入逐条 flush
//! 但不 `sync_all`——正常关闭由关闭流程的 flush 兜底，不为每条事件付一次磁盘 IOPS。
//!
//! 轮转按会话目录计量：数量（`max_sessions`）与总字节数（`max_bytes`）满足任一即清理，
//! 最旧先删；正在占用 / 写入的目录只计入统计、不进入删除候选。

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tracing::warn;
use types_core::session::RunEvent;

/// 事件根目录名：`<应用数据目录>/events`，与 P2 的 `agent.db` 同级。
pub const EVENTS_DIRECTORY: &str = "events";
/// 事件文件扩展名。
const FILE_EXTENSION: &str = "jsonl";

/// 事件文件根目录：按应用目录推导（`lib_core::directorys` 是应用根目录的唯一事实来源）。
pub fn default_root() -> io::Result<PathBuf> {
    lib_core::directorys::directorys()
        .map(|directories| directories.data.join(EVENTS_DIRECTORY))
        .map_err(|error| io::Error::other(format!("推导事件文件根目录失败：{error:#}")))
}

/// 会话目录：轮转按目录粒度统计与删除（RT-016）。
pub fn session_directory(root: &Path, session_id: &str) -> PathBuf {
    root.join(session_id)
}

/// 一条运行时会话的事件文件写者。
pub struct Journal {
    /// 文件路径：日志与排障用。
    path: PathBuf,
    /// 带缓冲的写者：随运行时会话创建与销毁。
    writer: BufWriter<File>,
    /// 文件内序号：由写者持有并覆写，不信任事件自带的值。
    next_seq: i64,
}

impl Journal {
    /// 打开本次运行时会话的事件文件：目录按需创建，文件以追加方式打开、不截断。
    pub fn open(root: &Path, session_id: &str, runtime_session_id: &str) -> io::Result<Self> {
        let directory = session_directory(root, session_id);
        fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{runtime_session_id}.{FILE_EXTENSION}"));
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            path,
            writer: BufWriter::new(file),
            next_seq: 0,
        })
    }

    /// 事件文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 追加一条事件：补 `seq`、写一行并 flush，返回带上 `seq` 的事件（转发与文件保持一致）。
    pub fn append(&mut self, mut event: RunEvent) -> io::Result<RunEvent> {
        event.seq = self.next_seq;
        let mut line = serde_json::to_vec(&event).map_err(io::Error::other)?;
        line.push(b'\n');
        self.writer.write_all(&line)?;
        self.writer.flush()?;
        self.next_seq += 1;
        Ok(event)
    }

    /// 收尾 flush：`Drop` 里的失败无处上报，正常路径显式调用。
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

/// 默认保留的会话目录数上限（RT-003 定稿）。
pub const DEFAULT_MAX_SESSIONS: u64 = 200;
/// 默认的 `data/events` 总字节数上限：512 MiB（RT-003 定稿）。
pub const DEFAULT_MAX_BYTES: u64 = 512 * 1024 * 1024;

/// 轮转配置：`0` 表示该维度不限制；配置在启动时读一次，运行期不做热更新。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalConfig {
    /// 保留的会话目录数上限；`0` 表示不限。
    pub max_sessions: u64,
    /// `data/events` 下全部文件字节数上限；`0` 表示不限。
    pub max_bytes: u64,
}

impl Default for JournalConfig {
    fn default() -> Self {
        Self {
            max_sessions: DEFAULT_MAX_SESSIONS,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

impl JournalConfig {
    /// 用配置项的 `Option` 解析：`None` 取默认值，`Some(0)` 表示该维度关闭。
    pub fn resolve(max_sessions: Option<u64>, max_bytes: Option<u64>) -> Self {
        Self {
            max_sessions: max_sessions.unwrap_or(DEFAULT_MAX_SESSIONS),
            max_bytes: max_bytes.unwrap_or(DEFAULT_MAX_BYTES),
        }
    }

    /// 两个维度都关闭：整个轮转任务不运行（`README.md` 第六节）。
    pub fn unlimited(&self) -> bool {
        self.max_sessions == 0 && self.max_bytes == 0
    }
}

/// 一次轮转的统计结果：只用于日志与用例断言，不参与 API 契约。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RotationSummary {
    /// 清理后的会话目录数（含受保护目录）。
    pub sessions: usize,
    /// 清理后的总字节数（含受保护目录）。
    pub bytes: u64,
    /// 删除成功的会话目录名。
    pub removed: Vec<String>,
    /// 删除失败的会话目录名：跳过并继续，不中断清理。
    pub failed: Vec<String>,
}

/// 一个会话目录的统计项。
struct SessionDirectory {
    /// 目录名即会话 ID。
    name: String,
    /// 目录内全部文件的字节数之和。
    bytes: u64,
    /// 最后一次写入时间：目录内文件的最大修改时间。
    modified: SystemTime,
}

/// 轮转清理：最旧先删，删到两个维度都在限内；`protected` 里的会话目录只计入统计。
///
/// 判据只有占用表快照，删除时不二次探测文件状态（`decisions.md` RT-003 第 6 条）；
/// 删除失败（Windows 上文件被占用）跳过并继续，只记日志。
pub fn rotate(root: &Path, config: JournalConfig, protected: &HashSet<String>) -> io::Result<RotationSummary> {
    let mut directories = scan(root)?;
    let mut summary = RotationSummary {
        sessions: directories.len(),
        bytes: directories.iter().map(|directory| directory.bytes).sum(),
        ..RotationSummary::default()
    };
    if config.unlimited() {
        return Ok(summary);
    }

    directories.sort_by_key(|directory| directory.modified);
    for directory in directories {
        if within_limits(&summary, config) {
            break;
        }
        if protected.contains(&directory.name) {
            continue;
        }
        match fs::remove_dir_all(root.join(&directory.name)) {
            Ok(()) => {
                summary.sessions -= 1;
                summary.bytes = summary.bytes.saturating_sub(directory.bytes);
                summary.removed.push(directory.name);
            }
            Err(error) => {
                warn!(%error, session_id = %directory.name, "清理事件文件目录失败，跳过");
                summary.failed.push(directory.name);
            }
        }
    }
    Ok(summary)
}

/// 两个维度都在限内：`0` 表示该维度不限制。
fn within_limits(summary: &RotationSummary, config: JournalConfig) -> bool {
    let sessions_within = config.max_sessions == 0 || summary.sessions as u64 <= config.max_sessions;
    let bytes_within = config.max_bytes == 0 || summary.bytes <= config.max_bytes;
    sessions_within && bytes_within
}

/// 扫描事件文件根目录下的会话目录；根目录还不存在时视为没有历史文件。
fn scan(root: &Path) -> io::Result<Vec<SessionDirectory>> {
    let mut directories = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(directories),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        if !entry.metadata()?.is_dir() {
            continue;
        }
        let (bytes, modified) = measure(&entry.path())?;
        directories.push(SessionDirectory {
            name: entry.file_name().to_string_lossy().into_owned(),
            bytes,
            modified,
        });
    }
    Ok(directories)
}

/// 会话目录的字节数与最后写入时间：按目录内各文件累加 / 取最大值，不看文件名。
fn measure(directory: &Path) -> io::Result<(u64, SystemTime)> {
    let mut bytes = 0;
    let mut modified = fs::metadata(directory)?.modified()?;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        bytes += metadata.len();
        modified = modified.max(metadata.modified()?);
    }
    Ok((bytes, modified))
}
