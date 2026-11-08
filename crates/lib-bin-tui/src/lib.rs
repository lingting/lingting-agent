//! TUI 公共组件：Ratatui 应用、状态管理、消息展示与流式增量渲染。

use lib_bin_cli::config;
use lib_bin_cli::result::CommandError;
use lib_core::logger;

/// 启动 TUI。
///
/// 客户端地址与日志目录在这里统一装载：与 `run` / `status` 共用同一套解析（D-012），
/// 日志写入 `logs-tui` 且不开控制台输出（D-023）。
///
/// 界面与事件循环由 P1-015 实现，当前只完成接入点：地址解析、日志初始化与错误返回。
pub fn run(server: Option<&str>) -> Result<(), CommandError> {
    let config = config::resolve_client(server).map_err(CommandError::argument)?;
    let guard = logger::init_tui(config.log_level).map_err(CommandError::runtime)?;
    guard.flush();

    Err(CommandError::runtime("TUI 尚未接入（P1-015）"))
}
