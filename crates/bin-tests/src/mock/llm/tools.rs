//! Mock 的工具调用脚本（P3-030）：把「这一轮要求模型调用哪些工具」写进用户输入里的关键字。
//!
//! 关键字形如 `tool:{…}`，载荷是一段 JSON：
//!
//! ```text
//! tool:{"calls": [{"name": "calculator", "arguments": {"expression": "1+1"}}],
//!       "fragments": 3, "missingEnd": false, "repeat": false}
//! ```
//!
//! - `calls`：本轮要发出的调用，按顺序，`arguments` 可以是任意 JSON（原样序列化成参数字符串），
//!   写成字符串时按原文发出——用来构造「参数不是合法 JSON」的调用；
//! - `fragments`：参数分几片写出，缺省 1（整段一次写出）；分片用于断言 Rig 的聚合口径；
//! - `missingEnd`：只写参数分片、不写结束帧（也没有 `[DONE]`），Rig 侧应落 `FAILED`；
//! - `repeat`：每一轮都重复这套调用（`tool.rounds` 用），缺省只在历史里还没有工具调用时发出。
//!
//! 换成普通文本回复的时机：历史里已经出现 assistant 的 `tool_calls`（也就是第一轮工具已经执行过），
//! 这是「两轮闭环」最稳的判据——不依赖 Mock 记住自己发过什么。
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

/// 一段工具调用脚本。
pub(crate) struct Script {
    /// 本轮发出的调用，按顺序。
    pub(crate) calls: Vec<ScriptedCall>,
    /// 参数分几片写出。
    pub(crate) fragments: usize,
    /// 是否省略结束帧。
    pub(crate) missing_end: bool,
    /// 是否每轮都重复。
    pub(crate) repeat: bool,
}

impl Script {
    /// 从用户输入里解析脚本；没有关键字或载荷不是合法 JSON 时返回 `None`。
    ///
    /// 载荷解析失败按「没有脚本」处理：Mock 是测试设施，不该因为用例拼错 JSON 就改变
    /// 别的用例的行为——那时用例会在「没有工具事件」上失败，错误就在它自己身上。
    pub(crate) fn parse(content: &str) -> Option<Self> {
        let start = content.find(KEYWORD)? + KEYWORD.len();
        let payload: Value = serde_json::from_str(content[start..].trim()).ok()?;
        let calls: Vec<ScriptedCall> = payload
            .get("calls")?
            .as_array()?
            .iter()
            .enumerate()
            .map(|(index, call)| {
                let name = call.get("name")?.as_str()?.to_owned();
                Some(ScriptedCall {
                    id: format!("call_mock_{index}"),
                    name,
                    arguments: arguments_text(call.get("arguments")),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        if calls.is_empty() {
            return None;
        }
        Some(Self {
            calls,
            fragments: payload.get("fragments").and_then(Value::as_u64).unwrap_or(1).max(1) as usize,
            missing_end: payload.get("missingEnd").and_then(Value::as_bool).unwrap_or(false),
            repeat: payload.get("repeat").and_then(Value::as_bool).unwrap_or(false),
        })
    }

    /// 这一轮是否该发工具调用。
    ///
    /// 判据是「本轮脚本所在的用户消息之后还没有 assistant 的 `tool_calls`」：多轮会话里每一轮
    /// 都带自己的脚本，因此不能只看整段历史里有没有工具调用（那会让第二轮的脚本永不生效）。
    pub(crate) fn applies_to(&self, request: &Value) -> bool {
        if self.repeat {
            return true;
        }
        let Some(messages) = request.get("messages").and_then(Value::as_array) else {
            return true;
        };
        let Some(turn) = messages
            .iter()
            .rposition(|message| message["content"].as_str().is_some_and(|text| text.contains(KEYWORD)))
        else {
            return true;
        };
        !messages[turn + 1..].iter().any(|message| {
            message
                .get("tool_calls")
                .and_then(Value::as_array)
                .is_some_and(|calls| !calls.is_empty())
        })
    }
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
