//! 技能面板的渲染：左侧清单、右侧详情（正文 / 缺失工具 / 参考资料）。
//!
//! 只读状态、不改状态；折行与滚动沿用消息区的做法（自己折行、`Paragraph` 不算行数）。

use crate::skill::SkillPanel;
use crate::text;
use crate::ui::{border_style, faded};
use lib_bin_cli::sdk::{SkillDetailVO, SkillStatusKind, SkillVO};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};

/// 清单区占面板主体的比例。
const LIST_PERCENT: u16 = 45;
/// 清单行里名称列的宽度：Skill 名字只含 ASCII（`^[a-z0-9][a-z0-9_-]{0,63}$`），按字符补齐即对齐。
const NAME_WIDTH: usize = 16;
/// 清单行里错误摘要的最大字符数：完整原因在详情里看（`Enter` 展开）。
const ERROR_CHARS: usize = 60;

/// 绘制面板：铺满主体区，会话与消息在面板打开期间不渲染。
pub fn draw(frame: &mut Frame, area: Rect, panel: &SkillPanel) {
    // 面板是覆盖层：先清掉底下的消息，避免残影混进清单。
    frame.render_widget(Clear, area);
    let [list_area, detail_area] = Layout::horizontal([
        Constraint::Percentage(LIST_PERCENT),
        Constraint::Percentage(100 - LIST_PERCENT),
    ])
    .areas(area);
    draw_list(frame, list_area, panel);
    draw_detail(frame, detail_area, panel);
}

/// 清单：名字、状态（`READY` / `INVALID` / 禁用）、缺失工具数与目录；`INVALID` 行带原因摘要。
fn draw_list(frame: &mut Frame, area: Rect, panel: &SkillPanel) {
    let items: Vec<ListItem> = if !panel.loaded {
        vec![ListItem::new(Span::styled("正在读取技能清单…", faded()))]
    } else if panel.items.is_empty() {
        vec![ListItem::new(Span::styled(
            "没有发现技能（目录为空或 skills 未配置）",
            faded(),
        ))]
    } else {
        panel.items.iter().map(list_item).collect()
    };

    let title = format!(" 技能（{}） ", panel.items.len());
    let list = List::new(items)
        .block(Block::bordered().title(title).border_style(border_style(true)))
        .highlight_style(Style::new().bg(Color::DarkGray).add_modifier(Modifier::BOLD));

    let selected = (!panel.items.is_empty()).then_some(panel.selected);
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(list, area, &mut state);
}

/// 一条清单项：一行摘要，`INVALID` 项多一行原因摘要。
fn list_item(skill: &SkillVO) -> ListItem<'static> {
    let (status, style) = status_of(skill);
    let missing = if skill.missing_tools.is_empty() {
        String::new()
    } else {
        format!("缺 {} 个工具", skill.missing_tools.len())
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("{:<NAME_WIDTH$}", skill.name),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{status:<8}"), style),
        Span::styled(missing, Style::new().fg(Color::Yellow)),
        Span::styled(format!("  {}", skill.directory), faded()),
    ])];
    if let Some(error) = &skill.error {
        lines.push(Line::from(format!("  ✗ {}", truncate(error, ERROR_CHARS))).style(Style::new().fg(Color::Red)));
    }
    ListItem::new(lines)
}

/// 右侧：详情到手就渲染正文，否则渲染选中项的摘要。
fn draw_detail(frame: &mut Frame, area: Rect, panel: &SkillPanel) {
    let width = area.width.saturating_sub(2) as usize;
    let view_height = area.height.saturating_sub(2) as usize;
    let (title, lines) = match &panel.detail {
        Some(detail) => (format!(" 正文：{} ", detail.name), detail_lines(detail, width)),
        None => {
            let title = match panel.selected_skill() {
                Some(skill) => format!(" {} ", skill.name),
                None => " 详情 ".to_string(),
            };
            (title, summary_lines(panel, width))
        }
    };

    let bottom = lines.len().saturating_sub(view_height);
    let offset = bottom.saturating_sub(panel.back as usize);
    let paragraph = Paragraph::new(lines)
        .block(Block::bordered().title(title).border_style(border_style(false)))
        .scroll((u16::try_from(offset).unwrap_or(u16::MAX), 0));
    frame.render_widget(paragraph, area);
}

/// 详情正文：`INVALID` 先给完整原因，其余渲染正文、缺失工具与参考资料。
fn detail_lines(detail: &SkillDetailVO, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if detail.status != SkillStatusKind::Ready {
        lines.push(Line::from("解析失败，不进索引也不可加载：").style(Style::new().fg(Color::Red)));
        for line in text::wrap(detail.error.as_deref().unwrap_or("（没有给出原因）"), width) {
            lines.push(Line::from(line).style(Style::new().fg(Color::Red)));
        }
        return lines;
    }
    if !detail.enabled {
        lines.push(Line::from("（已禁用：不进索引，也不返回正文）").style(faded()));
    } else if detail.body.trim().is_empty() {
        lines.push(Line::from("（正文为空）").style(faded()));
    } else {
        for line in text::wrap(&detail.body, width) {
            lines.push(Line::from(line));
        }
    }
    if let Some(line) = missing_tools_line(detail.missing_tools.as_slice()) {
        lines.push(line);
    }
    if !detail.references.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from("参考资料（用 file_read 读取）").style(Style::new().fg(Color::Cyan)));
        for reference in &detail.references {
            lines.push(Line::from(format!("  {reference}")).style(faded()));
        }
    }
    lines
}

/// 未取详情时的摘要：一眼看清状态、目录、声明工具与缺失工具。
fn summary_lines(panel: &SkillPanel, width: usize) -> Vec<Line<'static>> {
    if !panel.loaded {
        return vec![Line::from("正在读取技能清单…").style(faded())];
    }
    let Some(skill) = panel.selected_skill() else {
        return vec![Line::from("没有可展示的技能").style(faded())];
    };
    let (status, style) = status_of(skill);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(skill.name.clone(), Style::new().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(status, style),
        ]),
        Line::from(skill.description.clone()).style(faded()),
        Line::from(""),
        Line::from(format!("目录：{}", skill.directory)),
        Line::from(format!("声明工具：{}", join_or_none(skill.requires_tools.as_slice()))),
        Line::from(format!("缺失工具：{}", join_or_none(skill.missing_tools.as_slice()))),
        Line::from(format!("参考资料：{} 条", skill.reference_count)),
    ];
    if let Some(error) = &skill.error {
        lines.push(Line::from(""));
        for line in text::wrap(error, width) {
            lines.push(Line::from(line).style(Style::new().fg(Color::Red)));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from("d 查看正文 · Enter 启停 · r 重载 · Esc 关闭").style(faded()));
    lines
}

/// 缺失工具那一行；没有问题时不占行。
fn missing_tools_line(missing: &[String]) -> Option<Line<'static>> {
    if missing.is_empty() {
        return None;
    }
    Some(
        Line::from(format!(
            "缺少的工具：{}（涉及它们的步骤必须停止，并明确告知用户无法执行）",
            missing.join("、")
        ))
        .style(Style::new().fg(Color::Yellow)),
    )
}

/// 逗号分隔的清单；为空时给「无」。
fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "无".to_string()
    } else {
        values.join("、")
    }
}

/// 按字符截断：清单里只放摘要，完整原因在详情里看。
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>() + "…"
}

/// 状态文案与颜色：`INVALID` 先于启用标记判断，禁用只影响索引不影响清单。
fn status_of(skill: &SkillVO) -> (&'static str, Style) {
    if skill.status != SkillStatusKind::Ready {
        return ("INVALID", Style::new().fg(Color::Red));
    }
    if skill.enabled {
        return ("READY", Style::new().fg(Color::Green));
    }
    ("禁用", faded())
}
