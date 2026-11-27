//! Mock 的工具调用脚本（P3-030）：把「这一轮要求模型调用哪些工具」写进用户输入里的关键字。
//!
//! 关键字形如 `tool:{…}`，载荷是一段 JSON：
//!
//! ```text
//! tool:{"calls": [{"name": "calculator", "arguments": {"expression": "1+1"}}],
//!       "fragments": 3, "missingEnd": false, "repeat": false}
//! tool:{"rounds": [{"calls": [{"name": "skill_load", "arguments": {"name": "pdf-report"}}]},
//!                  {"calls": [{"name": "file_read", "arguments": {"path": "…"}}]}]}
//! ```
//!
//! - `calls`：单轮剧本，这一轮要发出的调用，按顺序；`arguments` 可以是任意 JSON（原样序列化成
//!   参数字符串），写成字符串时按原文发出——用来构造「参数不是合法 JSON」的调用；
//! - `rounds`：多轮剧本，第 `n` 次请求发出第 `n` 轮的调用，轮次用完后回普通文本（`skill.e2e` 用）；
//! - `fragments`：参数分几片写出，缺省 1（整段一次写出）；分片用于断言 Rig 的聚合口径；
//! - `missingEnd`：只写参数分片、不写结束帧（也没有 `[DONE]`），Rig 侧应落 `FAILED`；
//! - `repeat`：每一轮都重复第一轮的调用（`tool.rounds` 用），缺省按轮次推进。
//!
//! 换成普通文本回复的时机：剧本的轮次已经用完（单轮剧本就是「历史里已经出现 assistant 的
//! `tool_calls`」），这是「两轮闭环」最稳的判据——不依赖 Mock 记住自己发过什么。
//!
//! 工具清单本身也照常回给用例：`/mock/last-request` 是完整的请求体，`tools` 与 `messages` 都在里面。

use serde_json::{Value, json};

/// 关键字前缀。
const KEYWORD: &str = "tool:";

/// 一次脚本化的工具调用。
pub(crate) struct ScriptedCall {
    /// 调用标识：客户端据此把开始、确认与结束事件配成一张卡片。
    pub(crate) id: String,
    /// 工具名。
    pub(crate) name: String,
    /// 参数文本（JSON 原文）。
    pub(crate) arguments: String,
}

/// 一段工具调用剧本：每一轮一条调用清单。
pub(crate) struct Script {
    /// 按轮次的调用清单：第 `n` 次请求发出 `rounds[n]`，越界则回普通文本。
    rounds: Vec<Vec<ScriptedCall>>,
    /// 参数分几片写出。
    pub(crate) fragments: usize,
    /// 是否省略结束帧。
    pub(crate) missing_end: bool,
    /// 是否每轮都重复第一轮。
    repeat: bool,
}

/// 从请求体里找剧本：取**最近一条**带 `tool:{…}` 关键字的用户消息。
///
/// 第二轮起最后一条消息是工具结果，剧本仍留在历史里的那条用户消息上，因此不能只看最后一条。
pub(crate) fn parse_in(body: &Value) -> Option<Script> {
    body.get("messages")?
        .as_array()?
        .iter()
        .rev()
        .filter_map(|message| message.get("content").and_then(Value::as_str))
        .find_map(Script::parse)
}

impl Script {
    /// 从用户输入里解析剧本；没有关键字或载荷不是合法 JSON 时返回 `None`。
    ///
    /// 载荷解析失败按「没有剧本」处理：Mock 是测试设施，不该因为用例拼错 JSON 就改变
    /// 别的用例的行为——那时用例会在「没有工具事件」上失败，错误就在它自己身上。
    pub(crate) fn parse(content: &str) -> Option<Self> {
        let start = content.find(KEYWORD)? + KEYWORD.len();
        let payload: Value = serde_json::from_str(content[start..].trim()).ok()?;
        // 两种写法：`calls` 是单轮（沿用旧格式与调用标识），`rounds` 是逐轮剧本。
        let rounds = match payload.get("rounds") {
            Some(rounds) => {
                let items = rounds.as_array()?;
                if items.is_empty() {
                    return None;
                }
                items
                    .iter()
                    .enumerate()
                    .map(|(round, item)| round_calls(item, Some(round)))
                    .collect::<Option<Vec<_>>>()?
            }
            None => vec![round_calls(&payload, None)?],
        };
        Some(Self {
            rounds,
            fragments: payload.get("fragments").and_then(Value::as_u64).unwrap_or(1).max(1) as usize,
            missing_end: payload.get("missingEnd").and_then(Value::as_bool).unwrap_or(false),
            repeat: payload.get("repeat").and_then(Value::as_bool).unwrap_or(false),
        })
    }

    /// 这一轮该发出的调用；`None` 表示该回普通文本。
    ///
    /// 轮次 = 「剧本所在的用户消息之后出现了几条 assistant 的 `tool_calls`」：多轮会话里每条用户
    /// 消息带自己的剧本，因此不能只看整段历史里有没有工具调用（那会让第二轮的剧本永不生效）。
    /// `repeat` 时每轮都发第一轮，用于构造「反复调用同一工具」的场景。
    pub(crate) fn round(&self, request: &Value) -> Option<&[ScriptedCall]> {
        let index = if self.repeat { 0 } else { done_rounds(request) };
        self.rounds.get(index).map(Vec::as_slice)
    }
}

/// 剧本所在的那条用户消息之后已经完成的工具轮次。
fn done_rounds(request: &Value) -> usize {
    let Some(messages) = request.get("messages").and_then(Value::as_array) else {
        return 0;
    };
    let Some(turn) = messages
        .iter()
        .rposition(|message| message["content"].as_str().is_some_and(|text| text.contains(KEYWORD)))
    else {
        return 0;
    };
    messages[turn + 1..]
        .iter()
        .filter(|message| {
            message
                .get("tool_calls")
                .and_then(Value::as_array)
                .is_some_and(|calls| !calls.is_empty())
        })
        .count()
}

/// 一轮里的调用清单；`round` 为 `Some` 时调用标识带上轮次（同一 Run 内跨轮不重名）。
fn round_calls(round: &Value, round_index: Option<usize>) -> Option<Vec<ScriptedCall>> {
    let calls: Vec<ScriptedCall> = round
        .get("calls")?
        .as_array()?
        .iter()
        .enumerate()
        .map(|(index, call)| {
            let name = call.get("name")?.as_str()?.to_owned();
            Some(ScriptedCall {
                id: match round_index {
                    Some(round) => format!("call_mock_{round}_{index}"),
                    None => format!("call_mock_{index}"),
                },
                name,
                arguments: arguments_text(call.get("arguments")),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    (!calls.is_empty()).then_some(calls)
}

/// 参数文本：对象 / 数组 / 数字按紧凑 JSON 序列化，字符串按原文发出（用于构造非法参数）。
fn arguments_text(arguments: Option<&Value>) -> String {
    match arguments {
        Some(Value::String(raw)) => raw.clone(),
        Some(other) => other.to_string(),
        None => json!({}).to_string(),
    }
}

/// 最后一条消息的文本内容：脚本就写在用户输入里。
pub(crate) fn last_content(body: &Value) -> String {
    body.get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| messages.last())
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// 参数文本切成 `fragments` 片：按字符切，不拆开多字节字符。
pub(crate) fn fragments_of(text: &str, fragments: usize) -> Vec<String> {
    let total = text.chars().count();
    if fragments <= 1 || total == 0 {
        return vec![text.to_owned()];
    }
    let size = total.div_ceil(fragments);
    let mut pieces = Vec::new();
    let mut current = String::new();
    for (index, character) in text.chars().enumerate() {
        current.push(character);
        if current.chars().count() == size && index + 1 < total {
            pieces.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}
