//! 与领域无关的通用支持：用例上下文、服务端子进程、会话流程、路径与前端命令、运行观测报告。
//!
//! 平铺放在这里（R3），入口与调度框架（`main` / `cli` / `runner` / `report` / `case`）留在顶层。
//! 依赖方向（R4）：`cases → {util, mock}`、`mock → util`，本目录不依赖 `cases`。

pub(crate) mod context;
pub(crate) mod flow;
pub(crate) mod frontend;
pub(crate) mod mcp;
pub(crate) mod observability;
pub(crate) mod paths;
pub(crate) mod prepare;
pub(crate) mod process;
pub(crate) mod server;
pub(crate) mod stress;
pub(crate) mod support;
