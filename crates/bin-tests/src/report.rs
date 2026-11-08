//! 测试报告：把结果写到 `target/tests/<运行 ID>/`。
//!
//! `README.md` 是本次运行的汇总与用例索引；用例按领域汇总到 `<领域>.md`，避免零散文件。
//! README 的索引给出行号范围，领域文档顶部也有同样的行号索引，便于按行读取具体结果。

use crate::case::CaseKind;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 单个用例的报告数据。
pub struct CaseReport {
    /// 用例名。
    pub name: &'static str,
    /// 服务端形态。
    pub kind: CaseKind,
    /// 该用例访问的服务基地址。
    pub base_url: String,
    /// 该用例的子进程与日志目录。
    pub log_dir: PathBuf,
    /// 执行耗时。
    pub elapsed: Duration,
    /// 用例返回的结果信息。
    pub notes: Vec<String>,
    /// 失败原因（已格式化为错误链文本）。
    pub error: Option<String>,
}

impl CaseReport {
    /// 用例是否通过。
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

/// 一次运行的报告数据。
pub struct RunReport<'a> {
    /// 运行 ID。
    pub run_id: &'a str,
    /// 开始时间（Unix 毫秒）。
    pub started_at_ms: i64,
    /// 并发度。
    pub concurrency: usize,
    /// 共享服务端地址；未启动时为空。
    pub shared_base_url: Option<&'a str>,
    /// 子进程日志根目录。
    pub log_root: &'a Path,
    /// 全部用例结果。
    pub cases: &'a [CaseReport],
}

/// 用例在领域文档中的位置，供 README 索引使用。
struct Location {
    file: String,
    start: usize,
    lines: usize,
}

/// 用例名到领域文档位置的索引。
type Locations = HashMap<&'static str, Location>;

/// 用例在领域文档中的起始行与行数。
type Span = (usize, usize);

/// 领域文档的渲染结果。
struct DomainDoc {
    text: String,
    locations: Vec<(&'static str, Span)>,
}

/// 写入汇总 README 与按领域汇总的用例详情文档。
pub fn write(report: &RunReport<'_>) -> Result<()> {
    let directory = report_dir(report.run_id)?;
    fs::create_dir_all(&directory).with_context(|| format!("创建测试报告目录失败：{}", directory.display()))?;

    let locations = write_domains(&directory, report.cases)?;
    write_readme(&directory, report, &locations)?;

    println!("测试报告：{}", directory.join("README.md").display());
    Ok(())
}

/// 报告目录：`<target>/tests/<运行 ID>`。
fn report_dir(run_id: &str) -> Result<PathBuf> {
    let current = std::env::current_exe().context("获取测试入口路径失败")?;
    let target = current
        .parent()
        .and_then(Path::parent)
        .context("无法从测试入口路径推导 target 目录")?;
    Ok(target.join("tests").join(run_id))
}

/// 按领域写 `<领域>.md`，并返回每个用例的行号位置。
fn write_domains(directory: &Path, cases: &[CaseReport]) -> Result<Locations> {
    let mut domains: BTreeMap<&'static str, Vec<&CaseReport>> = BTreeMap::new();
    for case in cases {
        domains.entry(domain_of(case.name)).or_default().push(case);
    }

    let mut locations = Locations::new();
    for (domain, cases) in domains {
        let file = format!("{domain}.md");
        let document = render_domain(domain, &cases);
        write_file(directory.join(&file), document.text)?;
        for (name, (start, lines)) in document.locations {
            locations.insert(
                name,
                Location {
                    file: file.clone(),
                    start,
                    lines,
                },
            );
        }
    }
    Ok(locations)
}

/// 渲染一个领域文档：顶部行号索引 + 按用例分节。
fn render_domain(domain: &str, cases: &[&CaseReport]) -> DomainDoc {
    let sections: Vec<Vec<String>> = cases.iter().map(|case| section_lines(case)).collect();
    let lengths: Vec<usize> = sections.iter().map(Vec::len).collect();
    // 顶部固定 6 行 + 每个用例一行索引 + 1 个空行，之后开始第一个用例。
    let header_lines = 6 + cases.len() + 1;
    let mut starts = Vec::with_capacity(sections.len());
    let mut offset = header_lines;
    for length in &lengths {
        starts.push(offset + 1);
        offset += length + 1;
    }

    let mut lines: Vec<String> = vec![
        format!("# {domain}"),
        String::new(),
        "> 用例索引：起始行是该用例在本文件中的第一行，行数是该用例占用的行数。".to_string(),
        String::new(),
        "| 用例 | 类型 | 结果 | 耗时 | 起始行 | 行数 |".to_string(),
        "|---|---|---|---|---|---|".to_string(),
    ];
    for ((case, start), length) in cases.iter().zip(&starts).zip(&lengths) {
        let status = if case.is_success() { "通过" } else { "失败" };
        lines.push(format!(
            "| {} | {} | {status} | {}ms | {start} | {length} |",
            case.name,
            case.kind.label(),
            case.elapsed.as_millis()
        ));
    }
    lines.push(String::new());

    for section in sections {
        lines.extend(section);
        lines.push(String::new());
    }

    let locations = cases
        .iter()
        .zip(starts)
        .zip(lengths)
        .map(|((case, start), length)| (case.name, (start, length)))
        .collect();
    DomainDoc {
        text: lines.join("\n"),
        locations,
    }
}

/// 渲染单个用例的章节。
fn section_lines(case: &CaseReport) -> Vec<String> {
    let status = if case.is_success() { "通过" } else { "失败" };
    let mut lines = vec![
        format!("## {}", case.name),
        String::new(),
        "| 项 | 值 |".to_string(),
        "|---|---|".to_string(),
        format!("| 类型 | {} |", case.kind.label()),
        format!("| 结果 | {status} |"),
        format!("| 耗时 | {}ms |", case.elapsed.as_millis()),
        format!("| 服务地址 | {} |", case.base_url),
        format!("| 子进程与日志目录 | `{}` |", case.log_dir.display()),
        String::new(),
        "### 结果信息".to_string(),
        String::new(),
    ];

    if case.notes.is_empty() {
        lines.push("- （无）".to_string());
    } else {
        for note in &case.notes {
            lines.push(format!("- {note}"));
        }
    }

    if let Some(error) = &case.error {
        lines.push(String::new());
        lines.push("### 失败原因".to_string());
        lines.push(String::new());
        lines.push("```text".to_string());
        if error.is_empty() {
            lines.push(String::new());
        } else {
            lines.extend(error.lines().map(ToString::to_string));
        }
        lines.push("```".to_string());
    }

    lines
}

/// 写汇总 README：运行信息、统计与用例行号索引。
fn write_readme(directory: &Path, report: &RunReport<'_>, locations: &Locations) -> Result<()> {
    let passed = report.cases.iter().filter(|case| case.is_success()).count();
    let failed = report.cases.len() - passed;
    let shared = report.shared_base_url.unwrap_or("未启动");

    let mut text = format!("# 测试报告 {}\n\n", report.run_id);
    text.push_str("| 项 | 值 |\n|---|---|\n");
    text.push_str(&format!("| 开始时间（Unix 毫秒） | {} |\n", report.started_at_ms));
    text.push_str(&format!("| 并发度 | {} |\n", report.concurrency));
    text.push_str(&format!("| 共享服务端 | {shared} |\n"));
    text.push_str(&format!("| 用例总数 | {} |\n", report.cases.len()));
    text.push_str(&format!("| 通过 | {passed} |\n"));
    text.push_str(&format!("| 失败 | {failed} |\n"));
    text.push_str(&format!("| 子进程日志根目录 | `{}` |\n\n", report.log_root.display()));

    text.push_str("## 用例\n\n");
    text.push_str("> 详情列是 `文件:起始行-结束行`，按行打开对应领域文档即可读到该用例的完整结果。\n\n");
    text.push_str("| 用例 | 类型 | 结果 | 耗时 | 详情（行号） |\n|---|---|---|---|---|\n");
    for case in report.cases {
        let status = if case.is_success() { "通过" } else { "失败" };
        text.push_str(&format!(
            "| {} | {} | {status} | {}ms | {} |\n",
            case.name,
            case.kind.label(),
            case.elapsed.as_millis(),
            detail_link(case, locations)
        ));
    }

    write_file(directory.join("README.md"), text)
}

/// 生成 README 的行号链接；找不到位置时给出明确标记。
fn detail_link(case: &CaseReport, locations: &Locations) -> String {
    let Some(location) = locations.get(case.name) else {
        return "（缺失）".to_string();
    };
    let end = location.start + location.lines.saturating_sub(1);
    if location.lines <= 1 {
        return format!("[{}:{}](./{})", location.file, location.start, location.file);
    }
    format!("[{}:{}-{}](./{})", location.file, location.start, end, location.file)
}

/// 用例名到领域名：第一个 `.` 之前的部分。
fn domain_of(name: &'static str) -> &'static str {
    name.split('.').next().unwrap_or(name)
}

/// 写文件并附带路径上下文。
fn write_file(path: PathBuf, text: String) -> Result<()> {
    fs::write(&path, text).with_context(|| format!("写入测试报告失败：{}", path.display()))
}
