//! Web Router 组装：收集 `web_api_*` 注册到 `inventory` 的路由。

use framework_web::{WebRoute, web_api_iter};

/// 收集当前二进制已注册的全部 Web 路由。
///
/// 除供路由组装与诊断使用外，本函数同时是本 crate 的**链接锚点**：`inventory` 注册项
/// 只有在 `lib-web` 被实际引用时才会进入最终二进制，否则会被链接器丢弃，
/// 表现为“接口定义了但全部 404”。服务启动与 SDK 生成器都需要显式引用本函数。
pub fn web_routes() -> Vec<WebRoute> {
    web_api_iter().collect()
}
