//! 运行时契约：客户端连接与会话占用（P1.1）。
//!
//! 领域与 `session` 平级：`client` 描述注册、心跳与下行事件，`occupancy` 描述一对一的会话占用。
//! 类型全部用 `auto_type` / `auto_enum` 标注，参与 TS / Rust SDK 生成；传输细节（帧编码、
//! 响应头、HTTP 状态码）不在本模块，见 `docs/develop.md` 的 SSE 约定。

mod client;
mod occupancy;

pub use client::{
    ClientCloseReason, ClientCreatePO, ClientCreateVO, ClientDeletePO, ClientEvent, ClientEventKind,
    ClientStreamParams, ClientUpdateKind, ClientUpdatePO, ClientUpdateVO,
};
pub use occupancy::{OccupancyCreatePO, OccupancyDeletePO, OccupancyGetParams, OccupancyVO};
