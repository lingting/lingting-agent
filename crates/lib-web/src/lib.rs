//! 服务端 HTTP API：路由、Handler、SSE 端点与 API 元数据收集。

pub mod error;
pub mod probe;
mod router;
pub mod session;

pub use error::ApiError;
pub use router::web_routes;
