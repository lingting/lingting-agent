//! 应用目录集合：统一提供配置、数据与各运行模式的日志目录。

use anyhow::{Context, Result};
use arc_swap::ArcSwapOption;
use framework_core::ApplicationDirectory;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

/// 配置文件名，位于应用根目录。
const CONFIG_FILE: &str = "config.toml";
/// CLI 日志目录名称。
const LOGS_CLI_DIRECTORY: &str = "logs-cli";
/// TUI 日志目录名称。
const LOGS_TUI_DIRECTORY: &str = "logs-tui";

static DIRECTORY: LazyLock<ArcSwapOption<AppDirectories>> = LazyLock::new(ArcSwapOption::empty);

/// 应用运行所需的目录与配置文件集合。
///
/// 服务端、CLI、TUI 的日志目录彼此同级且互不覆盖，避免多进程同时写同一批日志文件。
pub struct AppDirectories {
    /// 数据目录。
    pub data: PathBuf,
    /// 配置文件路径，位于应用根目录；文件按需创建，不预先创建。
    pub config_file: PathBuf,
    /// 服务端日志目录。
    pub logs_server: PathBuf,
    /// CLI 日志目录。
    pub logs_cli: PathBuf,
    /// TUI 日志目录。
    pub logs_tui: PathBuf,
}

/// 获取应用目录集合，首次调用时创建目录并缓存。
pub fn directorys() -> Result<Arc<AppDirectories>> {
    if let Some(v) = DIRECTORY.load_full() {
        return Ok(v.clone());
    }

    let directory = ApplicationDirectory::user_in("agent", ".lingting")?;
    let root = directory
        .logs
        .parent()
        .context("无法从日志目录推导应用根目录")?
        .to_path_buf();

    let v = Arc::new(AppDirectories {
        data: directory.data,
        config_file: root.join(CONFIG_FILE),
        logs_server: directory.logs,
        logs_cli: create_directory(&root.join(LOGS_CLI_DIRECTORY))?,
        logs_tui: create_directory(&root.join(LOGS_TUI_DIRECTORY))?,
    });
    DIRECTORY.store(Some(v.clone()));
    Ok(v)
}

fn create_directory(directory: &Path) -> Result<PathBuf> {
    fs::create_dir_all(directory).with_context(|| format!("创建应用目录失败: {}", directory.display()))?;
    Ok(directory.to_path_buf())
}
