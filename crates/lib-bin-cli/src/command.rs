//! 命令行定义与解析：只描述命令树与参数，不承载运行逻辑。

use clap::{Args, CommandFactory, Parser, Subcommand};

/// 顶层命令行。
#[derive(Debug, Parser)]
#[command(name = "agent", version, about = "lingting agent")]
pub struct Cli {
    #[command(subcommand)]
    pub mode: Option<Mode>,
}

/// 运行模式。
#[derive(Debug, Subcommand)]
pub enum Mode {
    /// 启动 HTTP 服务（前台常驻）
    Server(ServerArgs),
}

/// `server` 子命令参数。
#[derive(Debug, Args)]
pub struct ServerArgs {
    /// 监听地址，如 `127.0.0.1`
    #[arg(long, value_name = "ADDR")]
    pub addr: Option<String>,

    /// 监听端口，如 `20100`
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,

    /// 日志级别：off / error / warn / info / debug / trace
    #[arg(long, value_name = "LEVEL")]
    pub log_level: Option<String>,
}

/// 解析命令行参数；`--help`、`--version` 与参数错误由 clap 直接结束进程。
pub fn parse() -> Cli {
    Cli::parse()
}

/// 打印帮助信息，供尚未接入运行模式的入口使用。
pub fn print_help() {
    let _ = Cli::command().print_help();
}
