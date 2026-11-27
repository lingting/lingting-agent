//! Run 执行端口（D-P2-028）：本模块只定义输入 / 结局与 trait，不出现任何 Agent 概念。
//!
//! 执行队列（装配层 `manager-agent`）持 [`RunDriver`] 的实现并在 `run_one` 里调用；实现方
//! （`agent-runtime`）负责组装 Context、调用模型、逐段产出事件并经 [`SessionService`] 落库。
//! 端口放在本 Crate 是为了让「谁执行」与「执行队列在哪」解耦，同时保持依赖方向
//! `agent-runtime -> service-core`（`service-core` 不反向依赖任何 `agent-*`）。
//!
//! 结局只描述「发生了什么」，终态写入与终态事件都由实现方完成：[`RunOutcome::Completed`]
//! 表示助手消息与 `COMPLETED` 已在同一事务提交，[`RunOutcome::SessionGone`] 表示未提交任何终态。

use std::sync::{Arc, OnceLock};

use agent_core::session::{ResumePoint, Run, RunEvent};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::{CancellationToken, WaitForCancellationFuture};

/// 一次 Run 执行的输入。
pub struct RunInput {
    /// 已被推进到 `RUNNING` 的 Run。
    pub run: Run,
    /// 本次 Run 对应的用户消息正文（作为最后一条消息进入请求）。
    pub prompt: String,
    /// 恢复点：`Some` 表示这次执行是「从中断处继续」，执行方先推进 `PAUSED -> RUNNING`（D-P6-007）。
    ///
    /// 由 [`RunDriver::plan_resume`] 判定并产出；判定为不可恢复时不构造本字段，Run 直接落终态。
    pub resume: Option<ResumePoint>,
    /// 事件生产方的写入端：所有下行事件只能经它产出（先落盘再转发由写者负责）。
    pub events: mpsc::Sender<RunEvent>,
    /// 停止信号：取消来源与停止类别都由触发方决定。
    pub cancel: RunCancel,
}

/// 停止类别：决定执行方发出的终态事件类型（D-P2-029）。
///
/// 用户取消、释放占用与断连都落 `run_cancelled`；只有服务关闭落 `server_shutdown`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunStop {
    /// 取消：`api/run/cancel`、占用释放、客户端断连、启动自愈。
    #[default]
    Cancelled,
    /// 服务关闭：执行方发 `server_shutdown`（`status=CANCELLED`）。
    Shutdown,
}

/// 停止信号：取消 token + 停止类别。
///
/// 与裸 [`CancellationToken`] 相比多带一个类别，因为终态事件的类型只有停止方知道
/// （P2-024 第 2、3 条：断连与关闭必须分别落 `run_cancelled` 与 `server_shutdown`）。
/// 触发顺序固定为「先写库、再触发信号」，执行方据此从数据库取取消原因（D-P2-010）。
#[derive(Debug, Clone, Default)]
pub struct RunCancel {
    /// 取消 token：执行方在 `select!` 里等它，命中的分支负责 drop 模型流。
    token: CancellationToken,
    /// 停止类别：只有第一次触发生效。
    stop: Arc<OnceLock<RunStop>>,
}

impl RunCancel {
    /// 构造一个未触发的停止信号。
    pub fn new() -> Self {
        Self::default()
    }

    /// 触发停止并记录类别；重复触发只保留第一次的类别，token 已取消时不再重复通知。
    pub fn cancel(&self, stop: RunStop) {
        self.stop.set(stop).ok();
        self.token.cancel();
    }

    /// 等待停止信号：执行方在 `select!` 里与模型流并列等待它。
    pub fn cancelled(&self) -> WaitForCancellationFuture<'_> {
        self.token.cancelled()
    }

    /// 停止类别；未触发时按取消处理。
    pub fn stop(&self) -> RunStop {
        self.stop.get().copied().unwrap_or_default()
    }

    /// 取消 token 本身：执行方把它交给工具执行器（工具层不认识 `RunCancel`）。
    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// 是否已触发。
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}

/// 一次 Run 执行的结局。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// 正常结束：助手消息与 `COMPLETED` 已提交，`run_completed` 已发出。
    Completed,
    /// 失败收尾：`FAILED` 已写入（脱敏文案），`run_failed` 已发出。
    Failed {
        /// 对外可见的脱敏文案。
        public_error: String,
    },
    /// 取消收尾：`CANCELLED` 已在别处写入，`run_cancelled` 已发出。
    Cancelled {
        /// 事件里携带的取消原因。
        reason: String,
    },
    /// 挂起：命中工具确认，Run 已回到 `WAITING` 并让出并发许可（D-P3-027）。
    ///
    /// 既不是失败也不是取消：客户端决定后由队列重新取（`resume_run` 会把状态退回 `QUEUED`）。
    Suspended {
        /// 等待确认的调用标识。
        tool_call_id: String,
    },
    /// 运行时会话已停止：未提交任何终态，也没有终态事件。
    SessionGone,
}

/// 恢复前的判定结果（D-P6-007）：恢复点唯一，损坏原因是对外固定文案，旧 Run 单独成一态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumePlan {
    /// 可恢复：从该检查点继续。
    Recoverable(ResumePoint),
    /// 不可恢复：原因已是对外固定文案，Run 直接落 `FAILED`，不重试也不从头执行。
    Unrecoverable(String),
    /// 没有任何检查点：升级前的旧 Run（R-7），不恢复也不判死，保持可取消的中间态。
    NoCheckpoint,
}

/// Run 执行端口：装配层持有实现并注入，执行队列只依赖本 trait。
#[async_trait]
pub trait RunDriver: Send + Sync {
    /// 执行一个 `RUNNING` 的 Run，返回结局；终态写入与终态事件由实现方负责。
    async fn drive(&self, input: RunInput) -> RunOutcome;

    /// 恢复前判定：读最近检查点与已提交轨迹，产出确定恢复点或不可恢复原因（D-P6-007）。
    ///
    /// 返回 `Err` 只表示「判定所需的读库失败」（对外固定文案）：它不是数据损坏，
    /// 调用方不应据此把 Run 判成不可恢复。
    async fn plan_resume(&self, run: &Run) -> Result<ResumePlan, String>;
}
