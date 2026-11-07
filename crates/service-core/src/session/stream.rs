//! Mock 事件流生产方：把一段固定回复按增量定时投递给订阅方。
//!
//! P2 的替换点：本模块的 `spawn` 换成 Agent Runtime 的 token 流，`RunEventStream::recv`
//! 的接口保持不变，`lib-web` 的 SSE 编码不需要改。
//!
//! 清理规则（D-026）：消费方断开（流被 drop）→ 把仍在执行的 Run 落成 `CANCELLED`；
//! Run 被 `api/run/cancel` 取消 → 停止生产且不改状态（取消接口已经写过终态）。

use std::sync::Arc;
use std::time::Duration;

use lib_core::time;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep};
use types_core::session::{Message, MessageRole, Run, RunEvent, RunEventKind, RunStatus};

use crate::session::store::Store;

/// 助手回复按字符切分时单段的字符数（按字符而非字节切，避免拆坏多字节字符）。
const DELTA_CHARS: usize = 4;
/// 相邻两段增量之间的间隔。
const DELTA_INTERVAL: Duration = Duration::from_millis(120);
/// 空闲多久补一条心跳事件。
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
/// 事件通道容量：够放下若干增量，生产方不需要为了背压提前阻塞。
const CHANNEL_CAPACITY: usize = 32;

/// Run 事件流：生产方在独立任务里产出事件，消费方按需 `recv`。
pub struct RunEventStream {
    receiver: mpsc::Receiver<RunEvent>,
}

impl RunEventStream {
    /// 取下一个事件；生产方结束或消费方已断开时返回 `None`。
    pub async fn recv(&mut self) -> Option<RunEvent> {
        self.receiver.recv().await
    }
}

/// 启动 Mock 事件流的生产任务。
///
/// `assistant_message_id` 由调用方在订阅时预分配，`run_started` 就能带上最终消息 ID。
pub(crate) fn spawn(store: Arc<Store>, run: Run, prompt: String, assistant_message_id: String) -> RunEventStream {
    let (sender, receiver) = mpsc::channel(CHANNEL_CAPACITY);
    tokio::spawn(produce(store, run, prompt, assistant_message_id, sender));
    RunEventStream { receiver }
}

/// 生产任务的结局。
enum Outcome {
    /// 事件流正常结束，Run 已落成 `COMPLETED`。
    Completed,
    /// 消费方断开，Run 已由本任务落成 `CANCELLED`。
    ClientGone,
    /// Run 已不在 `RUNNING`（例如被取消），停止生产且不改状态。
    RunStopped,
}

/// 生产任务主流程：只在消费方断开时补一次清理。
async fn produce(
    store: Arc<Store>,
    run: Run,
    prompt: String,
    assistant_message_id: String,
    sender: mpsc::Sender<RunEvent>,
) {
    let outcome = emit(&store, &run, &prompt, &assistant_message_id, &sender).await;
    if matches!(outcome, Outcome::ClientGone) {
        mark_cancelled(&store, &run.id);
    }
}

/// 产出 `run_started` → 若干 `message_delta` → `run_completed`。
async fn emit(
    store: &Store,
    run: &Run,
    prompt: &str,
    assistant_message_id: &str,
    sender: &mpsc::Sender<RunEvent>,
) -> Outcome {
    let input_tokens = char_count(prompt);
    let mut producer = Producer { sender, run, seq: 0 };

    // Run 在订阅时已被推进到 RUNNING，这里只把状态告知客户端。
    let mut started = producer.event(RunEventKind::RunStarted);
    started.status = Some(RunStatus::Running);
    started.message_id = Some(assistant_message_id.to_string());
    started.input_tokens = Some(input_tokens);
    if !producer.send(started).await {
        return Outcome::ClientGone;
    }

    let mut content = String::new();
    for chunk in deltas(&mock_reply(prompt)) {
        // 每段之前确认 Run 仍在执行：被取消时立即停止，不再往客户端推事件。
        if !matches!(current_status(store, run), Some(RunStatus::Running)) {
            return Outcome::RunStopped;
        }
        if !producer.wait(DELTA_INTERVAL).await {
            return Outcome::ClientGone;
        }
        content.push_str(&chunk);
        let mut delta = producer.event(RunEventKind::MessageDelta);
        delta.delta = Some(chunk);
        if !producer.send(delta).await {
            return Outcome::ClientGone;
        }
    }

    // 先落库再发事件：客户端收到 `run_completed` 后查 `api/run/get` 即可看到终态。
    let output_tokens = char_count(&content);
    let now_millis = time::now_millis();
    let message = Message {
        id: assistant_message_id.to_string(),
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

    let mut completed = producer.event(RunEventKind::RunCompleted);
    completed.status = Some(RunStatus::Completed);
    completed.message_id = Some(assistant_message_id.to_string());
    completed.input_tokens = Some(input_tokens);
    completed.output_tokens = Some(output_tokens);
    // 客户端可能已经断开：Run 已是终态，这里失败也无需回滚。
    let _ = producer.send(completed).await;
    Outcome::Completed
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

    /// 发送事件；返回 `false` 表示消费方已断开。
    async fn send(&self, event: RunEvent) -> bool {
        self.sender.send(event).await.is_ok()
    }

    /// 等待下一段增量，期间空闲满心跳间隔则补一条心跳；返回 `false` 表示消费方已断开。
    async fn wait(&mut self, delay: Duration) -> bool {
        let deadline = Instant::now() + delay;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            if remaining.is_zero() {
                return true;
            }
            let wait = remaining.min(HEARTBEAT_INTERVAL);
            tokio::select! {
                () = sleep(wait) => {}
                () = self.sender.closed() => return false,
            }
            // 只睡了一个心跳间隔还没到截止时间，说明这段等待足够长，补一条心跳。
            if remaining > wait {
                let heartbeat = self.event(RunEventKind::Heartbeat);
                if !self.send(heartbeat).await {
                    return false;
                }
            }
        }
        true
    }
}

/// 取 Run 的当前状态；Run 消失时返回 `None`。
fn current_status(store: &Store, run: &Run) -> Option<RunStatus> {
    store.run(&run.id).map(|run| run.status)
}

/// 消费方断开：把仍在执行的 Run 落成 `CANCELLED`，避免留下永远 `RUNNING` 的记录。
fn mark_cancelled(store: &Store, run_id: &str) {
    let now_millis = time::now_millis();
    store.update_run(run_id, |run| {
        if matches!(run.status, RunStatus::Queued | RunStatus::Running) {
            run.status = RunStatus::Cancelled;
            run.updated_at_ms = now_millis;
            run.finished_at_ms = Some(now_millis);
        }
    });
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
