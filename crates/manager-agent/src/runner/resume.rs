//! 恢复触发（P6A-011 / D-P6-009）：会话收到一条新消息后，把该会话的 `PAUSED` Run 逐个重新拾起来。
//!
//! 三条固定口径：
//!
//! - **触发只有一条**：会话已被占用 ∧ `api/message/create` 成功。只占用不发消息不恢复，
//!   取消仍走 `api/run/cancel`（D-P6-005），也没有后台 sweeper。
//! - **顺序固定**：先该会话全部 `PAUSED`（`updated_at_ms` 升序），再该会话 `QUEUED` Run；
//!   两段在同一个串行任务里，因此被恢复 Run 的 `run_completed` 一定早于新 Run 的 `run_started`。
//! - **判定先于推进**：这里只读、只调端口判定；`PAUSED → RUNNING` 是执行方的条件更新
//!   （拿到全局并发许可之后），影响 0 行即静默放弃（D-P6-006 / D-P6-011）。

use std::sync::Arc;

use agent_core::session::RunEvent;
use service_core::session::{ResumePlan, RunDriver, SessionService};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::cancel::RunCancels;
use super::queue::{fail, run_one};
use crate::runtime::metrics::{
    record_resume_attempt, record_resume_rejected_error, record_resume_rejected_no_checkpoint,
    record_resume_unrecoverable,
};

/// 拾取该会话全部 `PAUSED` Run；返回 `true` 表示执行队列应当退出（停止信号到达或生产方关闭）。
///
/// 逐条处置由恢复判定的三态决定：可恢复 → 交给执行方继续跑；不可恢复 → 直接落 `FAILED`；
/// 没有检查点（升级前的旧 Run）→ 不改行，保持可取消的中间态（`R-7`）。
pub(crate) async fn paused(
    service: &SessionService,
    driver: Option<&Arc<dyn RunDriver>>,
    cancels: &RunCancels,
    session_id: &str,
    stops: &mut mpsc::Receiver<()>,
    producer: &mpsc::Sender<RunEvent>,
) -> bool {
    let runs = match service.paused_runs(session_id).await {
        Ok(runs) => runs,
        Err(error) => {
            warn!(session_id, %error, "读取可恢复 Run 失败，本次不恢复");
            return false;
        }
    };
    for run in runs {
        // 未装配执行端口：判定与执行都无从谈起。这里不改行——把可恢复的中断态判死会永久
        // 摧毁可恢复性，而端口缺失是装配问题，不是这个 Run 的问题。
        let Some(driver) = driver else {
            warn!(run_id = %run.id, "执行端口未装配，可恢复 Run 保持中断态");
            continue;
        };
        record_resume_attempt();
        match driver.plan_resume(&run).await {
            Ok(ResumePlan::Recoverable(point)) => {
                info!(run_id = %run.id, checkpoint_seq = point.checkpoint_seq, "从检查点恢复 Run");
                run_one(service, Some(driver), cancels, run, Some(point), producer).await;
            }
            // 不可恢复：只落终态与固定文案，不重试、也不降级成「从头重跑」（D-P6-007）。
            Ok(ResumePlan::Unrecoverable(reason)) => {
                record_resume_unrecoverable();
                warn!(run_id = %run.id, %reason, "Run 不可恢复，直接落终态");
                fail(service, &run, &reason, producer).await;
            }
            // 没有检查点就没有「从哪儿继续」：保持中断态，等取消或下一次触发（`R-7`）。
            Ok(ResumePlan::NoCheckpoint) => {
                record_resume_rejected_no_checkpoint();
                info!(run_id = %run.id, "Run 没有检查点，保持中断态不恢复");
            }
            // 读库失败是环境问题，不是数据损坏：保留中断态，等下一次消息触发。
            Err(error) => {
                record_resume_rejected_error();
                warn!(run_id = %run.id, %error, "恢复判定读库失败，保持中断态");
            }
        }
        // 生产方关闭说明运行时会话已在收尾，再恢复下一个也没有意义。
        if producer.is_closed() {
            return true;
        }
        // 停止信号到达：剩余 Run 已由停止方收尾，这里不再恢复。
        if stops.try_recv().is_ok() {
            return true;
        }
    }
    false
}
