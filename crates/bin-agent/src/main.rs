//! lingting agent 单 Binary 入口：只负责组装与启动，不承载业务逻辑。

use lib_bin_cli::command::{self, Mode, ServerArgs};
use lib_bin_cli::result::CommandError;
use lib_core::logger;
use std::process::ExitCode;

/// 参数错误退出码。
fn exit_argument() -> ExitCode {
    ExitCode::from(2)
}

fn main() -> ExitCode {
    match command::parse().mode.unwrap_or_default() {
        Mode::Tui(args) => command_result(lib_bin_tui::run(args.server.as_deref())),
        Mode::Server(args) => server(&args),
        Mode::Run(args) => command_result(lib_bin_cli::run::execute(&args)),
        Mode::Status(args) => command_result(lib_bin_cli::status::execute(&args)),
    }
}

/// 客户端命令的失败映射：参数与配置错误退出码 2，运行期错误退出码 1，错误只打一行。
fn command_result(result: Result<(), CommandError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", error.message());
            match error {
                CommandError::Argument(_) => exit_argument(),
                CommandError::Runtime(_) => ExitCode::FAILURE,
            }
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
