//! 事件文件：独占打开、按序追加与 flush（RT-015）。
//!
//! 一条运行时会话一份文件：`<根目录>/<sessionId>/<runtimeSessionId>.jsonl`（RT-003）。
//! 文件是唯一事实源：一行一条 `RunEvent` 的 JSON，`seq` 由写者持有并覆写，写入逐条 flush
//! 但不 `sync_all`——正常关闭由关闭流程的 flush 兜底，不为每条事件付一次磁盘 IOPS。

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

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
