//! Web Server 启动与 WebUI 资源管理：Bootstrap、API 挂载、静态资源与优雅关闭。

mod cors;
mod server;
mod shutdown;
mod webui;

/// 装配参数：`bin-agent` 从 `config.toml` 解析后透传给 [`serve`]（事件文件轮转、Provider / Agent 配置）。
pub use manager_agent::AssemblyOptions;
/// 事件文件轮转配置：`AssemblyOptions::journal` 的取值来源。
pub use manager_agent::runtime::JournalConfig;
pub use server::serve;
