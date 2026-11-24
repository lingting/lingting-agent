//! 参数 schema 的生成与参数校验：单一来源 = 参数类型本身（D-P3-002）。
//!
//! 每个工具声明一个 `*Args`（`serde::Deserialize` + `schemars::JsonSchema`）：
//! [`of`] 由它生成发给模型的 schema，[`parse`] 用 serde 反序列化做校验。
//! `schemars` 只负责生成，**不参与校验**——不存在「schema 说可以、执行说不行」的第二条路径。
//!
//! 已知差异（写在这里，方便对照）：`Option<T>` 在 schema 里是「非必填」，`#[serde(default)]`
//! 在 schema 里仍是「必填但可省」；两者都以 serde 的接受/拒绝行为为准。

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::ToolError;

/// 面向模型的参数 JSON Schema（`{"type":"object","properties":…,"required":[…]}`）。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSchema(Value);

impl ToolSchema {
    /// 取 schema 本体。
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    /// 文本形式：模型请求与契约快照 DTO 都用它。
    pub fn to_json_string(&self) -> String {
        self.0.to_string()
    }
}

/// 由参数类型生成面向模型的 schema。
///
/// 剥掉 `$schema` / `title` / `$defs` 之类与「让模型填参数」无关的键：它们只会白占 token，
/// 而 `$defs` 与 `$ref` 的组合对扁平参数结构没有意义。
pub fn of<A: JsonSchema>() -> ToolSchema {
    let generated = serde_json::to_value(schemars::schema_for!(A)).unwrap_or(Value::Null);
    // `schema_for!` 对结构体必然产出对象；取不到对象时退化为空 schema，交由注册期的人工核对发现。
    let mut object = generated.as_object().cloned().unwrap_or_default();
    object.remove("$schema");
    object.remove("title");
    object.remove("$defs");
    ToolSchema(Value::Object(object))
}

/// 用 serde 反序列化校验参数。
///
/// 失败一律是 [`ToolError::InvalidArguments`]，文本里保留字段名与原因，去掉 serde 的
/// `at line x column y` 噪声（模型拿不到位置信息，也不需要）。
pub fn parse<A: DeserializeOwned>(args: Value) -> Result<A, ToolError> {
    serde_json::from_value(args).map_err(|error| ToolError::InvalidArguments(explain(&error)))
}

fn explain(error: &serde_json::Error) -> String {
    let text = error.to_string();
    let reason = match text.find(" at line ") {
        Some(position) => &text[..position],
        None => &text[..],
    };
    format!("参数不合法：{reason}")
}
