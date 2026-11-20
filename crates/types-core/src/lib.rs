//! 通用领域类型：Provider / Model / Agent 配置与通用业务 DTO / 枚举，以及 P1.1 的客户端连接与会话占用契约。
//!
//! Session / Message / Run 的契约类型在 P2-006 迁入 `agent-core`（D-P2-018）；
//! `runtime`（客户端连接与会话占用）不属于 Agent 领域，保留在本 Crate。
//! `config`（P2-009）是内部配置类型，不参与 SDK 生成。

pub mod config;
pub mod runtime;
