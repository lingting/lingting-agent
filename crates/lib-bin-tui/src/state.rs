//! 界面状态：会话、消息、输入、连接、流式阶段与技能面板的唯一持有者。
//!
//! 状态迁移只有两个入口：[`App::apply`] 吸收工作线程的更新，[`App::handle_key`] 处理按键，
//! 两者都返回需要发给工作线程的请求；渲染只读状态，不改状态。

use crate::event::{Command, Update};
use crate::skill::{PanelAction, SkillPanel};
use lib_bin_cli::sdk::{ClientDecision, Message, MessageRole, SessionSummary, ToolCallStatus};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
use ratatui_textarea::{TextArea, WrapMode};

/// 滚动一页的行数。
const PAGE: u16 = 10;

/// 空闲期连通性巡检的间隔（tick 数）：200ms 一拍，即 5 秒。
const PROBE_TICKS: u64 = 25;

/// 确认行上参数摘要的最大字符数：一行放得下即可，完整参数不在这里展示。
const CONFIRM_ARGS_CHARS: usize = 60;

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

/// 一条界面消息（含工具调用行）。
pub struct ChatMessage {
    /// 消息角色，直接复用契约类型；工具调用行用 [`MessageRole::Tool`]。
    pub role: MessageRole,
    /// 正文。
    pub content: String,
    /// 是否因断线/中断而不完整。
    pub interrupted: bool,
    /// 附加说明（失败原因等），渲染在正文之后。
    pub note: Option<String>,
    /// 工具调用行对应的调用标识：开始与结束事件据此落到同一行（D-P3-013）。
    pub tool_call_id: Option<String>,
}

/// 一次待确认的工具调用（D-P3-026）：只有它存在时 `y` / `n` 才生效。
pub struct PendingConfirm {
    /// 等待确认的调用标识（上行时原样带回）。
    pub tool_call_id: String,
    /// 确认行上的「工具名（参数摘要）」，恢复可应答态时据此重建该行。
    pub summary: String,
    /// 是否已上行：已提交后不重复上行，等服务端给结果。
    pub submitted: bool,
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
    pub input: TextArea<'static>,
    /// 焦点。
    pub focus: Focus,
    /// 流式阶段。
    pub phase: Phase,
    /// 进行中的 Run ID；`Esc` 中断时用它调取消接口。
    pub run_id: Option<String>,
    /// 待确认的工具调用：`None` 时 `y` / `n` 不产生任何上行（D-P3-026）。
    pub pending: Option<PendingConfirm>,
    /// 一行提示（错误或操作反馈）。
    pub notice: Option<String>,
    /// 消息区距底部的行数：`0` 表示跟随最新内容。
    pub back: u16,
    /// 渲染 tick 计数，驱动进度指示。
    pub tick: u64,
    /// 技能面板：`Some` 表示面板打开，此时按键归面板（D-P5-019）。
    pub skills: Option<SkillPanel>,
    /// 是否退出。
    quit: bool,
}

impl App {
    /// 初始状态：连接中，没有会话与消息。
    pub fn new(base_url: String) -> Self {
        let mut input = TextArea::default();
        input.set_wrap_mode(WrapMode::WordOrGlyph);
        // 光标所在行不再整行加下划线：终端光标已经指示位置，保持与消息区一致的素净观感。
        input.set_cursor_line_style(Style::default());
        input.set_placeholder_text("输入消息；Enter 发送，Shift+Enter 换行");
        Self {
            base_url,
            connection: Connection::Connecting,
            sessions: Vec::new(),
            selected: 0,
            current: None,
            messages: Vec::new(),
            input,
            focus: Focus::Input,
            phase: Phase::Idle,
            run_id: None,
            pending: None,
            notice: None,
            back: 0,
            skills: None,
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
            Update::ToolStarted { tool_call_id, name } => {
                self.upsert_tool_line(&tool_call_id, format!("→ {name}"));
                self.back = 0;
                Vec::new()
            }
            Update::ToolFinished {
                tool_call_id,
                name,
                status,
                duration_ms,
                error,
            } => {
                let text = tool_result_line(&name, status, duration_ms, error.as_deref());
                self.upsert_tool_line(&tool_call_id, text);
                self.clear_pending(&tool_call_id);
                self.back = 0;
                Vec::new()
            }
            Update::ToolConfirmRequired {
                tool_call_id,
                name,
                arguments,
            } => {
                // 一次调用只占一行：开始行升级成确认行，而不是再追加一条。
                let summary = format!("{name}（{}）", arguments_summary(&arguments));
                self.upsert_tool_line(&tool_call_id, confirm_line(&summary, false));
                self.pending = Some(PendingConfirm {
                    tool_call_id,
                    summary,
                    submitted: false,
                });
                self.back = 0;
                Vec::new()
            }
            Update::Completed => {
                self.pending = None;
                self.phase = Phase::Idle;
                self.run_id = None;
                self.notice = None;
                self.bump_message_count();
                Vec::new()
            }
            Update::Failed { reason } => {
                // 已经有助手占位就直接挂说明；还没开始执行就失败时补一条，界面不会只剩提示行。
                let started = matches!(self.phase, Phase::Streaming | Phase::Cancelling);
                self.pending = None;
                self.phase = Phase::Idle;
                self.run_id = None;
                self.fail_message(started, &reason);
                self.notice = Some(format!("Run 执行失败：{reason}"));
                Vec::new()
            }
            Update::Cancelled => {
                self.notice = Some("已受理中断请求，正在收尾".to_string());
                Vec::new()
            }
            Update::Interrupted { reason } => {
                self.pending = None;
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
                self.pending = None;
                self.phase = Phase::Idle;
                self.run_id = None;
                // 取消可能落在第一段增量之前：没有内容的助手占位整条丢弃，不留半条消息（D-011）。
                if cancelled {
                    self.drop_empty_assistant();
                }
                self.mark_interrupted();
                self.notice = Some(if cancelled {
                    "已中断当前回复".to_string()
                } else {
                    "事件流意外结束，回复可能不完整（Ctrl+R 重试）".to_string()
                });
                Vec::new()
            }
            Update::Skills(items) => {
                if let Some(panel) = self.skills.as_mut() {
                    panel.apply_list(items);
                }
                Vec::new()
            }
            Update::SkillDetail(detail) => {
                if let Some(panel) = self.skills.as_mut() {
                    panel.apply_detail(detail);
                }
                Vec::new()
            }
            Update::Notice { text } => {
                // 详情请求失败时不能一直停在「读取中」：提示行说明原因，面板保持可操作。
                if let Some(panel) = self.skills.as_mut() {
                    panel.clear_pending();
                }
                // 等响应阶段出的错（建会话、发消息、建下行流失败）要把阶段放回去，否则永远等下去；
                // 中断请求失败时回复本身还在进行，退回到流式阶段等事件流收尾。
                self.phase = match self.phase {
                    Phase::Waiting => Phase::Idle,
                    Phase::Cancelling => Phase::Streaming,
                    phase => phase,
                };
                // 上行失败时把确认行恢复成可应答：用户不必重连就能重试（见 `reset_confirm`）。
                self.reset_confirm();
                self.notice = Some(text);
                Vec::new()
            }
        }
    }

    /// 处理按键，返回需要发给工作线程的请求。
    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<Command> {
        // 应用级快捷键优先：Ctrl+D / Ctrl+N / Ctrl+R 在输入组件里另有含义（删除 / 下移 / 重做），
        // 这里以应用原有行为为准；Ctrl+C 在 TUI 里不响应（输入组件的 Ctrl+C 是复制），
        // 因此显式吞掉。其余 Ctrl 组合键不拦，交给输入组件当编辑快捷键。
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => return Vec::new(),
                KeyCode::Char('d') => return self.exit(),
                KeyCode::Char('n') => return self.create_session(),
                KeyCode::Char('r') => return self.retry(),
                _ => {}
            }
        }
        // F2 是技能面板的开关：开与关都不改变焦点，聊天输入原样保留（D-P5-019）。
        if key.code == KeyCode::F(2) {
            return self.toggle_skills();
        }
        // 面板打开时按键归面板：导航、启停、详情与关闭都在面板里收口。
        if self.skills.is_some() {
            return self.handle_skills_key(key);
        }
        // 待确认项存在时 `y` / `n` 是应答键，不再落到输入框与会话列表（会话列表里 `n` 是新建）。
        if !key.modifiers.contains(KeyModifiers::CONTROL)
            && self.pending.is_some()
            && let Some(decision) = confirm_key(key.code)
        {
            return self.answer_confirm(decision);
        }
        match self.focus {
            Focus::Input => self.handle_input_key(key),
            Focus::Sessions => self.handle_sessions_key(key),
        }
    }

    /// 粘贴：只在输入焦点下写入输入框，不触发发送。
    ///
    /// 内容整体交给输入组件，`\n` 与 `\r\n` 由它拆成多行，不逐字模拟按键。
    pub fn handle_paste(&mut self, text: &str) {
        if self.focus == Focus::Input {
            self.input.insert_str(text);
        }
    }

    /// 技能面板的开关：`F2` 与 `/skills` 共用；打开时重新拉一次清单（文件系统是唯一事实源）。
    fn toggle_skills(&mut self) -> Vec<Command> {
        if self.skills.is_some() {
            self.skills = None;
            return Vec::new();
        }
        self.skills = Some(SkillPanel::default());
        vec![Command::SkillList]
    }

    /// 面板按键：状态迁移都在 [`SkillPanel`] 里，这里只把请求与关闭动作接出去。
    fn handle_skills_key(&mut self, key: KeyEvent) -> Vec<Command> {
        let Some(panel) = self.skills.as_mut() else {
            return Vec::new();
        };
        match panel.handle_key(key) {
            PanelAction::Stay => Vec::new(),
            PanelAction::Send(command) => vec![command],
            PanelAction::Close => {
                self.skills = None;
                Vec::new()
            }
        }
    }

    /// 输入框按键：只处理应用占用的键，其余（字符、方向键、Home/End、Backspace/Delete、
    /// 各类 Ctrl 编辑快捷键）统一交给输入组件。
    fn handle_input_key(&mut self, key: KeyEvent) -> Vec<Command> {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Enter if shift => {
                self.input.insert_newline();
                Vec::new()
            }
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
            KeyCode::PageUp => {
                self.scroll_up();
                Vec::new()
            }
            KeyCode::PageDown => {
                self.scroll_down();
                Vec::new()
            }
            _ => {
                self.input.input(key);
                Vec::new()
            }
        }
    }

    /// 会话列表按键：`q` 在这里退出，避免和输入框里的字母冲突。
    fn handle_sessions_key(&mut self, key: KeyEvent) -> Vec<Command> {
        // 会话列表不提供 Ctrl 组合键功能：未被应用占用的 Ctrl 键在这里保持无操作，
        // 避免 Ctrl+J / Ctrl+K 之类被当成 j/k 导航。
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return Vec::new();
        }
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
            KeyCode::Char('q') => self.exit(),
            KeyCode::Char('n') => self.create_session(),
            _ => Vec::new(),
        }
    }

    /// 发送输入框的内容。
    fn submit(&mut self) -> Vec<Command> {
        // `/skills` 是打开技能面板的命令：它不进对话，也不占用一次 Run（与 F2 同源）。
        if self.input.lines().join("\n").trim() == "/skills" {
            self.input.clear();
            return self.toggle_skills();
        }
        let content = self.input.lines().join("\n").trim().to_string();
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

    /// 应答待确认的工具调用：`y` 批准、`n` 驳回（D-P3-026）。
    ///
    /// 发出后进入「已提交，等待结果」态：重复按键不再上行，服务端也不必处理第二次确认。
    fn answer_confirm(&mut self, decision: ClientDecision) -> Vec<Command> {
        let Some(pending) = self.pending.as_mut() else {
            return Vec::new();
        };
        if pending.submitted {
            return Vec::new();
        }
        pending.submitted = true;
        let tool_call_id = pending.tool_call_id.clone();
        let text = confirm_line(&pending.summary, true);
        self.upsert_tool_line(&tool_call_id, text);
        self.notice = Some("已提交确认，等待结果".to_string());
        vec![Command::Confirm { tool_call_id, decision }]
    }

    /// 撤销「已提交」态：确认上行失败时把这一行恢复成可应答，用户不必重连就能重试。
    fn reset_confirm(&mut self) {
        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        if !pending.submitted {
            return;
        }
        pending.submitted = false;
        let tool_call_id = pending.tool_call_id.clone();
        let text = confirm_line(&pending.summary, false);
        self.upsert_tool_line(&tool_call_id, text);
    }

    /// 该调用的结果事件已到：待确认项随之结束，`y` / `n` 不再生效。
    fn clear_pending(&mut self, tool_call_id: &str) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.tool_call_id == tool_call_id)
        {
            self.pending = None;
        }
    }

    /// 工具调用行：同一 `toolCallId` 只占一行，后到的事件改写这一行的文本。
    fn upsert_tool_line(&mut self, tool_call_id: &str, text: String) {
        if let Some(message) = self
            .messages
            .iter_mut()
            .find(|message| message.tool_call_id.as_deref() == Some(tool_call_id))
        {
            message.content = text;
            return;
        }
        self.messages.push(ChatMessage {
            role: MessageRole::Tool,
            content: text,
            interrupted: false,
            note: None,
            tool_call_id: Some(tool_call_id.to_string()),
        });
    }

    /// 退出：有进行中的回复先发取消请求，服务端不必等连接断开才发现无人接收。
    ///
    /// 已经在中断中的不再重复请求；退出前发出的请求由工作线程在收尾窗口里送出。
    fn exit(&mut self) -> Vec<Command> {
        self.quit = true;
        match self.run_id.clone() {
            Some(run_id) if self.phase != Phase::Cancelling => {
                self.phase = Phase::Cancelling;
                vec![Command::Cancel { run_id }]
            }
            _ => Vec::new(),
        }
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
            note: None,
            tool_call_id: None,
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
    /// 丢掉末尾空的助手占位：没有内容就没有半条消息可保留。
    fn drop_empty_assistant(&mut self) {
        if self
            .messages
            .last()
            .is_some_and(|message| message.role == MessageRole::Assistant && message.content.is_empty())
        {
            self.messages.pop();
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

    /// 把失败原因挂到本次回复的助手消息末尾。
    ///
    /// 已经开始出增量时占位消息就在末尾，直接挂说明；还没开始执行就失败（排队超时、上下文超预算）
    /// 时末尾没有占位，补一条空消息承接原因，避免提示行成了唯一线索。
    fn fail_message(&mut self, started: bool, reason: &str) {
        let has_placeholder = self
            .messages
            .last()
            .is_some_and(|message| message.role == MessageRole::Assistant);
        if !started || !has_placeholder {
            self.push_assistant();
        }
        if let Some(message) = self.messages.last_mut() {
            message.note = Some(format!("（执行失败：{reason}）"));
        }
    }
}

/// 契约消息转界面消息：中断标记只属于本次会话的实时流。
fn chat_message(message: &Message) -> ChatMessage {
    ChatMessage {
        role: message.role,
        content: message.content.clone(),
        interrupted: false,
        note: None,
        tool_call_id: message.tool_call_id.clone(),
    }
}

/// 确认键：`y` 批准、`n` 驳回；其余按键不参与确认（D-P3-026）。
fn confirm_key(code: KeyCode) -> Option<ClientDecision> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(ClientDecision::Approve),
        KeyCode::Char('n') | KeyCode::Char('N') => Some(ClientDecision::Reject),
        _ => None,
    }
}

/// 确认行：等待应答与已提交两种形态，只差结尾的提示。
fn confirm_line(summary: &str, submitted: bool) -> String {
    if submitted {
        format!("⚠ 需要确认：{summary} 已提交，等待结果")
    } else {
        format!("⚠ 需要确认：{summary} [y] 允许 / [n] 拒绝")
    }
}

/// 参数摘要：已脱敏的 JSON 文本按字符截断，保证确认行只占一行。
fn arguments_summary(arguments: &str) -> String {
    let text = arguments.trim();
    if text.chars().count() <= CONFIRM_ARGS_CHARS {
        return text.to_string();
    }
    text.chars().take(CONFIRM_ARGS_CHARS).collect::<String>() + "…"
}

/// 工具结果行：成功带耗时，失败带服务端给的模型可读文本（未授权按固定文案）。
fn tool_result_line(name: &str, status: ToolCallStatus, duration_ms: i64, error: Option<&str>) -> String {
    match status {
        ToolCallStatus::Ok => format!("✓ {name}（{duration_ms}ms）"),
        ToolCallStatus::Denied => format!("✗ {name}：工具未授权"),
        ToolCallStatus::Cancelled => format!("⊘ {name}：已取消"),
        // `PENDING` 不应出现在完成事件里：真的出现时按失败展示，避免界面把未知结果当成功。
        ToolCallStatus::Error
        | ToolCallStatus::Timeout
        | ToolCallStatus::InvalidArguments
        | ToolCallStatus::Pending => {
            format!("✗ {name}：{}", error.unwrap_or("工具执行失败"))
        }
    }
}
