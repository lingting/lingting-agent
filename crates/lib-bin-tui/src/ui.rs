//! 渲染：只读 [`App`]，不修改状态。
//!
//! 布局自上而下：状态栏 / 提示行（有提示时才占位）/ 主体（左会话、右消息）/ 输入框 / 快捷键。
//! 消息区自己折行，`Paragraph` 不再开 `wrap`，滚动位置直接由行数算出。

use crate::state::{App, Connection, Focus, Phase};
use crate::text;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

/// 会话区占主体的比例。
const SESSIONS_PERCENT: u16 = 28;
/// 消息正文相对正文区的缩进。
const INDENT: usize = 2;

/// 绘制整屏。
pub fn draw(frame: &mut Frame, app: &App) {
    let notice_height = u16::from(app.notice.is_some());
    let [status_area, notice_area, body_area, input_area, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(notice_height),
        Constraint::Min(3),
        // 输入框固定 5 行（边框内 3 行）：多行输入与软换行需要可见的纵向空间。
        Constraint::Length(5),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    draw_status(frame, status_area, app);
    if let Some(notice) = &app.notice {
        draw_notice(frame, notice_area, notice);
    }
    let [sessions_area, messages_area] = Layout::horizontal([
        Constraint::Percentage(SESSIONS_PERCENT),
        Constraint::Percentage(100 - SESSIONS_PERCENT),
    ])
    .areas(body_area);
    draw_sessions(frame, sessions_area, app);
    draw_messages(frame, messages_area, app);
    draw_input(frame, input_area, app);
    draw_hint(frame, hint_area, app);
}

/// 状态栏：服务地址、连接状态与流式阶段。
fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let (connection, color) = connection_text(app);
    let mut spans = vec![
        Span::styled(" agent", Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(" │ ", faded()),
        Span::styled(format!("服务 {}", app.base_url), Style::new().fg(Color::Gray)),
        Span::styled(" │ ", faded()),
        Span::styled(connection, Style::new().fg(color)),
    ];
    if let Some(phase) = phase_text(app) {
        spans.push(Span::styled(" │ ", faded()));
        spans.push(Span::styled(
            format!("{} {phase}", app.spinner()),
            Style::new().fg(Color::Yellow),
        ));
    }
    frame.render_widget(Line::from(spans), area);
}

/// 提示行：错误与操作反馈。
fn draw_notice(frame: &mut Frame, area: Rect, notice: &str) {
    frame.render_widget(
        Span::styled(format!(" ⚠ {notice}"), Style::new().fg(Color::Yellow)),
        area,
    );
}

/// 会话列表：当前会话带 `▸` 标记，选中项高亮。
fn draw_sessions(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Sessions;
    let items: Vec<ListItem> = if app.sessions.is_empty() {
        vec![ListItem::new(Span::styled("暂无会话", faded()))]
    } else {
        app.sessions
            .iter()
            .map(|session| {
                let marker = if app.current.as_deref() == Some(session.id.as_str()) {
                    "▸ "
                } else {
                    "  "
                };
                ListItem::new(Line::from(vec![
                    Span::raw(marker),
                    Span::raw(session.title.clone()),
                    Span::styled(format!(" {}", session.message_count), faded()),
                ]))
            })
            .collect()
    };

    let title = format!(" 会话（{}） ", app.sessions.len());
    let list = List::new(items)
        .block(Block::bordered().title(title).border_style(border_style(focused)))
        .highlight_style(if focused {
            Style::new().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::BOLD)
        });

    let selected = (!app.sessions.is_empty()).then_some(app.selected);
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(list, area, &mut state);
}

/// 消息区：按角色分段，助手增量边收边重绘。
fn draw_messages(frame: &mut Frame, area: Rect, app: &App) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let content_width = inner_width.saturating_sub(INDENT);
    let mut lines: Vec<Line> = Vec::new();

    for message in &app.messages {
        lines.push(Line::from(message.label()).style(role_style(message.role)));
        for line in text::wrap(&message.content, content_width) {
            lines.push(Line::from(format!("{:INDENT$}{line}", "")));
        }
        if message.interrupted {
            lines.push(
                Line::from(format!("{:INDENT$}（连接中断，内容可能不完整）", "")).style(Style::new().fg(Color::Yellow)),
            );
        }
        lines.push(Line::from(""));
    }
    if lines.is_empty() {
        lines.push(Line::from("输入内容后按 Enter 开始对话").style(faded()));
    }

    let view_height = area.height.saturating_sub(2) as usize;
    let bottom = lines.len().saturating_sub(view_height);
    let offset = bottom.saturating_sub(app.back as usize);
    let paragraph = Paragraph::new(lines)
        .block(Block::bordered().title(" 消息 "))
        .scroll((u16::try_from(offset).unwrap_or(u16::MAX), 0));
    frame.render_widget(paragraph, area);
}

/// 输入框：多行编辑、软换行与视口滚动都交给输入组件，这里只负责边框、绘制与光标定位。
fn draw_input(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Input;
    let block = Block::bordered()
        .title(" 输入 · Enter 发送 / Shift+Enter 换行 ")
        .border_style(border_style(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(&app.input, inner);

    if !focused || inner.width == 0 || inner.height == 0 {
        return;
    }
    // 组件的屏幕光标是相对整段文本的显示坐标，减去本次渲染记录的滚动起点才是框内位置。
    let cursor = app.input.screen_cursor();
    let (top_row, top_col) = app.input.scroll_offset();
    let row = cursor.row.saturating_sub(top_row as usize);
    let col = cursor.col.saturating_sub(top_col as usize);
    let x = inner.x.saturating_add(u16::try_from(col).unwrap_or(u16::MAX));
    let y = inner.y.saturating_add(u16::try_from(row).unwrap_or(u16::MAX));
    let x = x.min(inner.right().saturating_sub(1));
    let y = y.min(inner.bottom().saturating_sub(1));
    frame.set_cursor_position((x, y));
}

/// 底部快捷键提示：随焦点切换。
fn draw_hint(frame: &mut Frame, area: Rect, app: &App) {
    let hint = match app.focus {
        Focus::Input => {
            "Enter 发送 · Shift+Enter 换行 · Esc 中断 · PgUp/PgDn 滚动 · Tab 会话区 · Ctrl+N 新建 · Ctrl+R 重试 · Ctrl+D 退出"
        }
        Focus::Sessions => "↑↓ 选择 · Enter 打开 · n 新建 · q 退出 · Tab/Esc 回到输入",
    };
    frame.render_widget(Span::styled(format!(" {hint}"), faded()), area);
}

/// 连接状态的文案与颜色。
fn connection_text(app: &App) -> (String, Color) {
    match &app.connection {
        Connection::Connecting => (format!("连接中 {}", app.spinner()), Color::Yellow),
        Connection::Online { version } => (format!("已连接 v{version}"), Color::Green),
        Connection::Offline => ("未连接".to_string(), Color::Red),
    }
}

/// 流式阶段文案；空闲时不占状态栏。
fn phase_text(app: &App) -> Option<&'static str> {
    match app.phase {
        Phase::Idle => None,
        Phase::Waiting => Some("等待响应"),
        Phase::Streaming => Some("流式输出中"),
        Phase::Cancelling => Some("正在中断"),
    }
}

/// 角色标签样式。
fn role_style(role: lib_bin_cli::sdk::MessageRole) -> Style {
    use lib_bin_cli::sdk::MessageRole;
    let color = match role {
        MessageRole::User => Color::Cyan,
        MessageRole::Assistant => Color::Green,
        MessageRole::System => Color::Magenta,
        MessageRole::Tool => Color::Blue,
    };
    Style::new().fg(color).add_modifier(Modifier::BOLD)
}

/// 边框样式：焦点所在区域高亮。
fn border_style(focused: bool) -> Style {
    if focused {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().fg(Color::DarkGray)
    }
}

/// 次要文字样式。
fn faded() -> Style {
    Style::new().fg(Color::DarkGray)
}
