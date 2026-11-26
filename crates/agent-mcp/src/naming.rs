//! 工具命名映射：`mcp__<server_id>__<tool>`（D-P4-005）。
//!
//! 规则只有一个实现点（README §4.4）：
//!
//! 1. server 段直接用配置的 `id`（装载期已校验 `^[a-z][a-z0-9_]{0,31}$`）；
//! 2. tool 段小写化，`[^a-z0-9_]` 逐字符替换成 `_`，连续 `_` 折叠，去掉首尾 `_`，空串退化为 `tool`；
//! 3. 结果不是 `[a-z]` 开头时补前缀 `t_`；
//! 4. 总长度超过 64 时保留 `mcp__<server>__` + tool 段前若干字符 + `_` + 8 位 `blake3` 短哈希；
//! 5. 与已注册名字冲突时追加 `_<8 位哈希>`（确定性来自「server 按 id 升序、工具按原名升序」）。
//!
//! 冲突消解不在本模块做顺序决策：它只提供「给定原名时怎么加后缀」，固定顺序由
//! [`crate::discover`] 的排序保证。所有输出都满足 `agent_tool::validate_name` 的规则。

/// 模型可见工具名的固定前缀。
pub const NAME_PREFIX: &str = "mcp__";
/// 工具名长度上限（与 [`validate_name`] 的口径一致）。
pub const MAX_NAME_LEN: usize = 64;
/// 短哈希长度。
const HASH_LEN: usize = 8;

/// 由 server `id` 与 server 报告的工具原名算出模型可见名（不含冲突消解）。
pub fn visible_name(server_id: &str, original: &str) -> String {
    let prefix = format!("{NAME_PREFIX}{server_id}__");
    let tool = sanitize(original);
    if prefix.len() + tool.len() <= MAX_NAME_LEN {
        return format!("{prefix}{tool}");
    }
    // 超长：保住前缀（server 段是定位 server 的唯一线索），tool 段截断后加短哈希。
    let suffix_len = 1 + HASH_LEN;
    let budget = MAX_NAME_LEN.saturating_sub(prefix.len() + suffix_len);
    format!("{prefix}{}_{}", take_chars(&tool, budget), short_hash(original))
}

/// 与已占用的名字冲突时追加 `_<8 位哈希>`（哈希取原名，因此可复算）。
///
/// 追加后若超过长度上限，先按字符截短基础名——哈希后缀不能省，否则「同名 server 的两个工具」
/// 还是同一个名字。
pub fn disambiguate(name: &str, original: &str) -> String {
    let suffix = format!("_{}", short_hash(original));
    if name.len() + suffix.len() <= MAX_NAME_LEN {
        return format!("{name}{suffix}");
    }
    format!("{}{suffix}", take_chars(name, MAX_NAME_LEN - suffix.len()))
}

/// tool 段的规范化：规则 2 与规则 3。
fn sanitize(original: &str) -> String {
    let mut cleaned = String::with_capacity(original.len());
    for character in original.chars().flat_map(char::to_lowercase) {
        let mapped = if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_' {
            character
        } else {
            '_'
        };
        // 连续 `_` 折叠：逐个比较前一个写入的字符即可。
        if mapped == '_' && cleaned.ends_with('_') {
            continue;
        }
        cleaned.push(mapped);
    }
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        return "tool".to_owned();
    }
    if trimmed.starts_with(|character: char| character.is_ascii_lowercase()) {
        trimmed.to_owned()
    } else {
        format!("t_{trimmed}")
    }
}

/// 取前 `budget` 个字符（按字符边界截断，不切坏 UTF-8）。
fn take_chars(text: &str, budget: usize) -> String {
    text.chars().take(budget).collect()
}

/// 原名对应的 8 位 `blake3` 短哈希（十六进制，只在 `[0-9a-f]` 里取值）。
fn short_hash(original: &str) -> String {
    let hash = blake3::hash(original.as_bytes()).to_hex().to_string();
    hash[..HASH_LEN].to_owned()
}
