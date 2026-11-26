//! MCP 测试替身的剧本：工具清单 + 每个工具的调用行为（P4-017）。
//!
//! 剧本只描述「server 会说什么」，不含任何断言口径：用例从接口响应、客户端事件与 `metrics`
//! 快照取证。工具名故意覆盖命名规则（非法字符 / 大写 / 空名 / 超长 / 撞名）与结果映射的四种
//! 形态，让同一份替身服务多个用例。

use serde_json::{Value, json};

/// 一个剧本里的工具：名字、说明、`inputSchema` 原文与调用行为。
pub(crate) struct ToolDef {
    /// server 报告的工具原名（`tools/list` 与 `tools/call` 都用它）。
    pub(crate) name: &'static str,
    /// 工具说明。
    pub(crate) description: &'static str,
    /// `inputSchema` 的 JSON 原文；调用方解析成对象后原样返回。
    pub(crate) schema: &'static str,
    /// `annotations.readOnlyHint`：客户端据此推导风险等级。
    pub(crate) read_only: bool,
    /// 被调用时的行为。
    pub(crate) behavior: Behavior,
}

/// 工具的调用行为。
#[derive(Clone, Copy)]
pub(crate) enum Behavior {
    /// 返回固定文本。
    Text(&'static str),
    /// 延迟一小段时间后返回固定文本：并发断言用它把「并行」与「串行」区分开。
    Delayed(&'static str, u64),
    /// 回显参数字符串（`ECHO：<参数 JSON>`）。
    Echo,
    /// 文本段 + 图片段。
    TextImage,
    /// 只有 `structuredContent`，没有内容段。
    Structured,
    /// 内容段与结构化内容都为空。
    Empty,
    /// `isError = true` 的工具级错误，带可读文本。
    ToolError(&'static str),
    /// 协议级错误（JSON-RPC error），客户端只能拿到固定文案。
    ProtocolError,
    /// 记录一次调用后直接退出进程（模拟崩溃）。
    Crash,
    /// 睡眠给定时长后正常返回（模拟慢响应）。
    Sleep(u64),
}

/// 剧本：工具清单随「第几次 `tools/list`」变化（`list_changed` 用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scenario {
    /// 常规工具：调用闭环、权限、确认与审计用例的默认剧本。
    Normal,
    /// 命名规则：非法字符、大写、空名（全为符号）、数字开头、超长、撞名。
    Naming,
    /// 参数 schema 原样透传：含 `$schema` / 自定义键 / 嵌套 `anyOf`。
    Schema,
    /// 结果映射：纯文本、文本 + 图片、仅结构化、空内容。
    Results,
    /// 失败路径：工具级错误与协议级错误。
    Error,
    /// 崩溃：被调用后退出进程。
    Crash,
    /// 慢响应：被调用后长时间不返回。
    Slow,
    /// 工具清单变化：第一次列清单后新增一个工具并发 `notifications/tools/list_changed`。
    ListChanged,
}

impl Scenario {
    /// 剧本名解析：`--scenario` 的取值。
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "normal" | "ok" => Some(Self::Normal),
            "naming" => Some(Self::Naming),
            "schema" => Some(Self::Schema),
            "results" => Some(Self::Results),
            "error" => Some(Self::Error),
            "crash" => Some(Self::Crash),
            "slow" => Some(Self::Slow),
            "list_changed" | "list-changed" => Some(Self::ListChanged),
            _ => None,
        }
    }

    /// 剧本名：证据文件与日志里用它。
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Naming => "naming",
            Self::Schema => "schema",
            Self::Results => "results",
            Self::Error => "error",
            Self::Crash => "crash",
            Self::Slow => "slow",
            Self::ListChanged => "list_changed",
        }
    }

    /// 是否在第一次列清单之后追加工具并发通知（L-6）。
    pub(crate) fn announces_list_changed(self) -> bool {
        self == Self::ListChanged
    }

    /// 当前的完整工具清单：`list_round` 是第几次 `tools/list`（从 0 开始）。
    pub(crate) fn tools(self, list_round: u64) -> Vec<ToolDef> {
        match self {
            Self::Normal => vec![
                tool(
                    "read_file",
                    "读取一个文件的全文",
                    FILE_SCHEMA,
                    true,
                    Behavior::Text("read_file 的结果"),
                ),
                tool(
                    "write_file",
                    "写入一个文件",
                    FILE_SCHEMA,
                    false,
                    Behavior::Delayed("write_file 的结果", WRITE_DELAY_MS),
                ),
                tool(
                    "write_log",
                    "追加一行日志",
                    FILE_SCHEMA,
                    false,
                    Behavior::Delayed("write_log 的结果", WRITE_DELAY_MS),
                ),
                tool(
                    "pre_commit_write",
                    "提交前写一次索引",
                    FILE_SCHEMA,
                    false,
                    Behavior::Delayed("pre_commit_write 的结果", WRITE_DELAY_MS),
                ),
                tool(
                    "delete_all",
                    "删除全部文件",
                    FILE_SCHEMA,
                    false,
                    Behavior::Delayed("delete_all 的结果", WRITE_DELAY_MS),
                ),
                tool("echo", "回显参数", ECHO_SCHEMA, true, Behavior::Echo),
            ],
            Self::Naming => vec![
                tool("Read-File!", "非法字符名", ECHO_SCHEMA, true, Behavior::Echo),
                tool("read_file", "与上一个撞名", ECHO_SCHEMA, true, Behavior::Echo),
                tool("UPPER_CASE", "大写名", ECHO_SCHEMA, true, Behavior::Echo),
                tool("***", "全是符号（清洗后为空）", ECHO_SCHEMA, true, Behavior::Echo),
                tool("9lives", "数字开头", ECHO_SCHEMA, true, Behavior::Echo),
                tool(LONG_NAME, "超长名", ECHO_SCHEMA, true, Behavior::Echo),
            ],
            Self::Schema => vec![tool(
                "weird_schema",
                "参数 schema 含方言键与嵌套结构",
                WEIRD_SCHEMA,
                true,
                Behavior::Text("weird_schema 的结果"),
            )],
            Self::Results => vec![
                tool(
                    "text_only",
                    "只返回文本",
                    ECHO_SCHEMA,
                    true,
                    Behavior::Text("纯文本结果"),
                ),
                tool("text_image", "文本 + 图片段", ECHO_SCHEMA, true, Behavior::TextImage),
                tool(
                    "structured_only",
                    "只有结构化内容",
                    ECHO_SCHEMA,
                    true,
                    Behavior::Structured,
                ),
                tool("empty", "空内容", ECHO_SCHEMA, true, Behavior::Empty),
            ],
            Self::Error => vec![
                tool(
                    "fail_tool",
                    "工具级错误",
                    ECHO_SCHEMA,
                    true,
                    Behavior::ToolError("磁盘只读，写入被拒绝"),
                ),
                tool(
                    "protocol_error",
                    "协议级错误",
                    ECHO_SCHEMA,
                    true,
                    Behavior::ProtocolError,
                ),
                tool(
                    "ok_tool",
                    "正常工具",
                    ECHO_SCHEMA,
                    true,
                    Behavior::Text("ok_tool 的结果"),
                ),
            ],
            Self::Crash => vec![
                tool("crash", "被调用后退出进程", ECHO_SCHEMA, true, Behavior::Crash),
                tool("echo", "回显参数", ECHO_SCHEMA, true, Behavior::Echo),
            ],
            Self::Slow => vec![
                tool("slow", "长时间不返回", ECHO_SCHEMA, true, Behavior::Sleep(SLOW_MS)),
                tool("echo", "回显参数", ECHO_SCHEMA, true, Behavior::Echo),
            ],
            Self::ListChanged => {
                let mut tools = vec![tool("echo", "回显参数", ECHO_SCHEMA, true, Behavior::Echo)];
                // 第一次列清单之后才出现：差集刷新（D-P4-010）据此加注册。
                if list_round >= 1 {
                    tools.push(tool(
                        "late_tool",
                        "后加的工具",
                        ECHO_SCHEMA,
                        true,
                        Behavior::Text("late_tool 的结果"),
                    ));
                }
                tools
            }
        }
    }
}

/// 构造一条工具定义（简短化剧本表）。
fn tool(
    name: &'static str,
    description: &'static str,
    schema: &'static str,
    read_only: bool,
    behavior: Behavior,
) -> ToolDef {
    ToolDef {
        name,
        description,
        schema,
        read_only,
        behavior,
    }
}

/// 慢响应的等待时长：远大于用例配置的 `[tools] timeout_ms`。
const SLOW_MS: u64 = 30_000;
/// 写声明工具的响应延迟：串行执行时两次调用之间必然出现不重叠的区间。
pub(crate) const WRITE_DELAY_MS: u64 = 120;

/// 超长工具名的样本：加上 `mcp__<server>__` 前缀后必然超过 64 字符。
const LONG_NAME: &str = "long_tool_name_that_exceeds_the_sixty_four_character_limit_by_far";

/// 常规参数 schema：一个必填字符串字段。
const FILE_SCHEMA: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"文件路径"}},"required":["path"],"additionalProperties":false}"#;

/// 回显工具的 schema：一个可选字符串字段。
const ECHO_SCHEMA: &str = r#"{"type":"object","properties":{"text":{"type":"string"}}}"#;

/// 方言齐活的 schema：`$schema`、自定义键、嵌套 `anyOf`、数组与嵌套对象。
pub(crate) const WEIRD_SCHEMA: &str = r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","title":"怪 schema","x-custom":{"keep":true},"type":"object","properties":{"mode":{"anyOf":[{"type":"string"},{"type":"integer"}]},"filters":{"type":"array","items":{"type":"object","properties":{"field":{"type":"string"}},"required":["field"]}}},"required":["mode"],"additionalProperties":false}"#;

/// 结构化结果的固定 JSON（`structured_only` 用）。
pub(crate) fn structured_content() -> Value {
    json!({"rows": 2, "note": "结构化内容"})
}

/// 图片段的固定 data（1×1 PNG 的 base64，只为占位，不参与断言加密内容）。
pub(crate) const IMAGE_DATA: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
/// 图片段的 MIME。
pub(crate) const IMAGE_MIME: &str = "image/png";
/// 文本 + 图片剧本里文本段的固定内容。
pub(crate) const IMAGE_TEXT: &str = "前半段文本";
