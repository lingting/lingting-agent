//! 命令行参数：用例选择、列表与并发度。

use crate::case::Case;
use crate::cases;
use crate::runner::Config;
use anyhow::{Result, bail};
use clap::Parser;

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

/// 解析结果：执行选中的用例，或只列出全部用例。
pub enum Command {
    /// 执行选中的用例。
    Run(Config),
    /// 列出全部用例。
    List(Vec<Case>),
}

/// 解析命令行；用法错误以 `Err` 返回，由调用方按参数错误退出。
pub fn parse() -> Result<Command> {
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
