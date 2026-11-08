//! 终端生命周期：进入 raw 模式与备用屏幕，退出时无论成功失败都恢复。
//!
//! `ratatui::try_init` 自己会装 panic 钩子（panic 时先恢复终端再交给原钩子），
//! 这里再补一条 `Drop`：正常退出、错误返回与提前 `return` 都走同一条恢复路径。
//! 另外开启 bracketed paste：终端粘贴以整段文本进入事件循环，恢复终端前先关掉它。

use crate::state::App;
use crate::ui;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use ratatui::crossterm::execute;

/// 已进入 raw 模式与备用屏幕的终端。
pub struct Terminal {
    /// Ratatui 终端，绘制入口。
    terminal: DefaultTerminal,
}

impl Terminal {
    /// 进入 TUI 终端状态，并开启 bracketed paste。
    pub fn enter() -> std::io::Result<Self> {
        let terminal = match ratatui::try_init() {
            Ok(terminal) => terminal,
            Err(error) => {
                // 初始化可能已经开了 raw 模式（例如算不出终端尺寸），退出前补一次恢复。
                let _ = ratatui::try_restore();
                return Err(error);
            }
        };
        // 开启后终端把一次粘贴整体送成 `Event::Paste`，界面不会收到逐字按键。
        if let Err(error) = execute!(std::io::stdout(), EnableBracketedPaste) {
            // 清理失败可以忍，初始化错误必须原样返回。
            let _ = execute!(std::io::stdout(), DisableBracketedPaste);
            let _ = ratatui::try_restore();
            return Err(error);
        }
        Ok(Self { terminal })
    }

    /// 绘制一帧。
    pub fn draw(&mut self, app: &App) -> std::io::Result<()> {
        self.terminal.draw(|frame| ui::draw(frame, app))?;
        Ok(())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // 先关掉 paste 模式再恢复 raw 模式与备用屏幕；失败时已无法再做别的补救。
        let _ = execute!(std::io::stdout(), DisableBracketedPaste);
        let _ = ratatui::try_restore();
    }
}
