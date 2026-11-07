//! Web Server 启动与 WebUI 资源管理：Bootstrap、API 挂载、静态资源与优雅关闭。

mod cors;
mod server;
mod webui;

pub use server::serve;
