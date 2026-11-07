//! 通用领域类型：Settings、Provider、Model 与通用业务 DTO / 枚举。
//!
//! P1 没有 `agent-*` 领域 Crate，Session / Message / Run 的契约类型先落在本 Crate；
//! P2 建立 `agent-core` 后按 `docs/workspace.md` 第六节整体迁移，类型名与字段不变。

pub mod session;
