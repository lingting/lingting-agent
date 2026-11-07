//! 会话业务接口（P1 Mock）。
//!
//! 契约与 P2 的真实实现同名同构：P2 只替换 `service-core` 的内部实现，Handler 签名不变。
//! `stream` 不是接口，而是把事件流编码成 SSE 的适配层。

pub mod api;
mod stream;
