//! MCP 工具的本地文件操作声明：按「工具原名 / `*` 模式」解析（D-P4-006）。
//!
//! MCP 工具的行为在 server 里，本进程无从自述，`file_access` 因此由配置声明。键是 **server
//! 报告的工具原名**（不是模型可见名），也可以是含 `*` 的模式：
//!
//! - `*` 可出现在任意位置、匹配任意长度（含空串），连续 `*` 等价于一个 `*`；
//! - 不支持正则、`?`、`[...]`，这些字符按字面匹配；
//! - 优先级固定：精确名 > 命中的最具体模式（非通配字符数最多，同分取字典序最小）> 缺省 `None`。
//!
//! 匹配器是自研的两指针贪心（不引 glob crate）：规则越少，「模型看到的行为」就越可论证。
//! 这是**声明**，不是校验：系统只用它推导并发语义并写审计，不验证 server 实际动了什么（README §七 风险 4）。

use agent_tool::ToolFileAccess;

/// 一个 server 的文件操作声明集合。
///
/// 构造期就把模式按优先级排好，解析时只做线性扫描，不依赖注册表与运行时状态。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileAccessRules {
    /// 不含 `*` 的键：命中即最高优先级。
    exact: std::collections::BTreeMap<String, ToolFileAccess>,
    /// 含 `*` 的键：已按「非通配字符数降序 + 字典序升序」排序，取第一个命中的。
    patterns: Vec<Pattern>,
}

/// 一个通配模式与它声明的文件操作。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pattern {
    /// 折叠连续 `*` 之后的模式文本。
    text: String,
    /// 非通配字符数：越具体越优先。
    literal_chars: usize,
    /// 声明值。
    access: ToolFileAccess,
}

impl FileAccessRules {
    /// 由配置声明构造：键为工具原名或 `*` 模式，值为声明值。
    ///
    /// 重复键由调用方（`BTreeMap`）消除；空键在装载期就是硬错误，这里按「永不命中」丢弃。
    pub fn new<I>(entries: I) -> Self
    where
        I: IntoIterator<Item = (String, ToolFileAccess)>,
    {
        let mut exact = std::collections::BTreeMap::new();
        let mut patterns = Vec::new();
        for (key, access) in entries {
            let text = collapse_stars(key.trim());
            if text.is_empty() {
                continue;
            }
            if text.contains('*') {
                patterns.push(Pattern {
                    literal_chars: text.chars().filter(|item| *item != '*').count(),
                    text,
                    access,
                });
            } else {
                exact.insert(text, access);
            }
        }
        // 优先级排序：更具体的模式在前；同分按字典序，保证同一份配置在任何进程里给出同一结果。
        patterns.sort_by(|left, right| {
            right
                .literal_chars
                .cmp(&left.literal_chars)
                .then_with(|| left.text.cmp(&right.text))
        });
        Self { exact, patterns }
    }

    /// 解析一个工具原名的文件操作声明：精确名 > 最具体模式 > [`ToolFileAccess::None`]。
    pub fn access_of(&self, original_name: &str) -> ToolFileAccess {
        if let Some(access) = self.exact.get(original_name) {
            return *access;
        }
        self.patterns
            .iter()
            .find(|pattern| matches(&pattern.text, original_name))
            .map_or(ToolFileAccess::None, |pattern| pattern.access)
    }

    /// 是否没有任何声明（配置里没写 `file_access`）。
    pub fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.patterns.is_empty()
    }
}

/// 折叠连续 `*`：`a**b` 与 `a*b` 是同一个模式。
fn collapse_stars(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    for character in text.chars() {
        if character == '*' && collapsed.ends_with('*') {
            continue;
        }
        collapsed.push(character);
    }
    collapsed
}

/// 通配匹配：`*` 匹配任意长度（含空串），其余字符按字面比较（区分大小写）。
///
/// 两指针 + 单次回溯点：只记住「最后一个 `*`」，最坏 O(文本长度 × 模式长度)，无递归、无灾难性回溯。
fn matches(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut p, mut t) = (0, 0);
    // 最近一个 `*` 之后的模式位置与它对应的文本位置（回溯点）。
    let mut backtrack: Option<(usize, usize)> = None;

    while t < text.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            backtrack = Some((p + 1, t));
            p += 1;
        } else if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some((pattern_next, text_mark)) = backtrack {
            // 让上一个 `*` 多吃一个字符，模式回到 `*` 之后重新比较。
            p = pattern_next;
            t = text_mark + 1;
            backtrack = Some((pattern_next, text_mark + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|byte| *byte == b'*')
}
