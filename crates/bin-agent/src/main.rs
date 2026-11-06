//! lingting agent 单 Binary 入口：只负责组装与启动，不承载业务逻辑。

use lib_bin_cli::command::{self, Mode, ServerArgs};
use lib_core::logger;
use std::process::ExitCode;

/// 参数错误退出码。
fn exit_argument() -> ExitCode {
    ExitCode::from(2)
}

fn main() -> ExitCode {
    match command::parse().mode {
        Some(Mode::Server(args)) => server(&args),
        None => {
            // 无子命令默认进入 TUI；TUI 由 P1-015 接入，当前先输出帮助。
            command::print_help();
            ExitCode::SUCCESS
        }
    }
}

fn server(args: &ServerArgs) -> ExitCode {
    let config = match lib_bin_cli::config::resolve_server(args) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error:#}");
            return exit_argument();
        }
    };

    // 守卫必须存活到进程退出，提前释放会丢失日志并停止归档线程。
    let guard = match logger::init_server(config.log_level) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("{error:#}");
            return ExitCode::FAILURE;
        }
    };

    // 运行期错误由 serve 内部写入日志，这里只负责刷盘与退出码。
    let result = lib_bin_web::serve(&config.host, config.port);
    guard.flush();

    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
