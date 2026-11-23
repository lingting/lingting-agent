//! 跨 Crate 的公共基础能力：ID、时间、应用目录、日志、常量与通用值类型。

/// 日志审计与进程内计数（P2.2-003）：只在 `metrics` 特性下编译。
#[cfg(feature = "metrics")]
pub mod audit;
pub mod directorys;
pub mod id;
pub mod lifecycle;
pub mod logger;
pub mod time;
