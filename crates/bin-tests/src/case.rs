//! 用例声明：名称、资源类型与异步执行函数。

use crate::context::TestContext;
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

/// 用例执行函数返回的 future。
pub type CaseFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

/// 用例执行函数：借用上下文，返回可并发调度的 future。
pub type CaseFn = for<'a> fn(&'a TestContext) -> CaseFuture<'a>;

/// 一个测试用例。
#[derive(Clone, Copy)]
pub struct Case {
    /// 用例名，形如 `<领域>.<场景>`。
    pub name: &'static str,
    /// 需要的服务端形态。
    pub kind: CaseKind,
    /// 执行函数。
    pub run: CaseFn,
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
    /// `--list` 与结果输出里的短标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Isolated => "isolated",
        }
    }
}
