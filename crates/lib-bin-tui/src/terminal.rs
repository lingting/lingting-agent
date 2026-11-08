//! 终端生命周期：进入 raw 模式与备用屏幕，退出时无论成功失败都恢复。
//!
//! `ratatui::try_init` 自己会装 panic 钩子（panic 时先恢复终端再交给原钩子），
//! 这里再补一条 `Drop`：正常退出、错误返回与提前 `return` 都走同一条恢复路径。

use crate::state::App;
use crate::ui;
use ratatui::DefaultTerminal;

/// 已进入 raw 模式与备用屏幕的终端。
pub struct Terminal {
    /// Ratatui 终端，绘制入口。
    terminal: DefaultTerminal,
}

impl Terminal {
    /// 进入 TUI 终端状态。
    pub fn enter() -> std::io::Result<Self> {
        match ratatui::try_init() {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                // 初始化可能已经开了 raw 模式（例如算不出终端尺寸），退出前补一次恢复。
                let _ = ratatui::try_restore();
                Err(error)
            }
        }
    }

    /// 绘制一帧。
    pub fn draw(&mut self, app: &App) -> std::io::Result<()> {
        self.terminal.draw(|frame| ui::draw(frame, app))?;
        Ok(())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // 恢复失败时 Ratatui 会把错误打到 stderr；此时已无法再做别的补救。
        let _ = ratatui::try_restore();
    }
}
