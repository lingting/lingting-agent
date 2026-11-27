//! TUI 与工作线程之间的事件契约。
//!
//! 上行 [`Update`] 是工作线程推给界面的结果，下行 [`Command`] 是界面交给工作线程的请求。
//! 两条通道都无界：界面 `try_recv` 永不阻塞事件循环，工作线程的异步任务也不会因为界面
//! 短暂繁忙而被卡住。

use lib_bin_cli::sdk::{ClientDecision, Message, SessionSummary, SkillAction, SkillDetailVO, SkillVO, ToolCallStatus};

/// 工作线程推给界面的事件。
#[derive(Debug)]
pub enum Update {
    /// 探活成功。
    Online { version: String },
    /// 探活失败：服务未启动、地址不可达或服务异常。
    Offline { message: String },
    /// 刷新后的会话列表。
    Sessions(Vec<SessionSummary>),
    /// 新建的会话。
    SessionCreated(SessionSummary),
    /// 指定会话的历史消息。
    Messages { session_id: String, messages: Vec<Message> },
    /// 用户消息已写入，对应的 Run 已创建。
    Sent {
        session_id: String,
        message: Message,
        run_id: String,
    },
    /// Run 开始执行，助手消息的位置可以占位了。
    RunStarted,
    /// 助手回复的增量文本。
    Delta(String),
    /// 一次工具调用开始执行（参数已脱敏，界面只展示工具名）。
    ToolStarted { tool_call_id: String, name: String },
    /// 一次工具调用结束：状态、耗时与失败文本都在这一条上。
    ToolFinished {
        tool_call_id: String,
        name: String,
        status: ToolCallStatus,
        duration_ms: i64,
        error: Option<String>,
    },
    /// 需要用户确认一次工具调用（D-P3-026）：Run 已挂起，等 `y` / `n`。
    ToolConfirmRequired {
        tool_call_id: String,
        name: String,
        arguments: String,
    },
    /// Run 正常结束，助手消息已落库。
    Completed,
    /// Run 执行失败。
    Failed { reason: String },
    /// 取消请求已受理。
    Cancelled,
    /// 事件流中断：连接断开、服务关闭或帧无法解析。
    Interrupted { reason: String },
    /// 事件流结束但没有终态事件：取消后服务端就是这样收尾的。
    StreamEnded,
    /// 技能清单快照：面板打开与每次动作 / 重载之后都用整张清单替换。
    Skills(Vec<SkillVO>),
    /// 一条技能的详情（正文与引用清单）。
    SkillDetail(SkillDetailVO),
    /// 一行提示，通常是某次调用的错误。
    Notice { text: String },
}

/// 界面交给工作线程的请求。
#[derive(Debug, Clone)]
pub enum Command {
    /// 探活、刷新会话列表，并按需重新拉取指定会话的消息。
    Refresh { session: Option<String> },
    /// 新建会话。
    CreateSession,
    /// 打开会话并拉取历史消息。
    OpenSession { session_id: String },
    /// 发送消息；`session_id` 为空时先建会话再发。
    Send {
        session_id: Option<String>,
        content: String,
    },
    /// 取消正在执行的 Run。
    Cancel { run_id: String },
    /// 应答一次工具确认：上行 `api/client/post { kind = TOOL_CONFIRM }`。
    Confirm {
        tool_call_id: String,
        decision: ClientDecision,
    },
    /// 空闲期的连通性巡检：只在失败时报 `Offline`，成功不产生更新。
    Probe,
    /// 拉取技能清单：面板每次打开都重新拉一次（文件系统仍是唯一事实源）。
    SkillList,
    /// 启停或重载技能：成功时返回动作完成后的整张清单。
    SkillUpdate { name: Option<String>, action: SkillAction },
    /// 取一条技能的详情（正文与引用清单）。
    SkillDetail { name: String },
}
