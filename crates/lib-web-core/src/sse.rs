//! SSE（Server-Sent Events）帧编码：`id` / `event` / `data` 的组装与响应头约定。
//!
//! 编码与业务无关，只负责把“一条事件”拼成标准 SSE 帧；业务事件的类型由各领域 crate 定义，
//! 响应组装由 `lib-web` 完成（P1 的唯一消费者是 `GET api/run/stream`，见 D-007、D-026）。

use serde::Serialize;

/// SSE 响应的 content-type。
pub const CONTENT_TYPE: &str = "text/event-stream; charset=utf-8";
/// 响应头：禁止中间层缓存事件流。
pub const HEADER_CACHE_CONTROL: &str = "cache-control";
/// `HEADER_CACHE_CONTROL` 的取值。
pub const VALUE_NO_CACHE: &str = "no-cache";
/// 响应头：让反向代理不缓冲事件流。
pub const HEADER_NO_BUFFERING: &str = "x-accel-buffering";
/// `HEADER_NO_BUFFERING` 的取值。
pub const VALUE_NO_BUFFERING: &str = "no";

/// 组装一帧 SSE：`id` + `event` + 逐行 `data`，以空行结束。
///
/// `data` 里的换行会被拆成多条 `data:` 行，符合 SSE 规范；因此调用方不必预先转义换行。
pub fn frame(id: i64, event: &str, data: &str) -> String {
    let mut output = String::with_capacity(data.len() + 32);
    output.push_str("id: ");
    output.push_str(&id.to_string());
    output.push('\n');
    output.push_str("event: ");
    output.push_str(event);
    output.push('\n');
    for line in data.split('\n') {
        output.push_str("data: ");
        output.push_str(line);
        output.push('\n');
    }
    output.push('\n');
    output
}

/// 把载荷序列化成 JSON 后组装成一帧 SSE。
///
/// JSON 里的换行会被序列化成 `\n` 转义，`data:` 因此始终只有一行；序列化失败由调用方决定如何收尾。
pub fn json_frame<T>(id: i64, event: &str, value: &T) -> Result<String, serde_json::Error>
where
    T: Serialize,
{
    let data = serde_json::to_string(value)?;
    Ok(frame(id, event, &data))
}
