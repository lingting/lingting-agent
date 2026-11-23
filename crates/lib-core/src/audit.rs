//! 日志审计（P2.2-003）：把「日志里发生了什么」变成进程内计数。
//!
//! 原来靠检索 `server.log` 得到的三个事实，改在这里统计：
//!
//! 1. 事件级别分布（`total` / `error` / `warn`）——证明日志确实在产出，且没有 ERROR 级事件；
//! 2. 敏感串命中（`secret_hits`，如 `[provider] api_key`）——在**事件构造时**比对，命中即计数；
//! 3. 进程内 panic 次数（`panics`）——接管 panic 钩子，先调既有钩子再累加。
//!
//! **只在 `metrics` 特性下编译**：正式构建里本模块不存在，日志初始化路径与改动前一致。
//!
//! 审计只加计数：不改写事件、不做过滤、不落盘，也不把命中的明文写进任何输出。
//! 热路径不分配堆内存——`&str` 字段直接比对，Debug / Display 字段写进栈上定长缓冲。

use std::fmt::{self, Write as _};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once};

use framework_core::logging::LoggingLayer;
use tracing::field::{Field, Visit};
use tracing::{Event, Level};
use tracing_subscriber::Registry;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Context, Layer};

/// 事件总数。
static TOTAL: AtomicU64 = AtomicU64::new(0);
/// `ERROR` 级事件数。
static ERROR: AtomicU64 = AtomicU64::new(0);
/// `WARN` 级事件数。
static WARN: AtomicU64 = AtomicU64::new(0);
/// 敏感串命中的字段数（不区分是哪个敏感串）。
static SECRET_HITS: AtomicU64 = AtomicU64::new(0);
/// 进程内 panic 次数。
static PANICS: AtomicU64 = AtomicU64::new(0);

/// 待监控的敏感串：日志初始化前登记，构造审计层时取一份快照。
static SECRETS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// 审计快照：只读、取一次即可，不重置。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuditSnapshot {
    /// 事件总数。
    pub total: u64,
    /// `ERROR` 级事件数。
    pub error: u64,
    /// `WARN` 级事件数。
    pub warn: u64,
    /// 敏感串命中的字段数；`0` 表示日志里没有出现被监控的值。
    pub secret_hits: u64,
    /// 进程内 panic 次数。
    pub panics: u64,
}

/// 只读快照。
pub fn snapshot() -> AuditSnapshot {
    AuditSnapshot {
        total: TOTAL.load(Ordering::Relaxed),
        error: ERROR.load(Ordering::Relaxed),
        warn: WARN.load(Ordering::Relaxed),
        secret_hits: SECRET_HITS.load(Ordering::Relaxed),
        panics: PANICS.load(Ordering::Relaxed),
    }
}

/// 登记一个不许进日志的敏感串（空串忽略）。
///
/// 必须在 [`crate::logger`] 初始化**之前**调用：审计层在构造时取一次快照。
pub fn monitor_secret(value: &str) {
    if value.is_empty() {
        return;
    }
    let mut secrets = SECRETS.lock().unwrap_or_else(|error| error.into_inner());
    if !secrets.iter().any(|secret| secret == value) {
        secrets.push(value.to_owned());
    }
}

/// 安装 panic 计数钩子：先调用既有钩子（保留原输出），再累加计数。重复调用无副作用。
pub fn install_panic_hook() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PANICS.fetch_add(1, Ordering::Relaxed);
            previous(info);
        }));
    });
}

/// 构造审计层：与内置层并列挂到 subscriber 上，级别跟随日志配置。
pub fn layer(level: LevelFilter) -> LoggingLayer {
    let secrets = SECRETS.lock().map(|secrets| secrets.clone()).unwrap_or_default();
    Box::new(AuditLayer { secrets }.with_filter(level))
}

/// 审计层本体：只做计数。
struct AuditLayer {
    /// 登记过的敏感串；为空时完全跳过字段比对。
    secrets: Vec<String>,
}

impl Layer<Registry> for AuditLayer {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, Registry>) {
        TOTAL.fetch_add(1, Ordering::Relaxed);
        match *event.metadata().level() {
            Level::ERROR => {
                ERROR.fetch_add(1, Ordering::Relaxed);
            }
            Level::WARN => {
                WARN.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        if self.secrets.is_empty() {
            return;
        }
        let mut visitor = SecretVisitor {
            secrets: &self.secrets,
            hits: 0,
        };
        // 字段访问器：`&str` 字段零分配比对，其余字段写进栈上定长缓冲。
        event.record(&mut visitor);
        if visitor.hits > 0 {
            SECRET_HITS.fetch_add(visitor.hits, Ordering::Relaxed);
        }
    }
}

/// 字段访问器：命中的字段值只加计数，不保存、不打印。
struct SecretVisitor<'a> {
    /// 登记过的敏感串。
    secrets: &'a [String],
    /// 本次事件里命中的字段数。
    hits: u64,
}

impl SecretVisitor<'_> {
    /// 比对一段文本：命中即计数。
    fn inspect(&mut self, text: &str) {
        if self.secrets.iter().any(|secret| text.contains(secret.as_str())) {
            self.hits += 1;
        }
    }
}

impl Visit for SecretVisitor<'_> {
    fn record_str(&mut self, _field: &Field, value: &str) {
        self.inspect(value);
    }

    fn record_debug(&mut self, _field: &Field, value: &dyn fmt::Debug) {
        // `%` / `?` 两类字段都落到这里：只付一次栈上格式化，不分配堆内存。
        // 格式化超长时缓冲会中止产出（返回错误），此处只看前缀，因此错误照旧忽略。
        let mut buffer = InlineBuffer::new();
        let _ = write!(&mut buffer, "{value:?}");
        self.inspect(buffer.as_str());
    }
}

/// 栈上定长格式化缓冲：写满即中止格式化（返回错误），只保留前缀，不分配堆内存。
struct InlineBuffer {
    /// 已写入的 UTF-8 字节。
    bytes: [u8; INLINE_BYTES],
    /// 已写入长度。
    len: usize,
    /// 是否已经放不下：置位后拒绝继续写入，避免跨过缺口拼接出错误文本。
    overflow: bool,
}

/// 单个字段参与比对的字节上限：敏感串远短于此，超出的尾部不参与判定。
const INLINE_BYTES: usize = 512;

impl InlineBuffer {
    /// 新建空缓冲。
    fn new() -> Self {
        Self {
            bytes: [0; INLINE_BYTES],
            len: 0,
            overflow: false,
        }
    }

    /// 已写入内容；只写入完整分片，因此总是合法 UTF-8。
    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).unwrap_or_default()
    }
}

impl fmt::Write for InlineBuffer {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.overflow {
            return Err(fmt::Error);
        }
        let Some(end) = self.len.checked_add(value.len()).filter(|end| *end <= INLINE_BYTES) else {
            // 放不下就中止格式化：超长字段不再继续产出，比对只看已写入的前缀。
            self.overflow = true;
            return Err(fmt::Error);
        };
        self.bytes[self.len..end].copy_from_slice(value.as_bytes());
        self.len = end;
        Ok(())
    }
}
