//! 会话业务入口：Session / Message / Run 的读写规则，数据落在 Repository（SQLite）。
//!
//! 行为边界：
//!
//! - 发送消息只写入用户消息与 `QUEUED` 的 Run，不产出助手回复；回复内容与状态推进由运行时会话
//!   的执行队列负责（`run_trigger`，P2 后续由 Agent Runtime 接管）。
//! - 会话被占用时新 Run 排进执行队列；未占用时停在 `QUEUED`。唤醒动作由装配层
//!   （`manager-agent`）在发送之后完成，本 Service 不感知运行时。
//! - 取消只允许 `QUEUED` / `RUNNING` 的 Run；终态写入一律是条件更新（D-P2-030）。

use std::sync::Arc;

use agent_core::session::{
    CHECKPOINT_KEEP_LAST, Message, MessageListParams, MessageRole, MessageSendPO, MessageSendVO, Run, RunCancelPO,
    RunCheckpoint, RunGetParams, RunStatus, SessionCreatePO, SessionSummary,
};
use agent_tool::{ToolCallRecord, ToolCallStatus};
use lib_core::{id, time};
use lib_db::DbError;

use crate::repository::{CheckpointError, Repository};
use crate::session::{PendingToolCall, ResumeDecision, SessionError};

/// 未指定标题时的默认会话标题。
const DEFAULT_TITLE: &str = "新会话";
/// 未指定模型时的默认模型标识；P2 后续由 Provider / Model 配置决定。
const DEFAULT_MODEL: &str = "mock-model";
/// 结束 Run 时的取消原因：由业务主动取消（`api/run/cancel`，D-P2-029 的固定文本）。
pub const REASON_CANCELLED: &str = "用户取消执行";

/// 会话业务服务。
pub struct SessionService {
    /// 数据访问层；运行时会话与执行队列共享同一份 Repository，因此用 `Arc`。
    repository: Arc<Repository>,
}

impl SessionService {
    /// 创建会话服务实例。
    pub fn new(repository: Arc<Repository>) -> Self {
        Self { repository }
    }

    /// 创建会话。
    pub async fn create_session(&self, params: SessionCreatePO) -> Result<SessionSummary, SessionError> {
        let now_millis = time::now_millis();
        let session = SessionSummary {
            id: next_id()?,
            title: params
                .title
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_TITLE.to_string()),
            model: params
                .model
                .filter(|model| !model.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            message_count: 0,
            created_at_ms: now_millis,
            updated_at_ms: now_millis,
        };
        self.repository.insert_session(&session).await.map_err(internal)?;
        Ok(session)
    }

    /// 列出全部会话，按最后更新时间倒序。
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionError> {
        let mut sessions = self.repository.sessions().await.map_err(internal)?;
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
        Ok(sessions)
    }

    /// 统计执行中（`RUNNING`）的 Run 数量，供探活接口报告运行状态（P2-025 第 3 条）。
    pub async fn running_run_count(&self) -> Result<i64, SessionError> {
        self.repository.running_run_count().await.map_err(internal)
    }

    /// 会话是否存在：占用建立前的校验，避免为不存在的会话开事件文件。
    pub async fn has_session(&self, session_id: &str) -> Result<bool, SessionError> {
        self.repository.has_session(session_id).await.map_err(internal)
    }

    /// 取指定会话的消息，按写入顺序返回。
    pub async fn list_messages(&self, params: MessageListParams) -> Result<Vec<Message>, SessionError> {
        self.require_session(&params.session_id).await?;
        self.repository.messages(&params.session_id).await.map_err(internal)
    }

    /// 发送消息：写入用户消息并创建对应的 Run（同一个事务）。
    pub async fn send_message(&self, params: MessageSendPO) -> Result<MessageSendVO, SessionError> {
        let session = self.require_session(&params.session_id).await?;
        if params.content.trim().is_empty() {
            return Err(SessionError::Invalid("消息内容不能为空".to_string()));
        }

        let now_millis = time::now_millis();
        let run = Run {
            id: next_id()?,
            session_id: session.id.clone(),
            status: RunStatus::Queued,
            model: session.model.clone(),
            error: None,
            interrupted_reason: None,
            resume_count: 0,
            last_checkpoint_seq: None,
            input_tokens: None,
            output_tokens: None,
            created_at_ms: now_millis,
            updated_at_ms: now_millis,
            finished_at_ms: None,
        };
        let message = Message {
            id: next_id()?,
            session_id: session.id,
            run_id: Some(run.id.clone()),
            role: MessageRole::User,
            content: params.content,
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            model: None,
            input_tokens: None,
            output_tokens: None,
            created_at_ms: now_millis,
        };
        self.repository
            .with_run_exchange(&run, &message, now_millis)
            .await
            .map_err(internal)?;
        Ok(MessageSendVO { message, run })
    }

    /// 取消 Run：只允许取消未结束的 Run。
    pub async fn cancel_run(&self, params: RunCancelPO) -> Result<Run, SessionError> {
        let mut run = self.require_run(&params.run_id).await?;
        if !is_open(run.status) {
            return Err(not_cancellable(run.status));
        }

        let now_millis = time::now_millis();
        // 条件更新失败说明状态在读到写之间被推进：重新判定，按“已终态不可取消”处理。
        let cancelled = self
            .repository
            .cancel_run(&params.run_id, Some(REASON_CANCELLED), now_millis)
            .await
            .map_err(internal)?;
        if !cancelled {
            let current = self.require_run(&params.run_id).await?;
            return Err(not_cancellable(current.status));
        }
        run.status = RunStatus::Cancelled;
        run.error = Some(REASON_CANCELLED.to_string());
        run.updated_at_ms = now_millis;
        run.finished_at_ms = Some(now_millis);
        Ok(run)
    }

    /// 查询 Run。
    pub async fn get_run(&self, params: RunGetParams) -> Result<Run, SessionError> {
        self.require_run(&params.run_id).await
    }

    /// 取该会话最早的 `QUEUED` Run（执行队列专用，不推进状态）。
    ///
    /// 推进到 `RUNNING` 由执行方在拿到全局并发许可后调用 [`Self::start_run`] 完成（P2-025）：
    /// 排队等许可的 Run 停在 `QUEUED`，客户端看到的是「等待执行」。
    pub async fn next_queued_run(&self, session_id: &str) -> Result<Option<Run>, SessionError> {
        // 先把引用收敛成局部值：执行队列在 `tokio::spawn` 的任务里调用本方法，future 必须是
        // `Send`，而「跨 await 持有 `&str`」在当前 rustc 上会让 `Send` 推导失败（D-P2-033）。
        let session_id = session_id.to_string();
        self.repository.queued_run(&session_id).await.map_err(internal)
    }

    /// 把 `QUEUED` 的 Run 推进到 `RUNNING`；返回是否真的推进（0 行表示已被取消或已终态）。
    pub async fn start_run(&self, run_id: &str) -> Result<bool, SessionError> {
        self.repository
            .start_run(run_id, time::now_millis())
            .await
            .map_err(internal)
    }

    /// 取 Run 对应的用户消息正文：执行 Run 时作为 prompt。
    pub async fn user_prompt(&self, session_id: &str, run_id: &str) -> Result<Option<String>, SessionError> {
        self.repository.user_prompt(session_id, run_id).await.map_err(internal)
    }

    /// Run 的当前状态；Run 不存在时为空（执行方在每段增量前确认 Run 仍在执行）。
    pub async fn run_status(&self, run_id: &str) -> Result<Option<RunStatus>, SessionError> {
        Ok(self.run(run_id).await?.map(|run| run.status))
    }

    /// Run 当前的错误文本：取消时事件里的原因取数据库里的权威值（D-P2-029）。
    pub async fn run_error(&self, run_id: &str) -> Result<Option<String>, SessionError> {
        Ok(self.run(run_id).await?.and_then(|run| run.error))
    }

    /// 本次 Run 之前的历史消息（按 `seq` 升序，不含本次输入）：Context 组装的历史来源。
    pub async fn history_before_run(&self, session_id: &str, run_id: &str) -> Result<Vec<Message>, SessionError> {
        self.repository
            .history_before_run(session_id, run_id)
            .await
            .map_err(internal)
    }

    /// 本次 Run 已落库的工具轨迹：挂起唤醒后重建「本 Run 发生过什么」（D-P3-023）。
    ///
    /// 只取本次 Run 的 assistant `tool_calls` 与 TOOL 结果，按 `seq` 升序；普通历史仍由
    /// [`Self::history_before_run`] 提供，两者拼起来就是恢复时的完整 Context 来源。
    pub async fn tool_trace_of_run(&self, session_id: &str, run_id: &str) -> Result<Vec<Message>, SessionError> {
        // 同 `next_queued_run`：跨 await 持有 `&str` 会让 `Send` 推导失败（D-P2-033）。
        let session_id = session_id.to_string();
        let run_id = run_id.to_string();
        self.repository
            .tool_trace_of_run(&session_id, &run_id)
            .await
            .map_err(internal)
    }

    /// 执行前写入本批调用的开始标记（`PENDING` + `started_at_ms`，P6A-006）。
    ///
    /// 返回是否为本次插入：已存在同键行说明库里已有这条调用的记录（数据不一致），
    /// 调用方据此把 Run 落 `FAILED`，不覆盖既有行。
    pub async fn begin_tool_call(
        &self,
        run_id: &str,
        session_id: &str,
        record: &ToolCallRecord,
    ) -> Result<bool, SessionError> {
        let id = next_id()?;
        self.repository
            .insert_tool_call(&id, run_id, session_id, record)
            .await
            .map_err(internal)
    }

    /// 收尾一条工具调用：只命中执行前写入的 `PENDING` 行（P6A-006）。
    ///
    /// 返回是否命中：0 行说明没有开始标记（数据不一致），由调用方按轨迹损坏处理；
    /// 写库失败只是环境问题，不影响已经成功的 Run（P3-018）。
    pub async fn finish_tool_call(&self, run_id: &str, record: &ToolCallRecord) -> Result<bool, SessionError> {
        self.repository.finish_tool_call(run_id, record).await.map_err(internal)
    }

    /// 某个 Run 的审计记录，按产生时间升序；没有记录时返回空列表。
    ///
    /// 含 `PENDING` 行：恢复判定要区分「有开始无结束」与「从未开始」（D-P6-007）。
    pub async fn tool_calls_of_run(&self, run_id: &str) -> Result<Vec<ToolCallRecord>, SessionError> {
        self.repository.tool_calls(run_id).await.map_err(internal)
    }

    /// 某个 Run 的审计记录（查询口径）：**不含 `PENDING` 行**（D-P6-008）。
    ///
    /// `PENDING` 是「已开始、未结束」的过程量，不是结果；对外暴露会让客户端看到
    /// 永不结束的调用。行在库里保留，恢复判定仍用 [`Self::tool_calls_of_run`]。
    pub async fn tool_audit_of_run(&self, run_id: &str) -> Result<Vec<ToolCallRecord>, SessionError> {
        let records = self.repository.tool_calls(run_id).await.map_err(internal)?;
        Ok(records
            .into_iter()
            .filter(|record| record.status != ToolCallStatus::Pending)
            .collect())
    }
    /// 会话已提交消息的 `seq` 上界：检查点用它记录「进度到哪一条」（D-P6-001）。
    pub async fn last_message_seq(&self, session_id: &str) -> Result<i64, SessionError> {
        self.repository.max_message_seq(session_id).await.map_err(internal)
    }

    /// 追加一条检查点：插行与 `runs.last_checkpoint_seq` 在同一事务里完成，返回本次序号（D-P6-001）。
    ///
    /// 写失败不降级：调用方按 D-P6-010 把 Run 落 `FAILED`，不继续执行。
    pub async fn append_checkpoint(&self, run_id: &str, checkpoint: &RunCheckpoint) -> Result<i64, CheckpointError> {
        self.repository.append_checkpoint(run_id, checkpoint).await
    }

    /// 某 Run 的最近一条检查点；升级前的旧 Run 没有检查点，返回空。
    pub async fn latest_checkpoint(&self, run_id: &str) -> Result<Option<RunCheckpoint>, CheckpointError> {
        self.repository.latest_checkpoint(run_id).await
    }

    /// 某 Run 的全部检查点，按序号升序：步骤历史与恢复一致性校验的输入。
    pub async fn checkpoints_of_run(&self, run_id: &str) -> Result<Vec<RunCheckpoint>, CheckpointError> {
        self.repository.checkpoints_of_run(run_id).await
    }

    /// Run 落终态后清理检查点：只保留最近 [`CHECKPOINT_KEEP_LAST`] 条，返回被删除的条数。
    ///
    /// 未结束的 Run（含 `PAUSED`）一条都不删：检查点正是它恢复的依据。
    pub async fn prune_checkpoints(&self, run_id: &str) -> Result<usize, CheckpointError> {
        let Some(run) = self.repository.run(run_id).await? else {
            return Ok(0);
        };
        if !run.status.is_terminal() {
            return Ok(0);
        }
        self.repository.prune_checkpoints(run_id, CHECKPOINT_KEEP_LAST).await
    }

    /// 一轮工具轨迹落库：assistant `tool_calls` 与全部 TOOL 结果同一事务写入（D-P3-024）。
    ///
    /// 进下一轮之前必须成功——写不进去就本轮失败，否则恢复时认不出「哪次调用没有结果」。
    pub async fn append_tool_round(&self, messages: &[Message]) -> Result<(), SessionError> {
        self.repository.with_tool_round(messages).await.map_err(internal)
    }

    /// 挂起：`RUNNING` 的 Run 退回 `QUEUED` 并记下待确认调用；返回是否命中（D-P3-023）。
    pub async fn suspend_run(&self, run_id: &str, tool_call_id: &str, tool_round: u32) -> Result<bool, SessionError> {
        self.repository
            .suspend_run(run_id, tool_call_id, tool_round, time::now_millis())
            .await
            .map_err(internal)
    }

    /// 唤醒：待确认标识一致时清空；驳回时同一事务把审计记录置 `DENIED`（D-P3-023）。
    ///
    /// 返回是否命中：并发两次唤醒只有一次命中，第二次按「待确认的调用不匹配」处理。
    pub async fn resume_run(
        &self,
        run_id: &str,
        tool_call_id: &str,
        decision: ResumeDecision,
    ) -> Result<bool, SessionError> {
        self.repository
            .with_resume_run(run_id, tool_call_id, decision, time::now_millis())
            .await
            .map_err(internal)
    }

    /// 该 Run 正在等待确认的调用；不处于挂起态时为空。
    pub async fn pending_tool_call(&self, run_id: &str) -> Result<Option<PendingToolCall>, SessionError> {
        self.repository.pending_tool_call(run_id).await.map_err(internal)
    }

    /// 某个会话里全部挂起中的 Run：重连补发确认事件与「取消时清挂起」共用。
    pub async fn pending_tool_calls_of_session(&self, session_id: &str) -> Result<Vec<PendingToolCall>, SessionError> {
        self.repository
            .pending_tool_calls_of_session(session_id)
            .await
            .map_err(internal)
    }

    /// 取消链路清挂起：挂起的 Run 落终态时一并清空待确认标识，不留悬挂挂起态。
    pub async fn clear_pending_tool_call(&self, run_id: &str) -> Result<bool, SessionError> {
        self.repository
            .clear_pending_tool_call(run_id, time::now_millis())
            .await
            .map_err(internal)
    }

    /// 取会话摘要：Context 组装需要会话标识，装配层也用它补齐缺省模型。
    pub async fn session_summary(&self, session_id: &str) -> Result<SessionSummary, SessionError> {
        self.require_session(session_id).await
    }

    /// 把未结束的 Run 落成 `FAILED`；返回是否真的推进（0 行表示已终态）。
    pub async fn fail_run(&self, run_id: &str, error: &str) -> Result<bool, SessionError> {
        self.repository
            .fail_run(run_id, Some(error), time::now_millis())
            .await
            .map_err(internal)
    }

    /// 完成 Run：条件推进到 `COMPLETED` 并写入助手消息；返回是否为本次推进。
    pub async fn complete_run(
        &self,
        run_id: &str,
        message: &Message,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
    ) -> Result<bool, SessionError> {
        self.repository
            .with_run_completion(run_id, message, input_tokens, output_tokens, time::now_millis())
            .await
            .map_err(internal)
    }

    /// 把未结束的 Run 落成 `CANCELLED`：执行前校验失败与服务关闭收尾共用（不区分结果）。
    pub async fn cancel_run_quietly(&self, run_id: &str, reason: &str) -> Result<(), SessionError> {
        self.repository
            .cancel_run(run_id, Some(reason), time::now_millis())
            .await
            .map_err(internal)?;
        Ok(())
    }

    /// 取消指定会话的全部未结束 Run，返回被取消的数量（释放占用时由运行时会话调用）。
    pub async fn cancel_session_runs(&self, session_id: &str, reason: &str) -> Result<usize, SessionError> {
        self.repository
            .cancel_session_runs(session_id, Some(reason), time::now_millis())
            .await
            .map_err(internal)
    }

    /// 取 Run；不存在时返回未找到错误。
    async fn require_run(&self, run_id: &str) -> Result<Run, SessionError> {
        self.run(run_id)
            .await?
            .ok_or_else(|| SessionError::NotFound(format!("Run 不存在：{run_id}")))
    }

    /// 取 Run；不存在时为空。
    async fn run(&self, run_id: &str) -> Result<Option<Run>, SessionError> {
        self.repository.run(run_id).await.map_err(internal)
    }

    /// 取会话；不存在时返回未找到错误。
    async fn require_session(&self, session_id: &str) -> Result<SessionSummary, SessionError> {
        self.repository
            .session(session_id)
            .await
            .map_err(internal)?
            .ok_or_else(|| SessionError::NotFound(format!("会话不存在：{session_id}")))
    }
}

/// Run 是否处于未结束状态（可取消、可执行）。
///
/// `WAITING`（等工具确认）与 `PAUSED`（非预期中断）也是未结束：它们没有终态，只是暂时不排队
/// （D-P3-027 / D-P6-004）。集合定义在 `RunStatus::is_active`，这里不另写列表。
pub fn is_open(status: RunStatus) -> bool {
    status.is_active()
}

/// 「已终态不可取消」的统一文案：与 P1 的 Mock 行为逐字一致。
fn not_cancellable(status: RunStatus) -> SessionError {
    SessionError::Invalid(format!("Run 当前状态为 {status}，不可取消"))
}

/// 生成业务 ID，把底层错误收敛为服务内部错误。
fn next_id() -> Result<String, SessionError> {
    id::next_id().map_err(|error| SessionError::Internal(format!("生成 ID 失败：{error:#}")))
}

/// 数据库错误对外只有内部错误：细节只进日志，不进响应体。
fn internal(error: DbError) -> SessionError {
    SessionError::Internal(format!("{error}"))
}
