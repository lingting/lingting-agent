//! 章节切分：按行扫描 ATX 标题（`#` ~ `######`），代码围栏内的 `#` 不算标题（D-P5-017）。
//!
//! 不引入 Markdown crate：本阶段只需要「标题 + 围栏」两种语法，完整 CommonMark AST 的收益
//! （更完整的语法支持）与代价（额外依赖、编译时间、字符偏移映射回行号）不匹配。
//!
//! **不识别** Setext 标题（`===` / `---` 下划线）：`SKILL.md` 的规范只允许 ATX 标题。
//!
//! 章节口径（可由正文独立复算）：
//!
//! - `start_line` 是标题行在**整个文件**里的行号（frontmatter 的行也算在内）；
//! - `chars` 是章节文本的字符数，**含子节**：章节文本从标题行首开始，到下一个同级或更高级标题
//!   的行首之前结束（没有则到正文末尾），末尾换行计入；
//! - `title` 是 `#` 串之后的内容去掉首尾空白，再去掉末尾连续的 `#`（CommonMark 的收尾序列）。

/// 一个 ATX 标题描述的章节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSection {
    /// 标题文本（不含 `#`）。
    pub title: String,
    /// 标题层级（`#` 的个数，1 ~ 6）。
    pub level: u8,
    /// 标题行在文件里的行号（1 起，含 frontmatter 的行）。
    pub start_line: usize,
    /// 章节字符数（含子节与末尾换行）。
    pub chars: usize,
}

/// 扫描到的一个标题；`byte` 是标题行首在正文里的字节偏移。
struct Heading {
    title: String,
    level: u8,
    line: usize,
    byte: usize,
}

/// 把正文切成章节；`first_line` 是正文首行在文件里的行号。
pub(crate) fn split(body: &str, first_line: usize) -> Vec<SkillSection> {
    let headings = headings(body);
    let mut sections = Vec::with_capacity(headings.len());
    for (index, heading) in headings.iter().enumerate() {
        let end = headings[index + 1..]
            .iter()
            .find(|later| later.level <= heading.level)
            .map_or(body.len(), |later| later.byte);
        // `byte <= end` 且两者都在字符边界上：标题行首与下个标题行首都是行边界。
        let chars = body.get(heading.byte..end).map_or(0, |text| text.chars().count());
        sections.push(SkillSection {
            title: heading.title.clone(),
            level: heading.level,
            start_line: first_line + heading.line,
            chars,
        });
    }
    sections
}

/// 按行扫描标题：围栏的开合状态跨行维护，围栏内的 `#` 一律不认。
fn headings(body: &str) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut fence: Option<char> = None;
    let mut offset = 0;
    for (index, line) in body.split_inclusive('\n').enumerate() {
        let content = line.trim_end_matches(['\n', '\r']);
        match fence {
            Some(marker) => {
                if fence_marker(content) == Some(marker) {
                    fence = None;
                }
            }
            None => match fence_marker(content) {
                Some(marker) => fence = Some(marker),
                None => {
                    if let Some((level, title)) = atx_heading(content) {
                        headings.push(Heading {
                            title,
                            level,
                            line: index,
                            byte: offset,
                        });
                    }
                }
            },
        }
        offset += line.len();
    }
    headings
}

/// 围栏标记（` ``` ` 或 `~~~`，至少三个）：返回标记字符。
fn fence_marker(content: &str) -> Option<char> {
    let trimmed = content.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|item| *item == marker).count();
    (count >= 3).then_some(marker)
}

/// ATX 标题：`#` ~ `######` 后必须跟空格（或行尾），返回（层级, 标题文本）。
fn atx_heading(content: &str) -> Option<(u8, String)> {
    let trimmed = content.trim_start();
    let level = trimmed.chars().take_while(|item| *item == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = trimmed.get(level..)?;
    let rest = match rest.strip_prefix(' ') {
        Some(rest) => rest,
        // `##` 后面什么都没有也算标题；`##x` 不是标题（与 CommonMark 一致）。
        None if rest.is_empty() => rest,
        None => return None,
    };
    let title = rest.trim().trim_end_matches('#').trim_end();
    Some((level as u8, title.to_owned()))
}
