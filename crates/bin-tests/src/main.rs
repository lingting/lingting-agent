//! `bin-tests`：lingting agent 端到端测试入口。
//!
//! 职责边界：只负责参数解析、运行时启动与退出码；用例选择、资源管理与并发调度在 `runner`，
//! 通用支持（服务端子进程、运行期准备、前端命令等）在 `util`，测试替身在 `mock`，用例本身在 `cases`。

mod case;
mod cases;
mod cli;
mod mock;
mod report;
mod runner;
mod util;

use std::process::ExitCode;

fn main() -> ExitCode {
    let command = match cli::parse() {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{error:#}");
            return ExitCode::from(2);
        }
    };

    if let cli::Command::List(cases) = &command {
        cli::print_cases(cases);
        return ExitCode::SUCCESS;
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("创建 Tokio 运行时失败：{error}");
            return ExitCode::FAILURE;
        }
    };

    // 隐藏子命令 `mock-mcp`：同一个二进制既是测试入口，也是 MCP 测试替身（P4-017）。
    if let cli::Command::MockMcp(config) = command {
        return match runtime.block_on(mock::mcp::serve(config)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error:#}");
                ExitCode::FAILURE
            }
        };
    }

    let cli::Command::Run(config) = command else {
        return ExitCode::SUCCESS;
    };

    match runtime.block_on(runner::execute(config)) {
        Ok(summary) if summary.is_success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}
