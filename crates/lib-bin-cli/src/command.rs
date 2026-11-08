//! 命令行定义与解析：只描述命令树与参数，不承载运行逻辑。
//!
//! 命令树：无子命令进 TUI；`tui` / `server` / `run` / `status` 各自带参数。
//! 客户端命令（`tui` / `run` / `status`）共用同一个 `--server` 参数与同一套地址解析。

use clap::{Args, Parser, Subcommand};

/// 顶层命令行。
#[derive(Debug, Parser)]
#[command(name = "agent", bin_name = "agent", version, about = "lingting agent")]
pub struct Cli {
    #[command(subcommand)]
    pub mode: Option<Mode>,
}

/// 运行模式。
#[derive(Debug, Subcommand)]
pub enum Mode {
    /// 启动终端界面（无子命令时的默认行为）
    Tui(TuiArgs),
    /// 启动 HTTP 服务（前台常驻）
    Server(ServerArgs),
    /// 执行一次任务并打印助手回复
    Run(RunArgs),
    /// 查询服务状态
    Status(StatusArgs),
}

impl Default for Mode {
    fn default() -> Self {
        Self::Tui(TuiArgs::default())
    }
}

/// `tui` 子命令参数。
#[derive(Debug, Default, Args)]
pub struct TuiArgs {
    /// 服务地址，如 `http://127.0.0.1:20100`
    #[arg(long, value_name = "ADDR")]
    pub server: Option<String>,
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

/// `run` 子命令参数。
#[derive(Debug, Args)]
pub struct RunArgs {
    /// 要执行的任务内容
    #[arg(value_name = "TASK")]
    pub task: String,

    /// 以 JSON 打印最终结果
    #[arg(long, conflicts_with = "stream")]
    pub json: bool,

    /// 逐段打印助手回复
    #[arg(long)]
    pub stream: bool,

    /// 服务地址，如 `http://127.0.0.1:20100`
    #[arg(long, value_name = "ADDR")]
    pub server: Option<String>,
}

/// `status` 子命令参数。
#[derive(Debug, Default, Args)]
pub struct StatusArgs {
    /// 服务地址，如 `http://127.0.0.1:20100`
    #[arg(long, value_name = "ADDR")]
    pub server: Option<String>,
}

/// 解析命令行参数；`--help`、`--version` 与参数错误由 clap 直接结束进程。
pub fn parse() -> Cli {
    Cli::parse()
}
