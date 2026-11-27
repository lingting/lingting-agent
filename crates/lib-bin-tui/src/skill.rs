//! 技能面板的状态：清单、选中项、详情与详情滚动位置。
//!
//! 面板不碰 I/O：打开时给出要发的清单请求，动作与详情同样只产出请求，结果经
//! [`SkillPanel::apply_list`] / [`SkillPanel::apply_detail`] 落地，与
//! [`crate::state::App`] 的「状态迁移只按更新驱动」口径一致。
//!
//! 只做四件事（D-P5-019）：列出、查看正文、启停、重载。编辑、新建与删除都不在这里：
//! 文件系统仍是唯一事实源。

use crate::event::Command;
use lib_bin_cli::sdk::{SkillAction, SkillDetailVO, SkillStatusKind, SkillVO};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// 详情区一页滚动的行数（与消息区一致）。
const PAGE: u16 = 10;

/// 面板按键的处理结果。
pub enum PanelAction {
    /// 已就地处理：移动选中项或滚动详情，没有请求要发。
    Stay,
    /// 需要发给工作线程的请求。
    Send(Command),
    /// 关闭面板。
    Close,
}

/// 技能面板。
#[derive(Default)]
pub struct SkillPanel {
    /// 清单快照：含 `INVALID` 与已禁用项，顺序与接口返回一致。
    pub items: Vec<SkillVO>,
    /// 清单里选中的下标。
    pub selected: usize,
    /// 清单是否已到手：区分「加载中」与「确实没有技能」。
    pub loaded: bool,
    /// 详情：`Some` 时右侧渲染正文，`Esc` 先回到清单。
    pub detail: Option<SkillDetailVO>,
    /// 详情请求在途。
    pub loading_detail: bool,
    /// 详情区距底部的行数。
    pub back: u16,
}

impl SkillPanel {
    /// 当前选中项；清单为空时为空。
    pub fn selected_skill(&self) -> Option<&SkillVO> {
        self.items.get(self.selected)
    }

    /// 面板按键：`Enter` 启停（`INVALID` 项改为看原因）、`d` 详情、`r` 重载、`Esc` 关闭。
    pub fn handle_key(&mut self, key: KeyEvent) -> PanelAction {
        // 面板里不提供 Ctrl 组合键功能：应用级快捷键已在 `App` 里先处理完。
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return PanelAction::Stay;
        }
        // 详情是只读滚动：先把光标键用于翻页，`Esc` 回到清单而不是直接关面板。
        if self.detail.is_some() {
            return match key.code {
                KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                    self.clear_detail();
                    PanelAction::Stay
                }
                KeyCode::PageUp => {
                    self.back = self.back.saturating_add(PAGE);
                    PanelAction::Stay
                }
                KeyCode::PageDown => {
                    self.back = self.back.saturating_sub(PAGE);
                    PanelAction::Stay
                }
                _ => PanelAction::Stay,
            };
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                PanelAction::Stay
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.items.len() {
                    self.selected += 1;
                }
                PanelAction::Stay
            }
            KeyCode::Enter => self.confirm(),
            KeyCode::Char('d') => self.ask_detail(),
            KeyCode::Char('r') => PanelAction::Send(Command::SkillUpdate {
                name: None,
                action: SkillAction::Reload,
            }),
            KeyCode::Esc => PanelAction::Close,
            _ => PanelAction::Stay,
        }
    }

    /// `Enter`：`READY` 项切换启停；`INVALID` 项没有可切换的状态，改为打开详情看完整原因。
    fn confirm(&mut self) -> PanelAction {
        let Some(skill) = self.selected_skill() else {
            return PanelAction::Stay;
        };
        if skill.status != SkillStatusKind::Ready {
            return self.ask_detail();
        }
        let name = skill.name.clone();
        let action = if skill.enabled {
            SkillAction::Disable
        } else {
            SkillAction::Enable
        };
        PanelAction::Send(Command::SkillUpdate {
            name: Some(name),
            action,
        })
    }

    /// `d`：取选中项的正文；详情还没到手前不重复上行。
    fn ask_detail(&mut self) -> PanelAction {
        if self.loading_detail {
            return PanelAction::Stay;
        }
        let Some(name) = self.selected_skill().map(|skill| skill.name.clone()) else {
            return PanelAction::Stay;
        };
        self.detail = None;
        self.loading_detail = true;
        self.back = 0;
        PanelAction::Send(Command::SkillDetail { name })
    }

    /// 收起详情回到清单。
    pub fn clear_detail(&mut self) {
        self.detail = None;
        self.loading_detail = false;
        self.back = 0;
    }

    /// 收到整张清单：就地替换并尽量保持选中的 Skill（动作与重载都返回整张清单）。
    pub fn apply_list(&mut self, items: Vec<SkillVO>) {
        let keep = self.selected_skill().map(|skill| skill.name.clone());
        self.items = items;
        self.loaded = true;
        self.clear_detail();
        self.selected = keep
            .and_then(|name| self.items.iter().position(|skill| skill.name == name))
            .unwrap_or_else(|| self.selected.min(self.items.len().saturating_sub(1)));
    }

    /// 收到详情：正文从顶部开始显示。
    pub fn apply_detail(&mut self, detail: SkillDetailVO) {
        self.detail = Some(detail);
        self.loading_detail = false;
        self.back = 0;
    }

    /// 请求失败：详情请求不再算在途，错误由提示行展示。
    pub fn clear_pending(&mut self) {
        self.loading_detail = false;
    }
}
