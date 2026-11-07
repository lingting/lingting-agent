//! 统一错误响应契约（P1-009，分配规则见 `decisions.md` D-024）。
//!
//! 服务端所有失败响应都是 `{"code": <HTTP 状态码>, "message": "<对外消息>"}`，由 `framework-web`
//! 统一生成（`WebResponse::from_error`）。这里定义其可被 SDK 导出的契约类型，
//! 供服务端与客户端按同一份定义解析错误响应体。

use framework_proc_auto::auto_type;

/// 统一错误响应体。
#[auto_type]
pub struct ApiError {
    /// 错误码：与 HTTP 状态码一致；4xx 为客户端错误，5xx 为服务端错误，已分配的取值只追加不复用。
    pub code: u32,
    /// 对外错误消息：内部错误与 panic 一律为固定文案，不含堆栈、文件路径等内部信息。
    pub message: String,
}
