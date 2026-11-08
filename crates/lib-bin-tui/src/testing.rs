//! 无终端驱动：把界面状态机与工作线程组合成可编程对象，供测试入口驱动。
//!
//! 只在 `testing` feature 下编译。被测路径与真实界面同源：按键走[`App::handle_key`]、
//! 更新走[`App::apply`]、渲染走[`crate::ui`] 的绘制函数，工作线程照常连真实服务端；
//! 区别只有渲染落到 [`TestBackend`]，终端既不进入 raw 模式也不写备用屏幕，
//! 因此在无人值守环境里也能驱动一整个界面会话。

use crate::event::Command;
use crate::ui;
use crate::worker::Worker;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

pub use crate::state::{App, Connection, Focus, Phase};
pub use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub use ratatui::layout::Position;

/// 轮询更新的间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(10);

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
    pub fn wait_until(&mut self, timeout: Duration, mut done: impl FnMut(&App) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            self.pump();
            if done(&self.app) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(POLL_INTERVAL);
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
            worker.shutdown();
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
