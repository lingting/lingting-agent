//! 日志初始化：统一目录、级别与归档策略，各入口只需按运行模式调用对应方法。
//!
//! 日志目录取自 [`crate::directorys::directorys`]：服务端、CLI、TUI 各写自己的目录，
//! 避免同一批日志文件被多个进程同时追加。

use crate::directorys::directorys;
use anyhow::{Context, Result};
use framework_core::logging::{LogArchiveConfig, LoggingConfig, LoggingGuard};
use std::path::PathBuf;
use tracing_subscriber::filter::LevelFilter;

/// 归档日志保留天数。
///
/// 归档只用于短期排查，保留过长会持续占用用户目录空间，因此固定取较小值。
const RETENTION_DAYS: u64 = 2;

/// 初始化服务端日志：控制台与 `logs` 目录同时输出。
///
/// 返回的守卫必须存活到进程退出，否则日志写入器与归档线程会被提前释放。
pub fn init_server(level: LevelFilter) -> Result<LoggingGuard> {
    let directory = directorys()?.logs_server.clone();
    init(level, directory, true)
}

/// 初始化 CLI 日志：控制台与 `logs-cli` 目录同时输出。
pub fn init_cli(level: LevelFilter) -> Result<LoggingGuard> {
    let directory = directorys()?.logs_cli.clone();
    init(level, directory, true)
}

/// 初始化 TUI 日志：仅写 `logs-tui` 目录。
///
/// TUI 独占终端，控制台日志会破坏界面，因此不开启控制台输出。
pub fn init_tui(level: LevelFilter) -> Result<LoggingGuard> {
    let directory = directorys()?.logs_tui.clone();
    init(level, directory, false)
}

fn init(level: LevelFilter, directory: PathBuf, console: bool) -> Result<LoggingGuard> {
    let mut config = LoggingConfig::new();
    config.level = level;
    config.console = console;
    config.directory = Some(directory);
    // 聚合日志会与单级别日志重复写入，额外占用空间，默认关闭。
    config.combined = false;
    config.archive = Some(LogArchiveConfig {
        retention_days: RETENTION_DAYS,
    });

    framework_core::logging::init(&config).context("初始化日志失败")
}
