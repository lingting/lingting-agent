//! 把 `service-core` 产出的 Run 事件流转成 SSE 响应体。
//!
//! 事件 → SSE 帧的映射集中在这里：`seq` 作 `id`，`RunEventKind` 的显示值作 `event:`，
//! 整条事件的 JSON 作 `data:`。P2 替换的是 `RunEventStream` 的生产方，本文件不需要改。

use bytes::Bytes;
use futures_util::stream::{BoxStream, unfold};
use lib_web_core::sse;
use service_core::session::RunEventStream;

/// 组装 SSE 响应体：每条事件一帧，流的结束即连接关闭。
pub fn body(events: RunEventStream) -> BoxStream<'static, Result<Bytes, std::io::Error>> {
    Box::pin(unfold(events, |mut events| async move {
        let event = events.recv().await?;
        // 事件名与 JSON 里的 `event` 字段同源（见 `RunEventKind` 的 serde / strum 约定）。
        let name = event.event.to_string();
        let frame = sse::json_frame(event.seq, &name, &event).map_err(std::io::Error::other);
        Some((frame.map(Bytes::from), events))
    }))
}
