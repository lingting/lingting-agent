//! `calculator`：安全的四则运算工具（P3-021，D-P3-015）。
//!
//! 求值器是自研递归下降，不引表达式求值库：文法越小，「模型能算出什么」就越可论证。
//!
//! ```text
//! expr   := term (('+' | '-') term)*
//! term   := factor (('*' | '/' | '%') factor)*
//! factor := ['-' | '+'] (number | '(' expr ')')
//! number := digits ['.' digits] | '.' digits
//! ```
//!
//! 只接受 ASCII 数字、`.`、运算符、括号与空白；不做函数、幂、变量与常量标识符
//! （这些一律是「含不支持的字符」或「语法错误」，不会静默当成别的意思）。

use std::sync::LazyLock;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::context::ToolContext;
use crate::definition::{ToolFileAccess, ToolRisk};
use crate::error::ToolError;
use crate::output::ToolOutput;
use crate::schema::{self, ToolSchema};
use crate::tool::Tool;

/// 表达式长度上限（字符）：防止病态长串（D-P3-015）。
const MAX_EXPRESSION_CHARS: usize = 512;
/// 括号嵌套深度上限：防止栈过深（D-P3-015）。
const MAX_DEPTH: usize = 32;
/// 结果保留的最大小数位数。
const MAX_SCALE: usize = 10;
/// 整数形态输出的绝对值上限：超过它就按小数格式输出，避免 `as i64` 饱和后再格式化。
const INTEGER_LIMIT: f64 = 1e15;

/// `calculator` 的参数。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalculatorArgs {
    /// 要计算的算术表达式，例如 `(2 + 3) * 4`。
    pub expression: String,
}

/// 参数 schema：由 [`CalculatorArgs`] 派生一次，进程内只算一遍（D-P3-002）。
static SCHEMA: LazyLock<ToolSchema> = LazyLock::new(schema::of::<CalculatorArgs>);

/// 四则运算工具。
#[derive(Debug, Default, Clone, Copy)]
pub struct Calculator;

#[async_trait::async_trait]
impl Tool for Calculator {
    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        "计算一个算术表达式的数值结果。只支持数字、四则运算（+ - * /）、取模（%）与括号，不支持函数与变量。"
    }

    fn schema(&self) -> &'static ToolSchema {
        &SCHEMA
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn file_access(&self) -> ToolFileAccess {
        // 纯计算，不操作本地文件（D-P3-021）。
        ToolFileAccess::None
    }

    async fn call(&self, _ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        let args: CalculatorArgs = schema::parse(args)?;
        let value = evaluate(&args.expression)?;
        Ok(ToolOutput::ok(format_number(value)))
    }
}

/// 求值：长度、字符集、语法与数值范围都在这里收口。
fn evaluate(expression: &str) -> Result<f64, ToolError> {
    let length = expression.chars().count();
    if length == 0 {
        return Err(ToolError::InvalidArguments("expression 不能为空".to_owned()));
    }
    if length > MAX_EXPRESSION_CHARS {
        return Err(ToolError::InvalidArguments(format!(
            "expression 长度不能超过 {MAX_EXPRESSION_CHARS} 字符"
        )));
    }
    // 字符集先收口：中文运算符、字母（函数名 / 变量）与 emoji 都在这里被拒，
    // 不给「看起来像表达式、其实按别的意思算」留空间。
    if !expression.chars().all(is_supported) {
        return Err(ToolError::InvalidArguments(
            "expression 只支持数字、. 、+ - * / % 与括号".to_owned(),
        ));
    }
    let value = Parser::new(expression).parse()?;
    if !value.is_finite() {
        return Err(ToolError::Failed("结果超出可表示范围".to_owned()));
    }
    Ok(value)
}

/// 递归下降解析器：只前进不回溯，每一步失败都是固定文案。
struct Parser {
    chars: Vec<char>,
    at: usize,
    depth: usize,
}

impl Parser {
    fn new(expression: &str) -> Self {
        Self {
            chars: expression.chars().collect(),
            at: 0,
            depth: 0,
        }
    }

    /// 解析完整表达式：结束后必须没有剩余字符，`1+` 与 `(1+2` 都在这里被判为语法错误。
    fn parse(mut self) -> Result<f64, ToolError> {
        let value = self.expr()?;
        self.skip_blank();
        if self.at < self.chars.len() {
            return Err(syntax_error());
        }
        Ok(value)
    }

    fn expr(&mut self) -> Result<f64, ToolError> {
        let mut value = self.term()?;
        loop {
            match self.peek() {
                Some('+') => {
                    self.at += 1;
                    value += self.term()?;
                }
                Some('-') => {
                    self.at += 1;
                    value -= self.term()?;
                }
                _ => return Ok(value),
            }
        }
    }

    fn term(&mut self) -> Result<f64, ToolError> {
        let mut value = self.factor()?;
        loop {
            match self.peek() {
                Some('*') => {
                    self.at += 1;
                    value *= self.factor()?;
                }
                Some('/') => {
                    self.at += 1;
                    value /= self.divisor()?;
                }
                Some('%') => {
                    self.at += 1;
                    // Rust 的 `%` 对被除数取符号：`-3 % 2 == -1`（P3-021 的判据之一）。
                    value %= self.divisor()?;
                }
                _ => return Ok(value),
            }
        }
    }

    fn factor(&mut self) -> Result<f64, ToolError> {
        let sign = match self.peek() {
            Some('-') => {
                self.at += 1;
                -1.0
            }
            Some('+') => {
                self.at += 1;
                1.0
            }
            _ => 1.0,
        };
        let value = match self.peek() {
            Some('(') => {
                self.at += 1;
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err(ToolError::Failed(format!("表达式嵌套过深（> {MAX_DEPTH} 层）")));
                }
                let value = self.expr()?;
                if self.peek() != Some(')') {
                    return Err(syntax_error());
                }
                self.at += 1;
                self.depth -= 1;
                value
            }
            Some(character) if character.is_ascii_digit() || character == '.' => self.number()?,
            _ => return Err(syntax_error()),
        };
        Ok(sign * value)
    }

    /// 除零单独判定：`f64` 的除零会得到 `inf`/`NaN`，那不是模型想看到的结果。
    fn divisor(&mut self) -> Result<f64, ToolError> {
        let value = self.factor()?;
        if value == 0.0 {
            return Err(ToolError::Failed("除数不能为零".to_owned()));
        }
        Ok(value)
    }

    /// 读一个数字：数字与至多一个 `.`；解析失败（例如只有一个 `.`）是语法错误。
    fn number(&mut self) -> Result<f64, ToolError> {
        let start = self.at;
        let mut seen_dot = false;
        while let Some(character) = self.chars.get(self.at).copied() {
            if character.is_ascii_digit() {
                self.at += 1;
                continue;
            }
            if character == '.' && !seen_dot {
                seen_dot = true;
                self.at += 1;
                continue;
            }
            break;
        }
        let text: String = self.chars[start..self.at].iter().collect();
        text.parse::<f64>().map_err(|_| syntax_error())
    }

    /// 跳过 ASCII 空白：表达式里允许空格与制表符，避免模型因排版被判非法。
    fn skip_blank(&mut self) {
        while matches!(self.chars.get(self.at), Some(' ' | '\t')) {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_blank();
        self.chars.get(self.at).copied()
    }
}

/// 语法错误是固定文案：`ToolError` 的 `Display` 会补上「工具执行失败：」前缀。
fn syntax_error() -> ToolError {
    ToolError::Failed("表达式语法错误".to_owned())
}

/// 允许出现在表达式里的字符：ASCII 数字、`.`、四则与取模、括号、空白。
fn is_supported(character: char) -> bool {
    matches!(
        character,
        '0'..='9' | '.' | '+' | '-' | '*' | '/' | '%' | '(' | ')' | ' ' | '\t'
    )
}

/// 结果格式：整数输出整数形式，否则保留最多 [`MAX_SCALE`] 位小数并去掉尾随零（D-P3-015）。
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < INTEGER_LIMIT {
        return format!("{}", value as i64);
    }
    let text = format!("{value:.precision$}", precision = MAX_SCALE);
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}
