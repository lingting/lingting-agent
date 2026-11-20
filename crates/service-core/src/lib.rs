//! 通用核心业务服务：会话业务（Service + Repository）。
//!
//! 数据落在 SQLite（经 `lib-db`），SQL 全部收敛在 [`repository`]。
//!
//! 本 Crate **只承载业务与数据访问**：客户端注册表、会话占用、事件文件与 Run 执行等
//! 「进程内运行时」在装配层 `manager-agent`（D-P2-035），因此这里既不持有进程级单例，
//! 也不感知 Agent / 运行时。

pub mod repository;
pub mod session;
