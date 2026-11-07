//! 会话业务入口：P1 用内存 Mock 实现，P2 换成真实存储与 Run 状态机。
//!
//! Mock 的行为边界：
//!
//! - 发送消息只写入用户消息与 `queued` 的 Run，不产出助手回复；
//!   回复内容与状态推进由 `subscribe_run` 启动的 Mock 事件流负责（P2 由 Agent Runtime 负责）。
//! - 取消只允许 `queued` / `running` 的 Run。

use std::sync::Arc;

use crate::session::store::Store;
use crate::session::stream;
use crate::session::{RunEventStream, SessionError};
use lib_core::{id, time};
use types_core::session::{
    Message, MessageListParams, MessageRole, MessageSendPO, MessageSendVO, Run, RunCancelPO, RunGetParams, RunStatus,
    RunStreamParams, SessionCreatePO, SessionSummary,
};

/// 未指定标题时的默认会话标题。
const DEFAULT_TITLE: &str = "新会话";
/// 未指定模型时的默认模型标识；P2 改由 Provider / Model 配置决定。
const DEFAULT_MODEL: &str = "mock-model";

/// 会话业务服务。
pub struct SessionService {
    /// 会话存储；P2 换成 Repository。事件流生产任务需要共享同一份存储，因此用 `Arc`。
    store: Arc<Store>,
}

impl Default for SessionService {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionService {
    /// 创建会话服务实例。
    pub fn new() -> Self {
        Self {
            store: Arc::new(Store::new()),
        }
    }

    /// 创建会话。
    pub fn create_session(&self, params: SessionCreatePO) -> Result<SessionSummary, SessionError> {
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
        self.store.insert_session(session.clone());
        Ok(session)
    }

    /// 列出全部会话，按最后更新时间倒序。
    pub fn list_sessions(&self) -> Vec<SessionSummary> {
        let mut sessions = self.store.sessions();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
        sessions
    }

    /// 取指定会话的消息，按写入顺序返回。
    pub fn list_messages(&self, params: MessageListParams) -> Result<Vec<Message>, SessionError> {
        self.require_session(&params.session_id)?;
        self.store
            .messages(&params.session_id)
            .ok_or_else(|| SessionError::NotFound(format!("会话不存在：{}", params.session_id)))
    }

    /// 发送消息：写入用户消息并创建对应的 Run。
    pub fn send_message(&self, params: MessageSendPO) -> Result<MessageSendVO, SessionError> {
        let session = self.require_session(&params.session_id)?;
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
            model: None,
            input_tokens: None,
            output_tokens: None,
            created_at_ms: now_millis,
        };
        self.store.insert_exchange(message.clone(), run.clone(), now_millis);
        Ok(MessageSendVO { message, run })
    }

    /// 取消 Run：只允许取消未结束的 Run。
    pub fn cancel_run(&self, params: RunCancelPO) -> Result<Run, SessionError> {
        let run = self.require_run(&params.run_id)?;
        if !matches!(run.status, RunStatus::Queued | RunStatus::Running) {
            return Err(SessionError::Invalid(format!(
                "Run 当前状态为 {}，不可取消",
                run.status
            )));
        }

        let now_millis = time::now_millis();
        self.store
            .update_run(&params.run_id, |run| {
                run.status = RunStatus::Cancelled;
                run.updated_at_ms = now_millis;
                run.finished_at_ms = Some(now_millis);
            })
            .ok_or_else(|| SessionError::NotFound(format!("Run 不存在：{}", params.run_id)))
    }

    /// 查询 Run。
    pub fn get_run(&self, params: RunGetParams) -> Result<Run, SessionError> {
        self.require_run(&params.run_id)
    }

    /// 订阅 Run 的事件流：把 `QUEUED` 的 Run 推进到 `RUNNING`，并启动 Mock 生产任务。
    ///
    /// 同一个 Run 只允许订阅一次：只有 `QUEUED` 可以订阅，其余状态返回参数错误（D-026）。
    pub fn subscribe_run(&self, params: RunStreamParams) -> Result<RunEventStream, SessionError> {
        let now_millis = time::now_millis();
        let run = self.store.start_run(&params.run_id, now_millis).ok_or_else(|| {
            // 推进失败时再读一次，区分「Run 不存在」与「状态不满足」。
            match self.store.run(&params.run_id) {
                Some(run) => SessionError::Invalid(format!("Run 当前状态为 {}，不可订阅事件流", run.status)),
                None => SessionError::NotFound(format!("Run 不存在：{}", params.run_id)),
            }
        })?;
        let prompt = self
            .store
            .messages(&run.session_id)
            .and_then(|messages| {
                messages.into_iter().find(|message| {
                    message.run_id.as_deref() == Some(run.id.as_str()) && matches!(message.role, MessageRole::User)
                })
            })
            .map(|message| message.content)
            .ok_or_else(|| SessionError::Internal(format!("Run {} 找不到对应的用户消息", run.id)))?;
        let assistant_message_id = next_id()?;
        Ok(stream::spawn(
            Arc::clone(&self.store),
            run,
            prompt,
            assistant_message_id,
        ))
    }

    /// 取会话；不存在时返回未找到错误。
    fn require_session(&self, session_id: &str) -> Result<SessionSummary, SessionError> {
        self.store
            .session(session_id)
            .ok_or_else(|| SessionError::NotFound(format!("会话不存在：{session_id}")))
    }

    /// 取 Run；不存在时返回未找到错误。
    fn require_run(&self, run_id: &str) -> Result<Run, SessionError> {
        self.store
            .run(run_id)
            .ok_or_else(|| SessionError::NotFound(format!("Run 不存在：{run_id}")))
    }
}

/// 生成业务 ID，把底层错误收敛为服务内部错误。
fn next_id() -> Result<String, SessionError> {
    id::next_id().map_err(|error| SessionError::Internal(format!("生成 ID 失败：{error:#}")))
}
