//! 用例声明：名称、资源类型与异步执行函数。

use crate::context::TestContext;
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

/// 用例成功后返回的结果信息：按条目记录实际观测，写入用例报告文档。
pub type CaseOutcome = Vec<String>;

/// 用例执行函数返回的 future。
pub type CaseFuture<'a> = Pin<Box<dyn Future<Output = Result<CaseOutcome>> + Send + 'a>>;

/// 用例执行函数：借用上下文，返回可并发调度的 future。
pub type CaseFn = for<'a> fn(&'a TestContext) -> CaseFuture<'a>;

/// 一个测试用例。
#[derive(Clone, Copy)]
pub struct Case {
    /// 用例名，形如 `<领域>.<场景>`。
    pub name: &'static str,
    /// 需要的服务端形态。
    pub kind: CaseKind,
    /// 执行前需要准备的测试环境；入口会在启动服务端之前按需准备一次。
    pub prep: &'static [Prep],
    /// 执行函数。
    pub run: CaseFn,
}

/// 用例对测试环境的额外前置要求。
///
/// 与 [`CaseKind`] 分开：`kind` 决定用哪个服务端，`prep` 决定启动服务端之前要先准备什么环境。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Prep {
    /// 需要 WebUI 构建产物（`lingting-agent-ui/dist`）才能执行。
    WebUiBuild,
}

/// 用例与服务端资源的绑定方式。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CaseKind {
    /// 复用入口统一启动的共享服务端。
    Shared,
    /// 自起独立服务端，用于有副作用或需要干净状态的用例。
    Isolated,
}

impl CaseKind {
    /// `--list`、控制台与报告里的短标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Isolated => "isolated",
        }
    }
}
