//! 进程内共享状态持有者。
//!
//! P1 的 Mock 存储需要跨调用共享，而框架的 Handler 不接收服务实例（`web_api` 只支持
//! `Json<T>` / `Query<T>` 参数），因此这里用进程级单例持有服务实例。
//! P2 接入真实 Repository 后由 `lib-bin-web` 在启动时构造并注入，本模块随之移除。

use crate::session::SessionService;
use std::sync::LazyLock;

static SESSION: LazyLock<SessionService> = LazyLock::new(SessionService::new);

/// 取会话业务服务实例。
pub fn session() -> &'static SessionService {
    &SESSION
}
