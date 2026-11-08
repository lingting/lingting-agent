//! 查询参数编码：把 SDK 生成的 JSON 查询载荷摊平成 URL 查询串。
//!
//! 生成的接口把查询载荷序列化成 JSON 交给 Transport，服务端的 `Query<T>` 从查询串重建
//! JSON 对象（`framework-web` 的 `query_json`）：单个值为字符串，多个同名值为数组。
//! 因此这里只做「JSON 对象 → 键值对」的摊平与百分号编码，字段名保持 SDK 产出的 camelCase。

use crate::client::ClientError;
use serde_json::Value;
use url::form_urlencoded::Serializer;

/// 把 JSON 对象编码成查询串；空对象返回空串。
pub(crate) fn encode(payload: &[u8]) -> Result<String, ClientError> {
    let value: Value = serde_json::from_slice(payload).map_err(|error| ClientError::Decode {
        detail: format!("查询参数不是有效的 JSON：{error}"),
    })?;
    let object = value.as_object().ok_or_else(|| ClientError::Unsupported {
        detail: "查询参数必须是 JSON 对象".to_string(),
    })?;

    let mut serializer = Serializer::new(String::new());
    for (name, value) in object {
        append(&mut serializer, name, value)?;
    }
    Ok(serializer.finish())
}

/// 写入一个字段；数组按同名重复写入，`null` 与嵌套对象直接报错或跳过。
fn append(serializer: &mut Serializer<String>, name: &str, value: &Value) -> Result<(), ClientError> {
    match value {
        // `Option` 字段为 `None` 时序列化成 `null`，对应“未传该查询参数”。
        Value::Null => Ok(()),
        Value::String(text) => {
            serializer.append_pair(name, text);
            Ok(())
        }
        Value::Bool(flag) => {
            serializer.append_pair(name, if *flag { "true" } else { "false" });
            Ok(())
        }
        Value::Number(number) => {
            serializer.append_pair(name, &number.to_string());
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                append(serializer, name, item)?;
            }
            Ok(())
        }
        Value::Object(_) => Err(ClientError::Unsupported {
            detail: format!("查询参数 {name} 不支持嵌套对象"),
        }),
    }
}
