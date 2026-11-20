//! Mock 事件生产方：把一个 Run 的完整事件写进运行时会话（RT-015、RT-018）。
//!
//! 生产方不再直接对接 SSE 响应：事件统一交给运行时会话的写者，由它先落盘再转发到
//! `api/client/stream`（D-P1.1-001）。生产方只关心「产出一条事件」与「Run 是否还能继续」，
//! 消费方断开由断连处理链负责，不在这里感知。
//!
//! P2 的替换点：本模块的 `produce` 换成 Agent Runtime 的 token 流，签名与调用方不变。

use std::time::Duration;

use lib_core::lifecycle;
use lib_core::time;
use tokio::sync::mpsc;
use tokio::time::sleep;
use types_core::session::{Message, MessageRole, Run, RunEvent, RunEventKind, RunStatus};

use crate::session::store::Store;

/// 助手回复按字符切分时单段的字符数（按字符而非字节切，避免拆坏多字节字符）。
const DELTA_CHARS: usize = 4;
/// 相邻两段增量之间的间隔。
const DELTA_INTERVAL: Duration = Duration::from_millis(120);
/// 服务关闭时终止事件里的原因文本，对外可见且不含内部细节。
const SHUTDOWN_REASON: &str = "服务正在关闭，事件流已终止";

/// 一个 Run 的生产结局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// 正常结束，Run 已落成 `COMPLETED`。
    Completed,
    /// 运行时会话已停止（写者退出）：生产自动结束。
    SessionGone,
    /// Run 已不在 `RUNNING`（例如被 `api/run/cancel` 取消），停止生产且不改状态。
    RunStopped,
    /// 服务进入关闭流程：Run 已落成 `CANCELLED` 并推送了终止事件。
    Shutdown,
}

/// 产出 `run_started` → 若干 `message_delta` → `run_completed`；
/// 中途被取消、会话停止或服务关闭时以对应结局提前收尾。
pub(crate) async fn produce(
    store: &Store,
    run: Run,
    prompt: String,
    assistant_message_id: String,
    producer: &mpsc::Sender<RunEvent>,
) -> Outcome {
    let input_tokens = char_count(&prompt);
    let mut events = Producer {
        sender: producer,
        run: &run,
        seq: 0,
    };

    // Run 在触发点已被推进到 RUNNING，这里只把状态告知消费方。
    let mut started = events.event(RunEventKind::RunStarted);
    started.status = Some(RunStatus::Running);
    started.message_id = Some(assistant_message_id.clone());
    started.input_tokens = Some(input_tokens);
    if !events.send(started).await {
        return Outcome::SessionGone;
    }

    let mut content = String::new();
    for chunk in deltas(&mock_reply(&prompt)) {
        // 服务关闭时先收尾：Run 落成 CANCELLED 并推一条终止事件，避免文件缺终态。
        if lifecycle::is_shutdown() {
            return shutdown(store, &run, &mut events).await;
        }
        // 每段之前确认 Run 仍在执行：被取消时立即停止，不再产出事件。
        if !matches!(current_status(store, &run), Some(RunStatus::Running)) {
            return Outcome::RunStopped;
        }
        if !events.wait(DELTA_INTERVAL).await {
            return Outcome::SessionGone;
        }
        content.push_str(&chunk);
        let mut delta = events.event(RunEventKind::MessageDelta);
        delta.delta = Some(chunk);
        if !events.send(delta).await {
            return Outcome::SessionGone;
        }
    }

    // 先落库再发事件：客户端收到 `run_completed` 后查 `api/run/get` 即可看到终态。
    let output_tokens = char_count(&content);
    let now_millis = time::now_millis();
    let message = Message {
        id: assistant_message_id.clone(),
        session_id: run.session_id.clone(),
        run_id: Some(run.id.clone()),
        role: MessageRole::Assistant,
        content,
        model: Some(run.model.clone()),
        input_tokens: Some(input_tokens),
        output_tokens: Some(output_tokens),
        created_at_ms: now_millis,
    };
    store.complete_run(&run.id, message, input_tokens, output_tokens, now_millis);

    let mut completed = events.event(RunEventKind::RunCompleted);
    completed.status = Some(RunStatus::Completed);
    completed.message_id = Some(assistant_message_id);
    completed.input_tokens = Some(input_tokens);
    completed.output_tokens = Some(output_tokens);
    // Run 已是终态：会话此刻停止也不影响记录，发送失败不算错误。
    let _ = events.send(completed).await;
    Outcome::Completed
}

/// 服务关闭收尾：把 Run 落成 `CANCELLED`，并补一条 `server_shutdown` 终止事件。
///
/// 事件只有一次机会：进程即将退出，发送失败也不再重试。
async fn shutdown(store: &Store, run: &Run, events: &mut Producer<'_>) -> Outcome {
    mark_cancelled(store, &run.id);
    let mut ended = events.event(RunEventKind::ServerShutdown);
    ended.status = Some(RunStatus::Cancelled);
    ended.error = Some(SHUTDOWN_REASON.to_string());
    let _ = events.send(ended).await;
    Outcome::Shutdown
}

/// 事件生产者：统一维护序号并填充每条事件都有的公共字段。
struct Producer<'a> {
    sender: &'a mpsc::Sender<RunEvent>,
    run: &'a Run,
    seq: i64,
}

impl Producer<'_> {
    /// 构造一条只填公共字段的事件，调用方按事件类型补充载荷。
    fn event(&mut self, kind: RunEventKind) -> RunEvent {
        let seq = self.seq;
        self.seq += 1;
        RunEvent {
            event: kind,
            seq,
            run_id: self.run.id.clone(),
            session_id: self.run.session_id.clone(),
            created_at_ms: time::now_millis(),
            delta: None,
            message_id: None,
            status: None,
            error: None,
            input_tokens: None,
            output_tokens: None,
        }
    }

    /// 写入一条事件；返回 `false` 表示运行时会话的写者已退出。
    async fn send(&self, event: RunEvent) -> bool {
        self.sender.send(event).await.is_ok()
    }

    /// 等待下一段增量；等待期间会话停止则提前返回 `false`。
    async fn wait(&self, delay: Duration) -> bool {
        tokio::select! {
            () = sleep(delay) => true,
            () = self.sender.closed() => false,
        }
    }
}

/// 取 Run 的当前状态；Run 消失时返回 `None`。
fn current_status(store: &Store, run: &Run) -> Option<RunStatus> {
    store.run(&run.id).map(|run| run.status)
}

/// 服务关闭：把仍在执行的 Run 落成 `CANCELLED`，避免留下永远 `RUNNING` 的记录。
fn mark_cancelled(store: &Store, run_id: &str) {
    store.cancel_run(run_id, time::now_millis());
}

/// Mock 回复：回显用户输入，并标注本段由 P1 的 Mock 产生。
fn mock_reply(prompt: &str) -> String {
    format!("收到你的消息：{prompt}\n本段由 P1 的 Mock 流式回复产生，P2 接入 Agent Runtime 后改由模型真实生成。")
}

/// 把回复按字符切成增量片段。
fn deltas(text: &str) -> Vec<String> {
    text.chars()
        .collect::<Vec<_>>()
        .chunks(DELTA_CHARS)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

/// Mock 的 token 用量：按字符数估算。
fn char_count(text: &str) -> i64 {
    text.chars().count() as i64
}
