//! 事件生产者：统一维护 `seq` 并填充每条事件的公共字段。
//!
//! 与 P1.1 的 Mock 生产方同一形态：事件写进运行时会话的生产方，由写者先落盘再转发。
//! `seq` 只是占位，写者会覆写成文件内序号（`manager-agent::runtime::journal`）；
//! 这里保留自增，是为了让「同一 Run 的事件顺序」在内存里也是可读的。

use agent_core::session::{Run, RunEvent, RunEventKind};
use lib_core::time;
use tokio::sync::mpsc;

/// 单个 Run 的事件生产者：绑定该 Run 与生产方写入端，顺序发号。
pub(crate) struct Feed<'a> {
    /// 生产方写入端：发送失败意味着运行时会话已停止。
    sender: &'a mpsc::Sender<RunEvent>,
    /// 本批次事件所属的 Run。
    run: &'a Run,
    /// 下一个事件序号。
    seq: i64,
}

impl<'a> Feed<'a> {
    /// 绑定 Run 与生产方写入端。
    pub(crate) fn new(run: &'a Run, sender: &'a mpsc::Sender<RunEvent>) -> Self {
        Self { sender, run, seq: 0 }
    }

    /// 构造一条只填公共字段的事件，调用方按事件类型补充载荷。
    pub(crate) fn event(&mut self, kind: RunEventKind) -> RunEvent {
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
            tool_call_id: None,
            tool_name: None,
            tool_arguments: None,
            tool_status: None,
            tool_duration_ms: None,
            tool_round: None,
        }
    }

    /// 写入一条事件；返回 `false` 表示运行时会话的写者已退出。
    pub(crate) async fn send(&self, event: RunEvent) -> bool {
        self.sender.send(event).await.is_ok()
    }
}
