//! 会话接口的入参契约：请求体用 `*PO`，查询参数用 `*Params`（见 `docs/develop.md`）。
//!
//! 查询参数只放字符串字段：`Query<T>` 的值全部来自查询字符串，数值与布尔需要额外适配（D-021）。

use framework_proc_auto::auto_type;

/// 创建会话的请求体。
#[auto_type]
pub struct SessionCreatePO {
    /// 会话标题；缺省时由服务端生成。
    pub title: Option<String>,
    /// 模型标识；缺省时使用服务端默认模型。
    pub model: Option<String>,
}

/// 获取会话消息的查询参数。
#[auto_type]
pub struct MessageListParams {
    /// 目标会话 ID。
    pub session_id: String,
}

/// 发送消息的请求体：写入一条用户消息并创建对应的 Run。
#[auto_type]
pub struct MessageSendPO {
    /// 目标会话 ID。
    pub session_id: String,
    /// 用户消息正文。
    pub content: String,
}

/// 查询 Run 的查询参数。
#[auto_type]
pub struct RunGetParams {
    /// 目标 Run ID。
    pub run_id: String,
}

/// 取消 Run 的请求体。
#[auto_type]
pub struct RunCancelPO {
    /// 目标 Run ID。
    pub run_id: String,
}
