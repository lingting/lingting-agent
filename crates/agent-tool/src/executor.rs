//! 工具执行器：一次调用只有一个入口，各步顺序固定（README §4.4，P3-006）。
//!
//! ```text
//! ① 查找工具（未注册 → DENIED）
//! ② 权限判定（deny → DENIED，不执行；confirm → ConfirmRequired，交回 Loop 挂起）
//! ③ 执行工具（参数校验由工具自己用 `schema::parse` 完成，失败 → INVALID_ARGUMENTS）
//! ④ 超时与取消并列 select：超时是「工具失败」，取消是「Run 终止」（D-P3-019）
//! ⑤ 归一化：截断到 `max_result_chars`，错误也是模型可读文本
//! ⑥ 产出审计记录（落库由调用方经 Service 完成，本 Crate 不写库）
//! ```
//!
//! 执行器不认识具体工具：并发语义取工具的 `concurrency()`，权限缺省等级取 `risk()`，
//! 全程不按工具名特判。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::audit::{ToolCallRecord, ToolCallStatus};
use crate::call::ToolCall;
use crate::context::ToolContext;
use crate::definition::ToolFileAccess;
use crate::error::ToolError;
use crate::output::truncate;
use crate::permission::ToolPermission;
use crate::registry::ToolRegistry;
use crate::tool::Tool;

/// 执行参数：由 `[tools]` 配置派生（`types-core` 的同名缺省值与这里一致）。
///
/// 本 Crate 不认识 `types-core` 的配置类型：装配层把配置读出来，逐项喂进这个结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolLimits {
    /// 单次工具调用超时（毫秒）。
    pub timeout_ms: u64,
    /// 单次结果回灌给模型的最大字符数。
    pub max_result_chars: usize,
    /// 一轮内并行执行上限。
    pub max_parallel_calls: usize,
    /// 可重试错误的重试次数上限（`0` 表示不重试）。
    pub max_retries: u32,
}

impl Default for ToolLimits {
    fn default() -> Self {
        Self {
            timeout_ms: 15_000,
            max_result_chars: 8_000,
            max_parallel_calls: 8,
            max_retries: 0,
        }
    }
}

/// 一次调用的结局。
///
/// 三态刻意把「需确认」与「失败」分开：确认不是失败，Loop 据此挂起 Run（D-P3-004）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecOutcome {
    /// 调用结束（成功、失败、超时、拒绝都在这条记录里）。
    Done(ToolCallRecord),
    /// 需要客户端确认：记录是 `PENDING` 的，工具没有被调用。
    ConfirmRequired {
        /// 等待确认的审计记录。
        record: ToolCallRecord,
    },
    /// 执行期间 Run 被取消：不产出记录。
    Cancelled,
}

impl ExecOutcome {
    /// 取本次结局的记录（取消时没有记录）。
    pub fn record(&self) -> Option<&ToolCallRecord> {
        match self {
            Self::Done(record) | Self::ConfirmRequired { record } => Some(record),
            Self::Cancelled => None,
        }
    }
}

/// 工具执行器：持有只读注册表、执行参数与应用根目录。
pub struct ToolExecutor {
    registry: Arc<ToolRegistry>,
    limits: ToolLimits,
    home: PathBuf,
}

impl ToolExecutor {
    /// 装配执行器。
    pub fn new(registry: Arc<ToolRegistry>, limits: ToolLimits, home: PathBuf) -> Self {
        Self { registry, limits, home }
    }

    /// 执行参数。
    pub fn limits(&self) -> ToolLimits {
        self.limits
    }

    /// 注册表（同一轮的批量执行要用它读并发语义）。
    pub(crate) fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    /// 执行一次调用。
    pub async fn execute(&self, call: ToolCall, cancel: &CancellationToken) -> ExecOutcome {
        self.execute_of(call, cancel, false).await
    }

    /// 执行一次调用；`approved` 为真时把 `confirm` 权限当成已批准（挂起恢复专用，D-P3-023）。
    ///
    /// 只有「需确认」这一档被放行：`deny` 仍然拒绝，参数校验、超时、取消与归一化的顺序不变。
    pub(crate) async fn execute_of(&self, call: ToolCall, cancel: &CancellationToken, approved: bool) -> ExecOutcome {
        // ① 查找：未注册与「模型凭空构造的名字」都归到这里。
        let Some(tool) = self.registry.lookup(&call.tool_name) else {
            return self.denied(&call, ToolFileAccess::None);
        };
        // ② 权限：拒绝的工具不执行，需确认的工具交回 Loop 挂起（已批准的不再拦）。
        match self.registry.policy().permission_of(&call.tool_name, tool.risk()) {
            ToolPermission::Allow => {}
            ToolPermission::Deny => return self.denied(&call, tool.file_access()),
            ToolPermission::Confirm if !approved => {
                return ExecOutcome::ConfirmRequired {
                    record: ToolCallRecord::pending(&call, tool.file_access()),
                };
            }
            ToolPermission::Confirm => {}
        }
        // ③④ 执行：参数校验在工具内部（`schema::parse`），超时与取消在这里并列等待。
        let outcome = self.run(call, tool.as_ref(), cancel).await;
        let Some(record) = outcome else {
            return ExecOutcome::Cancelled;
        };
        ExecOutcome::Done(record)
    }

    /// 执行并归一化；取消返回 `None`。
    async fn run(&self, call: ToolCall, tool: &dyn Tool, cancel: &CancellationToken) -> Option<ToolCallRecord> {
        let started = Instant::now();
        let timeout = Duration::from_millis(self.limits.timeout_ms);
        let mut attempt: u32 = 0;
        let result = loop {
            let context = self.context(&call);
            let called = tokio::select! {
                () = cancel.cancelled() => return None,
                called = tokio::time::timeout(timeout, tool.call(&context, call.arguments.clone())) => called,
            };
            match called {
                // 超时不重试：它已经是「等太久」的信号（D-P3-008）。
                Err(_elapsed) => {
                    break Err(ToolError::Timeout {
                        limit_ms: self.limits.timeout_ms,
                    });
                }
                Ok(Ok(output)) => break Ok(output),
                Ok(Err(error)) => {
                    // 重试次数计入 `duration_ms`，事件只在最终结果上发一次。
                    if attempt < self.limits.max_retries && tool.retryable(&error) {
                        attempt += 1;
                        continue;
                    }
                    break Err(error);
                }
            }
        };

        let duration_ms = elapsed_ms(started);
        let file_access = tool.file_access();
        let record = match result {
            Ok(output) => {
                let status = if output.is_error {
                    ToolCallStatus::Error
                } else {
                    ToolCallStatus::Ok
                };
                self.finish(&call, file_access, status, &output.text, duration_ms)
            }
            Err(error) => self.finish(&call, file_access, status_of(&error), &error.to_string(), duration_ms),
        };
        Some(record)
    }

    /// 权限拒绝 / 未注册：产出 `DENIED` 记录，不调用工具。
    fn denied(&self, call: &ToolCall, file_access: ToolFileAccess) -> ExecOutcome {
        let text = ToolError::Denied(call.tool_name.clone()).to_string();
        ExecOutcome::Done(ToolCallRecord::pending(call, file_access).finished(ToolCallStatus::Denied, text, 0))
    }

    /// 归一化并收尾：结果文本按 `max_result_chars` 截断。
    fn finish(
        &self,
        call: &ToolCall,
        file_access: ToolFileAccess,
        status: ToolCallStatus,
        text: &str,
        duration_ms: u64,
    ) -> ToolCallRecord {
        let (text, _truncated) = truncate(text, self.limits.max_result_chars);
        ToolCallRecord::pending(call, file_access).finished(status, text, duration_ms)
    }

    /// 一次调用的执行上下文：只放既定事实，工具拿不到注册表 / 数据库 / Provider。
    fn context(&self, call: &ToolCall) -> ToolContext {
        ToolContext {
            home: self.home.clone(),
            session_id: call.session_id.clone(),
            run_id: call.run_id.clone(),
            tool_call_id: call.tool_call_id.clone(),
            deadline: Instant::now() + Duration::from_millis(self.limits.timeout_ms),
        }
    }
}

/// 错误 → 审计状态。
fn status_of(error: &ToolError) -> ToolCallStatus {
    match error {
        ToolError::InvalidArguments(_) => ToolCallStatus::InvalidArguments,
        ToolError::Denied(_) => ToolCallStatus::Denied,
        ToolError::Timeout { .. } => ToolCallStatus::Timeout,
        // 工具在 `call` 里返回 `ConfirmRequired` 属于契约违约（确认只由执行器判定），按失败收尾。
        ToolError::ConfirmRequired | ToolError::Retryable(_) | ToolError::Failed(_) => ToolCallStatus::Error,
    }
}

/// 耗时（毫秒）：转成 `u64` 时饱和，不 panic。
fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}
