//! 界面状态：会话、消息、输入、连接与流式阶段的唯一持有者。
//!
//! 状态迁移只有两个入口：[`App::apply`] 吸收工作线程的更新，[`App::handle_key`] 处理按键，
//! 两者都返回需要发给工作线程的请求；渲染只读状态，不改状态。

use crate::event::{Command, Update};
use crate::input::Input;
use lib_bin_cli::sdk::{Message, MessageRole, SessionSummary};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// 滚动一页的行数。
const PAGE: u16 = 10;

/// 空闲期连通性巡检的间隔（tick 数）：200ms 一拍，即 5 秒。
const PROBE_TICKS: u64 = 25;

/// 等待响应的进度指示，按 tick 轮换。
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// 与 Server 的连接状态。
#[derive(Debug, Clone)]
pub enum Connection {
    /// 尚未探活，或正在重试。
    Connecting,
    /// 已连接，携带服务版本。
    Online { version: String },
    /// 未连接；失败原因在提示行展示。
    Offline,
}

/// 流式阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// 空闲，可以发送。
    Idle,
    /// 消息已发出，等 `run_started`。
    Waiting,
    /// 正在接收增量。
    Streaming,
    /// 已发出中断请求，等事件流收尾。
    Cancelling,
}

/// 焦点区域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// 输入框。
    Input,
    /// 会话列表。
    Sessions,
}

/// 一条界面消息。
pub struct ChatMessage {
    /// 消息角色，直接复用契约类型。
    pub role: MessageRole,
    /// 正文。
    pub content: String,
    /// 是否因断线/中断而不完整。
    pub interrupted: bool,
}

impl ChatMessage {
    /// 角色标签。
    pub fn label(&self) -> &'static str {
        match self.role {
            MessageRole::User => "你",
            MessageRole::Assistant => "助手",
            MessageRole::System => "系统",
            MessageRole::Tool => "工具",
        }
    }
}

/// 界面状态。
pub struct App {
    /// 服务基地址，状态栏展示用。
    pub base_url: String,
    /// 连接状态。
    pub connection: Connection,
    /// 会话列表。
    pub sessions: Vec<SessionSummary>,
    /// 会话列表里选中的下标。
    pub selected: usize,
    /// 当前会话 ID；为空表示还没有会话。
    pub current: Option<String>,
    /// 当前会话的消息。
    pub messages: Vec<ChatMessage>,
    /// 输入框。
    pub input: Input,
    /// 焦点。
    pub focus: Focus,
    /// 流式阶段。
    pub phase: Phase,
    /// 进行中的 Run ID；`Esc` 中断时用它调取消接口。
    pub run_id: Option<String>,
    /// 一行提示（错误或操作反馈）。
    pub notice: Option<String>,
    /// 消息区距底部的行数：`0` 表示跟随最新内容。
    pub back: u16,
    /// 渲染 tick 计数，驱动进度指示。
    pub tick: u64,
    /// 是否退出。
    quit: bool,
}

impl App {
    /// 初始状态：连接中，没有会话与消息。
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            connection: Connection::Connecting,
            sessions: Vec::new(),
            selected: 0,
            current: None,
            messages: Vec::new(),
            input: Input::default(),
            focus: Focus::Input,
            phase: Phase::Idle,
            run_id: None,
            notice: None,
            back: 0,
            tick: 0,
            quit: false,
        }
    }

    /// 启动时要发给工作线程的请求：探活 + 拉会话列表。
    pub fn initial_commands(&self) -> Vec<Command> {
        vec![Command::Refresh { session: None }]
    }

    /// 是否退出。
    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// 是否有进行中的回复。
    pub fn is_busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// 进度指示字符。
    pub fn spinner(&self) -> &'static str {
        SPINNER[(self.tick % SPINNER.len() as u64) as usize]
    }

    /// 渲染 tick：返回是否需要重绘，需要发的请求追加进 `commands`。
    ///
    /// 空闲且连接正常时每 [`PROBE_TICKS`] 拍巡检一次，让服务端被关掉后界面能自己发现，
    /// 不必等到用户下一次发送才发现。
    pub fn on_tick(&mut self, commands: &mut Vec<Command>) -> bool {
        self.tick = self.tick.wrapping_add(1);
        if self.tick.is_multiple_of(PROBE_TICKS)
            && !self.is_busy()
            && matches!(self.connection, Connection::Online { .. })
        {
            commands.push(Command::Probe);
        }
        self.is_busy() || matches!(self.connection, Connection::Connecting)
    }

    /// 吸收工作线程的更新，返回需要追加发出的请求。
    pub fn apply(&mut self, update: Update) -> Vec<Command> {
        match update {
            Update::Online { version } => {
                self.connection = Connection::Online { version };
                self.notice = None;
                Vec::new()
            }
            Update::Offline { message } => {
                self.phase = Phase::Idle;
                self.mark_interrupted();
                self.connection = Connection::Offline;
                self.notice = Some(format!("{message}（Ctrl+R 重试）"));
                Vec::new()
            }
            Update::Sessions(sessions) => {
                self.sessions = sessions;
                self.selected = self.selected.min(self.sessions.len().saturating_sub(1));
                Vec::new()
            }
            Update::SessionCreated(session) => {
                self.selected = 0;
                self.current = Some(session.id.clone());
                self.messages.clear();
                self.back = 0;
                self.sessions.retain(|item| item.id != session.id);
                self.sessions.insert(0, session);
                self.focus = Focus::Input;
                Vec::new()
            }
            Update::Messages { session_id, messages } => {
                self.current = Some(session_id);
                self.messages = messages.iter().map(chat_message).collect();
                self.back = 0;
                self.phase = Phase::Idle;
                Vec::new()
            }
            Update::Sent {
                session_id,
                message,
                run_id,
            } => {
                self.current.get_or_insert(session_id);
                self.messages.push(chat_message(&message));
                self.run_id = Some(run_id);
                self.phase = Phase::Waiting;
                self.notice = None;
                self.back = 0;
                self.bump_message_count();
                Vec::new()
            }
            Update::RunStarted => {
                self.phase = Phase::Streaming;
                self.push_assistant();
                Vec::new()
            }
            Update::Delta(delta) => {
                if self.phase != Phase::Cancelling {
                    self.phase = Phase::Streaming;
                }
                self.append_delta(&delta);
                self.back = 0;
                Vec::new()
            }
            Update::Completed => {
                self.phase = Phase::Idle;
                self.run_id = None;
                self.notice = None;
                self.bump_message_count();
                Vec::new()
            }
            Update::Failed { reason } => {
                self.phase = Phase::Idle;
                self.run_id = None;
                self.mark_interrupted();
                self.notice = Some(format!("Run 执行失败：{reason}"));
                Vec::new()
            }
            Update::Cancelled => {
                self.notice = Some("已受理中断请求，正在收尾".to_string());
                Vec::new()
            }
            Update::Interrupted { reason } => {
                self.phase = Phase::Idle;
                self.run_id = None;
                self.mark_interrupted();
                if matches!(self.connection, Connection::Online { .. }) {
                    self.connection = Connection::Offline;
                }
                self.notice = Some(format!("连接中断：{reason}（Ctrl+R 重试）"));
                Vec::new()
            }
            Update::StreamEnded => {
                if !self.is_busy() {
                    return Vec::new();
                }
                let cancelled = self.phase == Phase::Cancelling;
                self.phase = Phase::Idle;
                self.run_id = None;
                self.mark_interrupted();
                self.notice = Some(if cancelled {
                    "已中断当前回复".to_string()
                } else {
                    "事件流意外结束，回复可能不完整（Ctrl+R 重试）".to_string()
                });
                Vec::new()
            }
            Update::Notice { text } => {
                // 等响应阶段出的错（建会话、发消息、订阅失败）要把阶段放回去，否则永远等下去；
                // 中断请求失败时回复本身还在进行，退回到流式阶段等事件流收尾。
                self.phase = match self.phase {
                    Phase::Waiting => Phase::Idle,
                    Phase::Cancelling => Phase::Streaming,
                    phase => phase,
                };
                self.notice = Some(text);
                Vec::new()
            }
        }
    }

    /// 处理按键，返回需要发给工作线程的请求。
    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<Command> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('c') => {
                    self.quit = true;
                    Vec::new()
                }
                KeyCode::Char('n') => self.create_session(),
                KeyCode::Char('r') => self.retry(),
                _ => Vec::new(),
            };
        }
        match self.focus {
            Focus::Input => self.handle_input_key(key),
            Focus::Sessions => self.handle_sessions_key(key),
        }
    }

    /// 输入框按键。
    fn handle_input_key(&mut self, key: KeyEvent) -> Vec<Command> {
        match key.code {
            KeyCode::Enter => self.submit(),
            KeyCode::Esc => {
                if self.is_busy() {
                    return self.cancel();
                }
                self.notice = None;
                Vec::new()
            }
            KeyCode::Tab => {
                self.focus = Focus::Sessions;
                Vec::new()
            }
            KeyCode::Backspace => {
                self.input.backspace();
                Vec::new()
            }
            KeyCode::Delete => {
                self.input.delete();
                Vec::new()
            }
            KeyCode::Left => {
                self.input.move_left();
                Vec::new()
            }
            KeyCode::Right => {
                self.input.move_right();
                Vec::new()
            }
            KeyCode::Home => {
                self.input.home();
                Vec::new()
            }
            KeyCode::End => {
                self.input.end();
                Vec::new()
            }
            KeyCode::PageUp => {
                self.scroll_up();
                Vec::new()
            }
            KeyCode::PageDown => {
                self.scroll_down();
                Vec::new()
            }
            KeyCode::Char(value) => {
                self.input.insert(value);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// 会话列表按键：`q` 在这里退出，避免和输入框里的字母冲突。
    fn handle_sessions_key(&mut self, key: KeyEvent) -> Vec<Command> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                Vec::new()
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.sessions.len() {
                    self.selected += 1;
                }
                Vec::new()
            }
            KeyCode::Enter => self.open_selected(),
            KeyCode::Tab | KeyCode::Esc => {
                self.focus = Focus::Input;
                Vec::new()
            }
            KeyCode::Char('q') => {
                self.quit = true;
                Vec::new()
            }
            KeyCode::Char('n') => self.create_session(),
            _ => Vec::new(),
        }
    }

    /// 发送输入框的内容。
    fn submit(&mut self) -> Vec<Command> {
        let content = self.input.text().trim().to_string();
        if content.is_empty() {
            return Vec::new();
        }
        if self.is_busy() {
            self.notice = Some("当前回复尚未结束（Esc 中断后再发送）".to_string());
            return Vec::new();
        }
        self.input.clear();
        self.notice = None;
        self.phase = Phase::Waiting;
        vec![Command::Send {
            session_id: self.current.clone(),
            content,
        }]
    }

    /// 中断当前 Run。
    fn cancel(&mut self) -> Vec<Command> {
        let Some(run_id) = self.run_id.clone() else {
            self.notice = Some("当前没有进行中的回复".to_string());
            return Vec::new();
        };
        if self.phase == Phase::Cancelling {
            return Vec::new();
        }
        self.phase = Phase::Cancelling;
        self.notice = Some("正在中断当前回复…".to_string());
        vec![Command::Cancel { run_id }]
    }

    /// 重连：重新探活、刷新会话列表，并按需重新拉取当前会话。
    fn retry(&mut self) -> Vec<Command> {
        if self.is_busy() {
            self.notice = Some("当前回复尚未结束（Esc 中断后再重试）".to_string());
            return Vec::new();
        }
        self.connection = Connection::Connecting;
        self.notice = Some("正在重连…".to_string());
        vec![Command::Refresh {
            session: self.current.clone(),
        }]
    }

    /// 新建会话。
    fn create_session(&mut self) -> Vec<Command> {
        if self.is_busy() {
            self.notice = Some("当前回复尚未结束（Esc 中断后再试）".to_string());
            return Vec::new();
        }
        vec![Command::CreateSession]
    }

    /// 打开会话列表里选中的会话。
    fn open_selected(&mut self) -> Vec<Command> {
        if self.is_busy() {
            self.notice = Some("当前回复尚未结束（Esc 中断后再切换）".to_string());
            return Vec::new();
        }
        let Some(session) = self.sessions.get(self.selected) else {
            return Vec::new();
        };
        self.focus = Focus::Input;
        vec![Command::OpenSession {
            session_id: session.id.clone(),
        }]
    }

    /// 向上翻页：离开底部跟随。
    fn scroll_up(&mut self) {
        self.back = self.back.saturating_add(PAGE);
    }

    /// 向下翻页：回到 0 即恢复跟随最新内容。
    fn scroll_down(&mut self) {
        self.back = self.back.saturating_sub(PAGE);
    }

    /// 在消息区末尾放一条空的助手消息，等待增量写入。
    fn push_assistant(&mut self) {
        self.messages.push(ChatMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            interrupted: false,
        });
    }

    /// 追加增量；最后一条不是助手消息时先补一条。
    fn append_delta(&mut self, delta: &str) {
        match self.messages.last_mut() {
            Some(message) if message.role == MessageRole::Assistant && !message.interrupted => {
                message.content.push_str(delta);
            }
            _ => {
                self.push_assistant();
                if let Some(message) = self.messages.last_mut() {
                    message.content.push_str(delta);
                }
            }
        }
    }

    /// 会话列表里的消息数在本地自增，避免面板显示的还是建会话时的 0。
    fn bump_message_count(&mut self) {
        let Some(current) = self.current.as_deref() else {
            return;
        };
        if let Some(session) = self.sessions.iter_mut().find(|session| session.id == current) {
            session.message_count = session.message_count.saturating_add(1);
        }
    }

    /// 把最后一条有内容的助手消息标记为不完整。
    fn mark_interrupted(&mut self) {
        if let Some(message) = self.messages.last_mut()
            && message.role == MessageRole::Assistant
            && !message.content.is_empty()
        {
            message.interrupted = true;
        }
    }
}

/// 契约消息转界面消息：中断标记只属于本次会话的实时流。
fn chat_message(message: &Message) -> ChatMessage {
    ChatMessage {
        role: message.role,
        content: message.content.clone(),
        interrupted: false,
    }
}
