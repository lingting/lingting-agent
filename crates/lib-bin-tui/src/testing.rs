//! 无终端驱动：把界面状态机与工作线程组合成可编程对象，供测试入口驱动。
//!
//! 只在 `testing` feature 下编译。被测路径与真实界面同源：按键走[`App::handle_key`]、
//! 更新走[`App::apply`]、渲染走[`crate::ui`] 的绘制函数，工作线程照常连真实服务端；
//! 区别只有渲染落到 [`TestBackend`]，终端既不进入 raw 模式也不写备用屏幕，
//! 因此在无人值守环境里也能驱动一整个界面会话。

use crate::ui;
use crate::worker::Worker;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

pub use crate::event::{Command, Update};
pub use crate::state::{App, Connection, Focus, Phase};
pub use crate::worker::LinkSnapshot;
pub use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub use ratatui::layout::Position;

/// 轮询更新的间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 同步轮询等待的累计统计（`wait_until`）。
/// 同步轮询等待的累计统计（`wait_until`）。
static WAIT_CALLS: AtomicU64 = AtomicU64::new(0);
/// 同步轮询等待的累计耗时（纳秒）。
static WAIT_NANOS: AtomicU64 = AtomicU64::new(0);
/// 异步轮询等待的累计统计（`wait_until_async`）：不阻塞 worker，只作对照数据。
static ASYNC_WAIT_CALLS: AtomicU64 = AtomicU64::new(0);
/// 异步轮询等待的累计耗时（纳秒）。
static ASYNC_WAIT_NANOS: AtomicU64 = AtomicU64::new(0);
/// 收尾等待（等工作线程退出）的累计统计。
static SHUTDOWN_CALLS: AtomicU64 = AtomicU64::new(0);
/// 收尾等待的累计耗时（纳秒）。
static SHUTDOWN_NANOS: AtomicU64 = AtomicU64::new(0);
/// 异步收尾等待（`close`）的累计统计：join 在阻塞线程池里，不占 worker。
static ASYNC_SHUTDOWN_CALLS: AtomicU64 = AtomicU64::new(0);
/// 异步收尾等待的累计耗时（纳秒）。
static ASYNC_SHUTDOWN_NANOS: AtomicU64 = AtomicU64::new(0);
/// 等待与阻塞的只读快照：调用次数与累计耗时。
///
/// [`Harness::wait_until`] 与 [`Harness`] 的 `Drop` 都在**调用线程**上同步阻塞，在 `async`
/// 上下文里调用等于占住一个 Tokio worker，`total_millis` 就是「占住了多久」（P2.1-002）。
/// P2.1-004 之后 `async` 用例改走 [`Harness::wait_until_async`] 与 [`Harness::close`]：
/// 同一动作记进 `async_*` 字段，只作对照数据，不构成 worker 占用。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockingSnapshot {
    /// 同步轮询等待的次数。
    pub wait_calls: u64,
    /// 同步轮询等待的累计耗时（毫秒）。
    pub wait_millis: u64,
    /// 同步收尾等待的次数。
    pub shutdown_calls: u64,
    /// 同步收尾等待的累计耗时（毫秒）。
    pub shutdown_millis: u64,
    /// 两者合计：单次运行累计占住的 worker 时间（毫秒）。
    pub total_millis: u64,
    /// 异步轮询等待的次数（`wait_until_async`，不占 worker）。
    pub async_wait_calls: u64,
    /// 异步轮询等待的累计耗时（毫秒）。
    pub async_wait_millis: u64,
    /// 异步收尾等待的次数（`close`，join 在阻塞线程池里）。
    pub async_shutdown_calls: u64,
    /// 异步收尾等待的累计耗时（毫秒）。
    pub async_shutdown_millis: u64,
}

/// 读数入口：用例运行结束后取一次，用于判断同步等待的量级（P2.1-002）。
///
/// 只读，不重置：同一进程内的多次运行会累加。
pub fn blocking_snapshot() -> BlockingSnapshot {
    let wait_millis = millis(WAIT_NANOS.load(Ordering::Relaxed));
    let shutdown_millis = millis(SHUTDOWN_NANOS.load(Ordering::Relaxed));
    BlockingSnapshot {
        wait_calls: WAIT_CALLS.load(Ordering::Relaxed),
        wait_millis,
        shutdown_calls: SHUTDOWN_CALLS.load(Ordering::Relaxed),
        shutdown_millis,
        total_millis: wait_millis + shutdown_millis,
        async_wait_calls: ASYNC_WAIT_CALLS.load(Ordering::Relaxed),
        async_wait_millis: millis(ASYNC_WAIT_NANOS.load(Ordering::Relaxed)),
        async_shutdown_calls: ASYNC_SHUTDOWN_CALLS.load(Ordering::Relaxed),
        async_shutdown_millis: millis(ASYNC_SHUTDOWN_NANOS.load(Ordering::Relaxed)),
    }
}

/// 记一次同步阻塞：只做两次原子加，不做格式化。
fn record(calls: &AtomicU64, nanos: &AtomicU64, elapsed: Duration) {
    calls.fetch_add(1, Ordering::Relaxed);
    nanos.fetch_add(elapsed.as_nanos().min(u64::MAX as u128) as u64, Ordering::Relaxed);
}

/// 纳秒转毫秒。
fn millis(nanos: u64) -> u64 {
    nanos / 1_000_000
}
/// 一帧渲染结果。
pub struct Screen {
    /// 整屏文本，行尾空白已裁掉。
    pub text: String,
    /// 终端光标位置；`None` 表示这一帧没有要求定位光标（输入焦点之外）。
    pub cursor: Option<Position>,
}

/// 无终端界面：状态机、工作线程与测试后端。
pub struct Harness {
    app: App,
    worker: Option<Worker>,
}

impl Harness {
    /// 只建界面状态、不起工作线程：只验证按键迁移与渲染的用例走这条路，
    /// 连接状态停在「连接中」，不会产生任何网络请求。
    pub fn offline(base_url: impl Into<String>) -> Self {
        Self {
            app: App::new(base_url.into()),
            worker: None,
        }
    }

    /// 起工作线程，并发出启动请求（探活 + 会话列表）。
    pub fn start(base_url: impl Into<String>) -> std::io::Result<Self> {
        let base_url = base_url.into();
        let app = App::new(base_url.clone());
        let worker = Worker::spawn(base_url)?;
        for command in app.initial_commands() {
            worker.send(command);
        }
        Ok(Self {
            app,
            worker: Some(worker),
        })
    }

    /// 连接快照：用例据此拿到 TUI 当前注册的 `clientId` 与心跳次数。
    pub fn snapshot(&self) -> LinkSnapshot {
        self.worker.as_ref().map(Worker::snapshot).unwrap_or_default()
    }

    /// 只读状态：断言直接读界面字段。
    pub fn app(&self) -> &App {
        &self.app
    }

    /// 按一次键；按键产生的请求照常发给工作线程。
    pub fn press(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let commands = self.app.handle_key(KeyEvent::new(code, modifiers));
        self.send(commands);
    }

    /// 按一次 Ctrl 组合键。
    pub fn press_ctrl(&mut self, code: KeyCode) {
        self.press(code, KeyModifiers::CONTROL);
    }

    /// 逐字键入，和终端把字符键交给界面时一致。
    pub fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            self.press(KeyCode::Char(character), KeyModifiers::NONE);
        }
    }

    /// 粘贴一段文本：与终端把一次粘贴整体送进事件循环的行为一致。
    pub fn paste(&mut self, text: &str) {
        self.app.handle_paste(text);
    }

    /// 吸收工作线程的更新，返回是否吸收到更新。
    pub fn pump(&mut self) -> bool {
        let mut updates = Vec::new();
        if let Some(worker) = self.worker.as_ref() {
            while let Some(update) = worker.try_recv() {
                updates.push(update);
            }
        }
        let absorbed = !updates.is_empty();
        for update in updates {
            let commands = self.app.apply(update);
            self.send(commands);
        }
        absorbed
    }

    /// 轮询更新直到界面满足条件；超时返回 `false`。
    ///
    /// **同步阻塞**：内部用 `std::thread::sleep` 轮询，占住调用线程直到条件成立或超时。
    /// 禁止在 `async` 上下文里调用（会占住一个 Tokio worker）：`async` 用例一律用
    /// [`Harness::wait_until_async`]（P2.1-004）。
    pub fn wait_until(&mut self, timeout: Duration, mut done: impl FnMut(&App) -> bool) -> bool {
        let started = Instant::now();
        let deadline = started + timeout;
        let finished = loop {
            self.pump();
            if done(&self.app) {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(POLL_INTERVAL);
        };
        record(&WAIT_CALLS, &WAIT_NANOS, started.elapsed());
        finished
    }

    /// 轮询更新直到界面满足条件；超时返回 `false`。
    ///
    /// **异步等待**：`pump()` + `tokio::time::sleep`，等待期间不占住 Tokio worker，
    /// 是 `async` 用例的唯一入口（P2.1-004）。
    pub async fn wait_until_async(&mut self, timeout: Duration, mut done: impl FnMut(&App) -> bool) -> bool {
        let started = Instant::now();
        let deadline = started + timeout;
        let finished = loop {
            self.pump();
            if done(&self.app) {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        };
        record(&ASYNC_WAIT_CALLS, &ASYNC_WAIT_NANOS, started.elapsed());
        finished
    }

    /// 异步收尾：把「等工作线程退出」的 join 交给阻塞线程池，不占住 Tokio worker（P2.1-004）。
    ///
    /// 与 [`Drop`] 走同一条收尾路径（`Worker::shutdown`）：先关下行通道，工作线程在收尾窗口里
    /// 送出退出时的取消请求。`async` 用例在末尾显式调用它，就不再靠 `Drop` 同步阻塞；
    /// 之后 `Drop` 没有工作线程可 join，是空操作。
    pub async fn close(&mut self) {
        if let Some(worker) = self.worker.take() {
            let started = Instant::now();
            let _ = tokio::task::spawn_blocking(move || worker.shutdown()).await;
            record(&ASYNC_SHUTDOWN_CALLS, &ASYNC_SHUTDOWN_NANOS, started.elapsed());
        }
    }

    /// 按给定尺寸渲染一帧：文本用于断言界面内容，光标用于断言输入框定位。
    pub fn render(&self, width: u16, height: u16) -> Screen {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("创建测试终端失败");
        terminal.draw(|frame| ui::draw(frame, &self.app)).expect("绘制一帧失败");
        let text = buffer_text(terminal.backend().buffer());
        // 界面只在输入焦点下要求定位光标；测试后端初始位置是 (0, 0)，
        // 这一帧没设置过就还是它，据此区分「没定位」与真实坐标。
        let position = terminal.get_cursor_position().ok();
        let cursor = position.filter(|position| *position != Position::default());
        Screen { text, cursor }
    }

    /// 按给定尺寸渲染一帧，返回裁掉行尾空白的文本。
    pub fn screen(&self, width: u16, height: u16) -> String {
        self.render(width, height).text
    }

    /// 把请求转给工作线程；工作线程已退出说明正在收尾，丢弃即可。
    fn send(&self, commands: Vec<Command>) {
        let Some(worker) = self.worker.as_ref() else {
            return;
        };
        for command in commands {
            worker.send(command);
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // 与真实退出走同一条路径：关掉下行通道后，工作线程在收尾窗口内送出退出时的取消请求。
        if let Some(worker) = self.worker.take() {
            // 收尾同样阻塞调用线程（`Worker::shutdown` 会 join 工作线程），一并计入同步阻塞。
            let started = Instant::now();
            worker.shutdown();
            record(&SHUTDOWN_CALLS, &SHUTDOWN_NANOS, started.elapsed());
        }
    }
}

/// 缓冲区转文本：行尾空白裁掉，行间用换行分隔。
///
/// 宽字符占两列，缓冲区里只有左侧单元格带内容、右侧是占位空格；
/// 按字符显示宽度推进游标，避免把占位空格写进文本。
fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    let area = buffer.area;
    let mut lines = Vec::with_capacity(area.height as usize);
    for y in area.top()..area.bottom() {
        let mut line = String::new();
        let mut x = area.left();
        while x < area.right() {
            let symbol = buffer[(x, y)].symbol();
            line.push_str(symbol);
            x += UnicodeWidthStr::width(symbol).max(1) as u16;
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}
