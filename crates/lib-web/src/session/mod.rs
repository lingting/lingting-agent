//! 会话业务接口（P1 Mock）。
//!
//! 契约与 P2 的真实实现同名同构：P2 只替换 `service-core` 的内部实现，Handler 签名不变。
//! 事件推送只有 `api/client/stream` 一条通道（D-P1.1-004），会话接口不再编码 SSE。

pub mod api;
