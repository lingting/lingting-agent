//! Web Server 启动与 WebUI 资源管理：Bootstrap、API 挂载、静态资源与优雅关闭。

mod cors;
mod server;
mod shutdown;
mod webui;

pub use server::serve;
/// 事件文件轮转配置：`bin-agent` 从 `config.toml` 解析后透传给 [`serve`]。
pub use service_core::runtime::JournalConfig;
