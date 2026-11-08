//! TUI 公共组件：Ratatui 应用、状态管理、消息展示与流式增量渲染。
//!
//! 分工：
//!
//! - [`worker`] 在独立线程上跑异步请求与事件流，与界面之间只有两条无界通道；
//! - [`state`] 持有界面状态并做状态迁移，不碰 I/O；
//! - [`ui`] 只读状态、不改状态；
//! - 本模块负责终端生命周期与事件循环。
//!
//! 事件循环同时处理三类事件，互不阻塞：键盘输入由 `poll` 带超时地取，工作线程的更新由
//! 通道 `try_recv` 非阻塞地取，渲染 tick 就是那次轮询超时（只在有动画可放时重绘）。

mod event;
mod state;
mod terminal;
mod text;
mod ui;
mod worker;

#[cfg(feature = "testing")]
pub mod testing;

use lib_bin_cli::config;
use lib_bin_cli::result::CommandError;
use lib_core::logger;
use ratatui::crossterm::event::{self as terminal_event, Event, KeyEventKind};
use state::App;
use std::time::Duration;
use worker::Worker;

/// 事件轮询超时：既是渲染 tick 的间隔，也是退出与更新可见性的最大延迟。
const TICK: Duration = Duration::from_millis(200);

/// 启动 TUI。
///
/// 客户端地址与日志目录在这里统一装载：与 `run` / `status` 共用同一套解析（D-012），
/// 日志写入 `logs-tui` 且不开控制台输出（D-023），否则会打乱界面。
pub fn run(server: Option<&str>) -> Result<(), CommandError> {
    let config = config::resolve_client(server).map_err(CommandError::argument)?;
    let guard = logger::init_tui(config.log_level).map_err(CommandError::runtime)?;
    let outcome = run_app(config.base_url);
    guard.flush();
    outcome
}

/// 拉起工作线程与终端，跑完事件循环后按「退出 → 恢复终端 → 停工作线程」的顺序收尾。
///
/// 恢复终端由 [`terminal::Terminal`] 的 `Drop` 负责，因此错误路径与正常退出走同一条路。
fn run_app(base_url: String) -> Result<(), CommandError> {
    let worker = Worker::spawn(base_url.clone()).map_err(CommandError::runtime)?;
    let mut terminal = terminal::Terminal::enter().map_err(CommandError::runtime)?;
    let mut app = App::new(base_url);

    let outcome = event_loop(&mut terminal, &mut app, &worker);

    drop(terminal);
    worker.shutdown();
    outcome.map_err(CommandError::runtime)
}

/// 事件循环：吸收更新 → 重绘 → 等下一个事件。
fn event_loop(terminal: &mut terminal::Terminal, app: &mut App, worker: &Worker) -> std::io::Result<()> {
    for command in app.initial_commands() {
        worker.send(command);
    }

    let mut dirty = true;
    loop {
        while let Some(update) = worker.try_recv() {
            for command in app.apply(update) {
                worker.send(command);
            }
            dirty = true;
        }
        if app.should_quit() {
            return Ok(());
        }
        if dirty {
            terminal.draw(app)?;
            dirty = false;
        }
        // 超时返回 `false`：这一拍没有输入，交给 tick 决定是否需要重绘（流式中的进度动画）。
        if !terminal_event::poll(TICK)? {
            let mut commands = Vec::new();
            dirty = app.on_tick(&mut commands);
            for command in commands {
                worker.send(command);
            }
            continue;
        }
        match terminal_event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                for command in app.handle_key(key) {
                    worker.send(command);
                }
                dirty = true;
            }
            // 开启 bracketed paste 后，终端粘贴是整段文本，不是逐字按键；只在输入焦点下写入。
            Event::Paste(text) => {
                app.handle_paste(&text);
                dirty = true;
            }
            Event::Resize(_, _) => dirty = true,
            _ => {}
        }
    }
}
