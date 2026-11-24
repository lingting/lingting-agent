//! 一轮内多个调用的并行与串行分段（P3-007，D-P3-006）。
//!
//! - 连续的可并发调用（`ToolConcurrency::Parallel`）为一段，段内用 `buffered(n)` 并发执行；
//! - 独占调用（`Exclusive`）单独成段，段间串行，上一段全部结束才开始下一段；
//! - 结果**永远**按模型给出的调用顺序回填（`buffered` 保序），与完成顺序无关；
//! - 命中确认点时停止后续段：确认之后的调用不应该先跑掉。
//!
//! 并发语义来自工具自己的 `concurrency()`（默认由本地文件操作声明推导，D-P3-021），
//! 本模块不按工具名特判。

use futures_util::StreamExt;
use futures_util::stream;
use tokio_util::sync::CancellationToken;

use crate::audit::ToolCallRecord;
use crate::call::ToolCall;
use crate::definition::ToolConcurrency;
use crate::executor::{ExecOutcome, ToolExecutor};
use crate::permission::ToolPermission;

/// 一轮内多个调用的执行结局。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchOutcome {
    /// 按输入顺序排列的结局：命中确认点或被取消时被截断，未执行的调用不出现在这里。
    pub outcomes: Vec<ExecOutcome>,
    /// 是否因为 Run 被取消而中断。
    pub cancelled: bool,
}

impl BatchOutcome {
    /// 是否命中确认点（本轮要挂起，等客户端决定后重来）。
    pub fn confirm_point(&self) -> Option<&ToolCallRecord> {
        self.outcomes.iter().find_map(|outcome| match outcome {
            ExecOutcome::ConfirmRequired { record } => Some(record),
            _ => None,
        })
    }

    /// 已产出的审计记录，按输入顺序。
    pub fn records(&self) -> impl Iterator<Item = &ToolCallRecord> {
        self.outcomes.iter().filter_map(ExecOutcome::record)
    }
}

impl ToolExecutor {
    /// 执行一轮内的全部调用。
    pub async fn execute_batch(&self, calls: Vec<ToolCall>, cancel: &CancellationToken) -> BatchOutcome {
        self.execute_batch_of(calls, cancel, false).await
    }

    /// 执行一轮内的全部调用；`approved` 为真时把 `confirm` 权限当成已批准（只用于挂起恢复，
    /// 批准只对这一次调用有效，D-P3-023）。`deny` 与其余顺序不变。
    pub async fn execute_batch_of(
        &self,
        mut calls: Vec<ToolCall>,
        cancel: &CancellationToken,
        approved: bool,
    ) -> BatchOutcome {
        // 确认点可以提前识别（权限判定是纯计算）：先截掉它后面的调用，它们不应该被启动。
        // 已批准的批次没有确认点，因此不截断。
        if !approved && let Some(index) = self.confirm_index(&calls) {
            calls.truncate(index + 1);
        }

        let mut outcomes = Vec::with_capacity(calls.len());
        let mut cancelled = false;
        for segment in self.split_segments(calls) {
            let buffered = self.limits().max_parallel_calls.max(1);
            let futures = stream::iter(segment.into_iter().map(|call| self.execute_of(call, cancel, approved)))
                .buffered(buffered);
            let results: Vec<ExecOutcome> = futures.collect().await;

            // 段内保序：遇到取消或确认点，就把这一段的尾部丢掉。
            let mut stop = false;
            for outcome in results {
                let halted = matches!(outcome, ExecOutcome::Cancelled | ExecOutcome::ConfirmRequired { .. });
                if matches!(outcome, ExecOutcome::Cancelled) {
                    cancelled = true;
                }
                outcomes.push(outcome);
                if halted {
                    stop = true;
                    break;
                }
            }
            if stop {
                break;
            }
        }

        BatchOutcome { outcomes, cancelled }
    }

    /// 第一个「需确认」调用的下标：它之后的调用本轮不执行。
    fn confirm_index(&self, calls: &[ToolCall]) -> Option<usize> {
        calls.iter().position(|call| {
            let Some(tool) = self.registry().lookup(&call.tool_name) else {
                return false;
            };
            self.registry().policy().permission_of(&call.tool_name, tool.risk()) == ToolPermission::Confirm
        })
    }

    /// 按并发语义切段：连续可并发的调用合成一段，独占调用单独成段。
    fn split_segments(&self, calls: Vec<ToolCall>) -> Vec<Vec<ToolCall>> {
        let mut segments: Vec<Vec<ToolCall>> = Vec::new();
        let mut parallel: Vec<ToolCall> = Vec::new();
        for call in calls {
            let concurrency = self
                .registry()
                .lookup(&call.tool_name)
                .map(|tool| tool.concurrency())
                .unwrap_or(ToolConcurrency::Parallel);
            match concurrency {
                ToolConcurrency::Parallel => parallel.push(call),
                ToolConcurrency::Exclusive => {
                    if !parallel.is_empty() {
                        segments.push(std::mem::take(&mut parallel));
                    }
                    segments.push(vec![call]);
                }
            }
        }
        if !parallel.is_empty() {
            segments.push(parallel);
        }
        segments
    }
}
