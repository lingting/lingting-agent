//! 命令行参数：用例选择、列表与并发度；外加一个隐藏的 MCP 测试替身子命令。
//!
//! 替身子命令**不进主参数解析器**：`bin-tests --help` 里不出现 `mock-mcp`，只有直接执行
//! `bin-tests mock-mcp …`（服务端把它当子进程启动）才进入替身参数解析。这样做是因为主解析器
//! 的 `CASE` 是自由位置参数，把子命令混进去会让 `bin-tests mcp` 这类选择器有歧义。

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::case::Case;
use crate::cases;
use crate::mock::mcp::{MockConfig, MockTransport, Scenario};
use crate::runner::Config;
use anyhow::{Context, Result, bail};
use clap::Parser;

/// 隐藏子命令名。
const MOCK_MCP: &str = "mock-mcp";

/// 替身子命令的参数（不进 `--help`）。
#[derive(Debug, Parser)]
#[command(name = "bin-tests mock-mcp", about = "MCP 测试替身")]
struct MockMcpArgs {
    /// 传输方式：stdio（被服务端当子进程启动）或 http（用例自己启动）
    #[arg(long, value_name = "stdio|http", default_value = "stdio")]
    transport: String,

    /// HTTP 监听端口（`--transport http` 必填）
    #[arg(long, value_name = "N")]
    port: Option<u16>,

    /// 剧本名（normal / naming / schema / results / error / crash / slow / list_changed）
    #[arg(long, value_name = "NAME", default_value = "normal")]
    scenario: String,

    /// 证据文件路径：客户端信息与每次调用的结构化快照
    #[arg(long, value_name = "PATH")]
    evidence: Option<PathBuf>,

    /// PID 文件路径：「无孤儿进程」断言用
    #[arg(long, value_name = "PATH")]
    pid_file: Option<PathBuf>,
}

/// 顶层参数。
#[derive(Debug, Parser)]
#[command(
    name = "bin-tests",
    bin_name = "bin-tests",
    version,
    about = "lingting agent 端到端测试入口"
)]
struct Args {
    /// 用例名前缀，可传多个；例如 session 选中 session.*
    #[arg(value_name = "CASE")]
    selectors: Vec<String>,

    /// 运行全部用例
    #[arg(long)]
    all: bool,

    /// 只列出用例，不执行
    #[arg(long)]
    list: bool,

    /// 并发用例数，默认 CPU 内核数
    #[arg(long, value_name = "N")]
    concurrency: Option<usize>,
}

/// 解析结果：执行选中的用例、只列出全部用例，或运行 MCP 测试替身。
pub enum Command {
    /// 执行选中的用例。
    Run(Config),
    /// 列出全部用例。
    List(Vec<Case>),
    /// 运行 MCP 测试替身（隐藏子命令）。
    MockMcp(MockConfig),
}

/// 解析命令行；用法错误以 `Err` 返回，由调用方按参数错误退出。
pub fn parse() -> Result<Command> {
    if let Some(command) = mock_command()? {
        return Ok(command);
    }
    let args = Args::parse();
    let all = cases::all();

    if args.list {
        return Ok(Command::List(all));
    }
    if args.all && !args.selectors.is_empty() {
        bail!("--all 不能与用例名前缀同时使用");
    }
    if !args.all && args.selectors.is_empty() {
        bail!("必须指定用例名前缀或 --all，例如 make test ARGS=\"--all\"");
    }

    let concurrency = args.concurrency.unwrap_or_else(default_concurrency);
    if concurrency == 0 {
        bail!("--concurrency 必须大于 0");
    }

    let selected = if args.all { all } else { select(all, &args.selectors)? };

    Ok(Command::Run(Config {
        cases: selected,
        concurrency,
    }))
}

/// 打印全部用例，供 `--list` 与排查使用。
pub fn print_cases(cases: &[Case]) {
    for case in cases {
        println!("{:<20} {}", case.name, case.kind.label());
    }
}

/// 默认并发度：CPU 内核数。
fn default_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
}

/// 按 `<领域>.<场景>` 的点边界做前缀匹配；多个选择器取并集。
fn select(cases: Vec<Case>, selectors: &[String]) -> Result<Vec<Case>> {
    let mut selected: Vec<Case> = Vec::new();

    for selector in selectors {
        let matched: Vec<Case> = cases
            .iter()
            .filter(|case| matches_prefix(case.name, selector))
            .copied()
            .collect();
        if matched.is_empty() {
            bail!("没有匹配的用例：{selector}（用 --list 查看可用用例）");
        }
        for case in matched {
            if !selected.iter().any(|current| current.name == case.name) {
                selected.push(case);
            }
        }
    }

    selected.sort_by_key(|case| case.name);
    Ok(selected)
}

/// 用例名等于选择器，或以 `<选择器>.` 开头。
fn matches_prefix(name: &str, selector: &str) -> bool {
    name == selector || name.strip_prefix(selector).is_some_and(|rest| rest.starts_with('.'))
}

/// 隐藏的替身子命令：只有第二个参数正好是 `mock-mcp` 时才进入，因此主解析器的 `--help` 与
/// `CASE` 选择都不受影响。
fn mock_command() -> Result<Option<Command>> {
    let mut argv = std::env::args_os();
    let _program = argv.next();
    if argv.next().as_deref() != Some(OsStr::new(MOCK_MCP)) {
        return Ok(None);
    }
    let rest: Vec<OsString> = std::env::args_os().skip(2).collect();
    let args = MockMcpArgs::try_parse_from(std::iter::once(OsString::from(MOCK_MCP)).chain(rest))
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let transport = MockTransport::parse(&args.transport)
        .with_context(|| format!("--transport 只支持 stdio / http，实际：{}", args.transport))?;
    if transport == MockTransport::Http && args.port.is_none() {
        bail!("--transport http 必须给 --port");
    }
    let scenario = Scenario::parse(&args.scenario).with_context(|| {
        format!(
            "未知剧本：{}（可用 normal / naming / schema / results / error / crash / slow / list_changed）",
            args.scenario
        )
    })?;
    Ok(Some(Command::MockMcp(MockConfig {
        transport,
        port: args.port,
        scenario,
        evidence: args.evidence,
        pid_file: args.pid_file,
    })))
}
