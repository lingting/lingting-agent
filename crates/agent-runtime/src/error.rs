//! 对外失败文案的唯一来源：Run 落 `FAILED` 时写进 `runs.error` 与 `run_failed` 的就是这里。
//!
//! 文案固定、不含内部细节：原始错误只进日志（D-P2-017）。这些字符串都要能被
//! `bin-tests` 的 `run.failed_*` 用例逐字断言，因此集中在一处，不在调用点各写一份。

/// 排队超时：等待全局并发许可超过 `agent.run_queue_timeout_ms`（P2-025）。
pub(crate) const QUEUE_TIMEOUT: &str = "执行排队超时";
/// 读取会话数据失败：细节只进日志，对外只说明「读不到」。
pub(crate) const STORAGE: &str = "读取会话数据失败";
/// 取消时的兜底原因：正常路径下取消方先写库，这里只在数据库里没有原因文本时使用。
pub(crate) const CANCEL_FALLBACK: &str = "执行已取消";
