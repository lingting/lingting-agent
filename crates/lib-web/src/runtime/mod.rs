//! 运行时接口：客户端注册与下行事件流（P1.1）。
//!
//! Handler 只做“参数解包 + 业务调用 + 错误映射”，业务规则全部在 `service_core::runtime`；
//! 帧编码集中在 [`stream`]，错误映射集中在本文件（D-P1.1-005 的收敛点）。

pub mod api;
mod stream;

use framework_web::WebError;
use service_core::runtime::{RuntimeError, RuntimeState};

/// 「客户端未注册」的固定 message（D-P1.1-005）：客户端按 message 识别并重新注册。
const MESSAGE_CLIENT_NOT_REGISTERED: &str = "客户端未注册，请重新连接";

/// 运行时错误的统一出口：框架支持自定义 code 后只改这里。
fn runtime_error(error: RuntimeError) -> anyhow::Error {
    match error {
        RuntimeError::ClientNotRegistered => WebError::parameter(MESSAGE_CLIENT_NOT_REGISTERED, error).into(),
    }
}

/// 取已注入的运行时状态；缺失说明启动装配不完整，属服务端内部错误。
fn state() -> anyhow::Result<&'static RuntimeState> {
    service_core::runtime::state().ok_or_else(|| WebError::message("运行时状态未注入").into())
}
