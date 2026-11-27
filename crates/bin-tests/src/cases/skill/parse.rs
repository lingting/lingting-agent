//! `skill.parse`（P5-003 / P5-016）：`SKILL.md` 的解析口径逐条钉死。
//!
//! 证据来自进程内调用 `agent_skill::parse` 与 `agent_skill::scan_references`（服务端发现走同一条
//! 路径），不读日志、不读事件文件。四段：
//!
//! - 合法文件：字段、未知字段被忽略、名字与目录名不一致不报错；
//! - 六类非法输入：各自独立文案 + 行号；
//! - 章节切分：代码围栏内的 `#` 不算标题，每个章节的字符数由正文独立复算；
//! - 引用清单：越权路径不出现、符号链接不跟随、超过 200 条时按字典序截断。

use std::path::Path;

use agent_skill::{MAX_REFERENCES, SkillError, SkillFile, SkillSection, parse, scan_references};
use anyhow::{Context, Result, bail, ensure};

use super::{case, symlink};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;
use crate::util::paths::{skill_dir, skill_home};
use crate::util::support::temp_directory;

/// 合法样例的路径形态（相对应用根目录）。
const PATH: &str = "skills/demo/SKILL.md";

/// 合法样例：已知字段全写、额外带三个生态里常见的未知字段。
const VALID: &str = "---\nname: demo\ndescription: 演示技能\nrequires_tools: [file_read, file_search]\n\
                     version: \"1.2\"\nauthor: lingting\nallowed-tools: [bash]\nlicense: MIT\n\
                     metadata: {tags: [report]}\n---\n\n# 步骤\n\n1. 先读模板。\n\n## 细则\n\n2. 再查数据。\n";

/// 解析用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.parse", CaseKind::Shared, parser)]
}

/// 四段断言。
fn parser(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = vec![valid()?, invalid()?, sections()?];
        notes.push(references().await?);
        Ok(notes)
    })
}

/// 一、合法文件：已知字段落地、未知字段忽略、名字与目录名不一致不报错。
fn valid() -> Result<String> {
    let file = parse_text(PATH, VALID)?;
    ensure!(file.metadata.name == "demo", "name 未解析：{:?}", file.metadata);
    ensure!(
        file.metadata.description == "演示技能",
        "description 未解析：{:?}",
        file.metadata
    );
    ensure!(
        file.metadata.requires_tools == ["file_read", "file_search"],
        "requires_tools 未解析：{:?}",
        file.metadata
    );
    ensure!(
        file.metadata.version.as_deref() == Some("1.2") && file.metadata.author.as_deref() == Some("lingting"),
        "version / author 未解析：{:?}",
        file.metadata
    );
    ensure!(
        file.body.starts_with("\n# 步骤"),
        "正文应原样保留 frontmatter 之后的内容（含首部空行）：{:?}",
        file.body
    );

    // 目录名只是存放位置：解析器只认 frontmatter 里的 name（D-P5-004）。
    let elsewhere = parse_text("skills/another-directory/SKILL.md", VALID)?;
    ensure!(
        elsewhere.metadata.name == file.metadata.name && elsewhere.body == file.body,
        "名字与目录名不一致不应影响解析"
    );
    Ok(format!(
        "合法：name=demo、requires_tools={:?}、未知字段（allowed-tools / license / metadata）被忽略、名字与目录名不一致不报错",
        file.metadata.requires_tools
    ))
}

/// 二、六类非法输入：各自独立文案，可定位到行号。
fn invalid() -> Result<String> {
    let cases: Vec<(&str, &str, &str, Option<usize>)> = vec![
        (
            "缺 frontmatter",
            "# 步骤\n\n1. 没有 frontmatter。\n",
            "缺少 frontmatter",
            None,
        ),
        (
            "缺结束分隔符",
            "---\nname: demo\ndescription: 演示技能\n\n# 步骤\n",
            "frontmatter 缺少结束分隔符",
            Some(1),
        ),
        (
            "YAML 语法错",
            "---\nname: demo\ndescription: 演示技能\nrequires_tools: [file_read\n---\n\n# 步骤\n",
            "frontmatter 不是合法的 YAML",
            None,
        ),
        (
            "缺 name",
            "---\ndescription: 演示技能\n---\n\n# 步骤\n\n1. 做事。\n",
            "缺少 name",
            Some(2),
        ),
        (
            "缺 description",
            "---\nname: demo\n---\n\n# 步骤\n\n1. 做事。\n",
            "缺少 description",
            Some(2),
        ),
        (
            "name 不合规",
            "---\nname: Demo Skill\ndescription: 演示技能\n---\n\n# 步骤\n\n1. 做事。\n",
            "name 不合规",
            Some(2),
        ),
    ];
    let mut notes = Vec::new();
    for (label, text, expected, line) in cases {
        let error = parse_text(PATH, text)
            .err()
            .with_context(|| format!("{label} 应被拒绝"))?;
        let message = error.to_string();
        ensure!(
            message.contains(expected),
            "{label} 的文案应含 `{expected}`，实际：{message}"
        );
        ensure!(message.starts_with(PATH), "{label} 的文案应以相对路径开头：{message}");
        // YAML 语法错的行号取自 serde 的位置，只断言「有行号且落在 frontmatter 内」。
        let located = match line {
            Some(line) => message.starts_with(&format!("{PATH}:{line}: ")),
            // 没有精确行号的（缺 frontmatter）也要求文案里没有行号段。
            None => !message.contains(":0: "),
        };
        ensure!(located, "{label} 的行号不符（期望 {line:?}）：{message}");
        ensure!(
            !matches!(&error, SkillError::TooLarge { .. } | SkillError::NotUtf8 { .. }),
            "{label} 不应落到大小 / 编码错误：{message}"
        );
        notes.push(format!("{label}：{message}"));
    }

    // YAML 语法错落在 `InvalidFrontmatter` 且带行号，不退化成一句笼统的「解析失败」。
    let yaml = "---\nname: demo\ndescription: 演示技能\nrequires_tools: [file_read\n---\n\n# 步骤\n";
    match parse_text(PATH, yaml) {
        Err(SkillError::InvalidFrontmatter { line, .. }) => {
            ensure!(line >= 2, "YAML 错误的行号应落在 frontmatter 内：{line}");
        }
        other => bail!("YAML 语法错应落 InvalidFrontmatter：{other:?}"),
    }

    // 大小与编码也是独立文案（无行号）。
    let big = "x".repeat(64);
    let error = parse(PATH, big.as_bytes(), 16)
        .err()
        .context("超过 max_skill_bytes 应被拒绝")?;
    ensure!(error.to_string().contains("文件超出大小上限"), "超限文案不符：{error}");
    let error = parse(PATH, &[0xff, 0xfe, 0x00], 16)
        .err()
        .context("非 UTF-8 应被拒绝")?;
    ensure!(
        error.to_string().contains("不是 UTF-8 文本"),
        "编码错误文案不符：{error}"
    );
    notes.push("大小超限与非 UTF-8：两句话各自独立、不带行号".to_owned());
    Ok(notes.join("｜"))
}

/// 三、章节切分：围栏内的 `#` 不算标题，字符数可由正文独立复算。
fn sections() -> Result<String> {
    let text = "---\nname: demo\ndescription: 演示技能\n---\n\n# 步骤\n\n说明。\n\n```bash\n# 这不是标题\n\
                echo hi\n```\n\n## 细则\n\n~~~\n## 也不是标题\n~~~\n\n## 收尾\n\n结束。\n";
    let file = parse_text(PATH, text)?;
    let expected = recompute(&file.body, first_body_line(text));
    ensure!(
        file.sections.len() == expected.len(),
        "章节数不符：{:?} != {:?}",
        file.sections,
        expected
    );
    for (actual, want) in file.sections.iter().zip(expected.iter()) {
        ensure!(actual == want, "章节表与正文复算结果不符：{actual:?} != {want:?}");
    }
    ensure!(
        file.sections.iter().map(|item| item.title.as_str()).collect::<Vec<_>>() == ["步骤", "细则", "收尾"],
        "围栏内的 `#` 不应生成章节：{:?}",
        file.sections
    );
    // 父节字符数包含子节：`步骤` 覆盖到下一个同级或更高级标题之前。
    let first_line = first_body_line(text);
    let steps = &file.sections[0];
    let steps_text = section_text(&file.body, first_line, steps, &file.sections[1..]);
    ensure!(
        steps_text.chars().count() == steps.chars && steps_text.contains("## 细则"),
        "父节字符数应含子节：{:?}",
        steps
    );
    Ok(format!(
        "章节：{:?}（围栏内的 # 不计；字符数由正文复算）",
        file.sections
            .iter()
            .map(|item| (item.title.as_str(), item.level, item.start_line, item.chars))
            .collect::<Vec<_>>()
    ))
}

/// 四、引用清单：越权路径不出现、符号链接不跟随、超过 200 条按字典序截断。
async fn references() -> Result<String> {
    let mut notes = Vec::new();

    // 普通文件：递归、字典序、路径相对应用根目录。
    let home = skill_home("skill-parse-refs")?;
    let directory = skill_dir(&home, "demo");
    write(&directory.join("references/notes.md"), "笔记")?;
    write(&directory.join("references/nested/deep.md"), "深层")?;
    write(&directory.join("references/z-last.md"), "最后")?;
    let found = scan_references(&directory, "skills/demo")?;
    ensure!(
        found
            == [
                "skills/demo/references/nested/deep.md",
                "skills/demo/references/notes.md",
                "skills/demo/references/z-last.md"
            ],
        "引用清单应按相对路径字典序、以 skills/<dir> 开头：{found:?}"
    );
    ensure!(
        found
            .iter()
            .all(|item| !item.split('/').any(|part| part == ".." || part == ".")),
        "引用清单不得含越权分量：{found:?}"
    );
    notes.push(format!("引用清单：{found:?}"));

    // 符号链接：不跟随（Windows 上创建链接需要权限，缺权限时显式记「未采集」）。
    let outside = temp_directory("skill-parse-outside")?;
    write(&outside.join("secret.md"), "越权材料")?;
    let link = directory.join("references/escape.md");
    match symlink(&outside.join("secret.md"), &link) {
        Ok(()) => {
            let found = scan_references(&directory, "skills/demo")?;
            ensure!(
                !found.iter().any(|item| item.ends_with("escape.md")),
                "符号链接不应进引用清单：{found:?}"
            );
            notes.push("符号链接：指向应用目录外的链接被跳过".to_owned());
        }
        Err(reason) => notes.push(format!("未采集（符号链接）：{reason}")),
    }

    // 200 条上限：先排序再截断，顺序确定。
    let many = skill_home("skill-parse-many")?;
    let directory = skill_dir(&many, "many");
    for index in 0..(MAX_REFERENCES + 5) {
        write(&directory.join(format!("references/item-{index:03}.md")), "x")?;
    }
    let found = scan_references(&directory, "skills/many")?;
    ensure!(
        found.len() == MAX_REFERENCES,
        "引用清单应在上限处截断：{} != {MAX_REFERENCES}",
        found.len()
    );
    let mut sorted = found.clone();
    sorted.sort();
    ensure!(found == sorted, "截断前应先排序");
    ensure!(
        found[0] == "skills/many/references/item-000.md",
        "截断应保留字典序在前的那批：{:?}",
        &found[..2]
    );
    ensure!(
        found[MAX_REFERENCES - 1] == format!("skills/many/references/item-{:03}.md", MAX_REFERENCES - 1),
        "截断结果应是字典序前 {MAX_REFERENCES} 条：{}",
        found[MAX_REFERENCES - 1]
    );
    notes.push(format!("上限：{MAX_REFERENCES} 条，按字典序截断"));
    Ok(notes.join("｜"))
}

/// 解析一份 `SKILL.md` 文本（上限取缺省配置，与装配层的缺省一致）。
fn parse_text(path: &str, text: &str) -> Result<SkillFile, SkillError> {
    parse(
        path,
        text.as_bytes(),
        agent_skill::SkillLimits::default().max_skill_bytes,
    )
}

/// 正文首行在文件里的行号：frontmatter 结束分隔符之后的下一行。
fn first_body_line(text: &str) -> usize {
    let mut seen = 0;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end() == "---" {
            seen += 1;
            if seen == 2 {
                return text[..offset].lines().count() + 1;
            }
        }
    }
    1
}

/// 独立复算章节表：按行扫 ATX 标题并维护围栏状态，章节范围到下一个同级或更高级标题为止。
fn recompute(body: &str, first_line: usize) -> Vec<SkillSection> {
    let mut headings: Vec<(String, u8, usize, usize)> = Vec::new();
    let mut fence: Option<char> = None;
    let mut offset = 0;
    for (index, line) in body.split_inclusive('\n').enumerate() {
        let content = line.trim_end_matches(['\n', '\r']);
        let trimmed = content.trim_start();
        let marker = trimmed.chars().next().filter(|item| *item == '`' || *item == '~');
        let fence_len = marker
            .map(|marker| trimmed.chars().take_while(|item| *item == marker).count())
            .unwrap_or(0);
        match fence {
            Some(open) => {
                if marker == Some(open) && fence_len >= 3 {
                    fence = None;
                }
            }
            None if marker.is_some() && fence_len >= 3 => fence = marker,
            None => {
                let level = trimmed.chars().take_while(|item| *item == '#').count();
                let rest = trimmed.get(level..).unwrap_or_default();
                let is_heading = (1..=6).contains(&level) && (rest.is_empty() || rest.starts_with(' '));
                if is_heading {
                    let title = rest.trim().trim_end_matches('#').trim().to_owned();
                    headings.push((title, level as u8, index, offset));
                }
            }
        }
        offset += line.len();
    }
    headings
        .iter()
        .enumerate()
        .map(|(position, (title, level, line, byte))| {
            let end = headings[position + 1..]
                .iter()
                .find(|later| later.1 <= *level)
                .map_or(body.len(), |later| later.3);
            SkillSection {
                title: title.clone(),
                level: *level,
                start_line: first_line + line,
                chars: body[*byte..end].chars().count(),
            }
        })
        .collect()
}

/// 一个章节的正文（含子节）：与 [`recompute`] 同一套边界。
/// 一个章节的正文（含子节）：与 [`recompute`] 同一套边界；`first_line` 是正文首行的文件行号。
fn section_text<'a>(body: &'a str, first_line: usize, section: &SkillSection, later: &[SkillSection]) -> &'a str {
    let line_of = |file_line: usize| file_line.saturating_sub(first_line) + 1;
    let start = byte_of_line(body, line_of(section.start_line));
    let end = later
        .iter()
        .find(|next| next.level <= section.level)
        .map_or(body.len(), |next| byte_of_line(body, line_of(next.start_line)));
    body.get(start..end).unwrap_or_default()
}

/// 正文里某一行（文件行号）的字节偏移：调用方只在已知行号上使用。
fn byte_of_line(body: &str, line: usize) -> usize {
    let mut offset = 0;
    for (index, item) in body.split_inclusive('\n').enumerate() {
        if index + 1 == line {
            return offset;
        }
        offset += item.len();
    }
    body.len()
}

/// 写入用例自己的文件（父目录一并创建）。
fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("创建目录失败：{}", parent.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("写入文件失败：{}", path.display()))
}
