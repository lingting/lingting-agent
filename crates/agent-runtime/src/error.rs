//! 对外失败文案的唯一来源：Run 落 `FAILED` 时写进 `runs.error` 与 `run_failed` 的就是这里。
//!
//! 文案固定、不含内部细节：原始错误只进日志（D-P2-017）。这些字符串都要能被
//! `bin-tests` 的 `run.failed_*` 用例逐字断言，因此集中在一处，不在调用点各写一份。

use agent_route::LlmError;

/// 排队超时：等待全局并发许可超过 `agent.run_queue_timeout_ms`（P2-025）。
pub(crate) const QUEUE_TIMEOUT: &str = "执行排队超时";
/// 读取会话数据失败：细节只进日志，对外只说明「读不到」。
pub(crate) const STORAGE: &str = "读取会话数据失败";
/// 取消时的兜底原因：正常路径下取消方先写库，这里只在数据库里没有原因文本时使用。
pub(crate) const CANCEL_FALLBACK: &str = "执行已取消";
/// 工具轮次用尽：已经达到 `[agent] max_tool_rounds` 而模型仍在请求工具调用（P3-014）。
pub(crate) const TOOL_ROUNDS_EXCEEDED: &str = "工具调用轮次超过上限";
/// 工具轨迹落库失败：本轮 assistant 调用与 TOOL 结果没有提交（D-P3-024）。
pub(crate) const TOOL_TRACE_STORAGE: &str = "工具轨迹落库失败";
/// 工具轨迹不完整：历史里出现多处未配对的调用或孤儿结果（D-P3-023）。
pub(crate) const TOOL_TRACE_BROKEN: &str = "工具轨迹不完整";
/// 检查点写入失败：可恢复性无法证明，Run 立即落 `FAILED`，不降级继续（D-P6-010）。
pub(crate) const CHECKPOINT: &str = "检查点写入失败";
/// Run 级重试用尽：可重试分类的失败耗尽 `[agent] run_max_retries` 之后的固定原因（D-P6-012）。
///
/// 只有真正重试过（至少一次）才会出现这条文案；`run_max_retries = 0` 时沿用原始脱敏文案，
/// 因此既有失败口径逐字段不变。
pub(crate) const RETRY_EXHAUSTED: &str = "模型调用重试次数已用尽";
/// 检查点格式版本高于当前代码支持：判「不可恢复」，不猜测也不降级成从头执行（D-P6-003）。
pub(crate) const CHECKPOINT_UNSUPPORTED: &str = "检查点格式版本不受支持，Run 不可恢复";
/// 检查点内容无法解释（版本为 0、缺字段或与列不一致）：判「不可恢复」（D-P6-003）。
pub(crate) const CHECKPOINT_CORRUPT: &str = "检查点内容不完整或已损坏，Run 不可恢复";
/// 恢复一致性校验①：序号不连续，或 `runs.last_checkpoint_seq` 指针与最近检查点不一致。
pub(crate) const RESUME_GAP: &str = "检查点序号不连续，Run 不可恢复";
/// 恢复一致性校验②：检查点记录的消息上界在库里找不到对应消息。
pub(crate) const RESUME_MESSAGES_LOST: &str = "检查点记录的消息缺失，Run 不可恢复";
/// 恢复一致性校验③：最后一个工具组之外缺结果，或出现孤儿结果（配对规则不成立）。
pub(crate) const RESUME_TRACE_BROKEN: &str = "工具轨迹与检查点不一致，Run 不可恢复";
/// 恢复一致性校验④：`PENDING` 开始标记出现在更早的工具组。
pub(crate) const RESUME_PENDING_LEAK: &str = "工具开始标记出现在非最后的工具组，Run 不可恢复";
/// 恢复一致性校验⑤：检查点阶段记录的轮次与已提交的工具轨迹不一致。
pub(crate) const RESUME_PHASE_MISMATCH: &str = "检查点阶段与工具轨迹不一致，Run 不可恢复";

/// 中断通知的固定前缀：回灌给模型的文本只有「前缀 + 白名单简短原因」（D-P6-007）。
pub(crate) const INTERRUPTED_NOTICE: &str = "上次执行被中断";
/// 简短原因白名单（D-P6-007）：库里的 `interrupted_reason` → 对外可见的简短原因。
///
/// 新增中断来源时必须在这里登记；未登记的一律落到 [`INTERRUPTED_UNKNOWN`]，
/// 不把库里的原始文本直接拼进通知（它可能含环境细节，且长度不可控）。
const INTERRUPTED_REASONS: [(&str, &str); 1] = [("服务重启", "服务重启")];
/// 库里没有中断原因（旧库行）或原因不在白名单时的兜底文案。
const INTERRUPTED_UNKNOWN: &str = "原因未知";

/// 中断通知文本：恢复时回灌给模型的固定文本，也是恢复后首个 `run_started` 的原因（D-P6-007）。
///
/// 不含调用参数原文、不含绝对路径、不含日志片段：库里的原因只经白名单映射。
pub(crate) fn interrupted_notice(stored: Option<&str>) -> String {
    format!("{INTERRUPTED_NOTICE}：{}", interrupted_reason_of(stored))
}

/// 库里的中断原因 → 简短原因：只认白名单前缀，其余一律兜底。
pub(crate) fn interrupted_reason_of(stored: Option<&str>) -> &'static str {
    let Some(stored) = stored else {
        return INTERRUPTED_UNKNOWN;
    };
    INTERRUPTED_REASONS
        .iter()
        .find(|(key, _)| stored.starts_with(key))
        .map_or(INTERRUPTED_UNKNOWN, |(_, short)| *short)
}

/// 失败分类（D-P6-012 第 3 条）：**分类表只在这里出现一次**，调用点不再各自判断。
///
/// 分类与固定文案的对应关系（文案来源逐条点名，不在调用点拼字符串）：
///
/// | 分类 | 分支 | 固定文案 |
/// |---|---|---|
/// | 可重试 | 上游瞬时错误 `LlmError::Upstream`（5xx、错误信封） | 重试用尽后落 [`RETRY_EXHAUSTED`]，未用尽前不外泄 |
/// | 可重试 | 传输类 `LlmError::Network` / `Truncated` / `IdleTimeout` | 同上 |
/// | 不可重试 | 认证 `LlmError::Authentication` | `LlmError::public_message()`（`agent-route` 是 Provider 层文案的唯一产点） |
/// | 不可重试 | 配置与请求缺陷 `UnknownModel` / `Unconfigured` / `InvalidRequest` | 同上 |
/// | 不可重试 | 协议不符 `LlmError::InvalidResponse` | 同上 |
/// | 不可重试 | 上下文超预算 | Context 组装返回的脱敏文案（`agent-context` 单一产点） |
/// | 不可重试 | 轮次用尽 / 轨迹损坏 | [`TOOL_ROUNDS_EXCEEDED`] / [`TOOL_TRACE_BROKEN`] |
/// | 不可重试 | 检查点不可用或写失败 | [`CHECKPOINT`] / [`CHECKPOINT_UNSUPPORTED`] / [`CHECKPOINT_CORRUPT`] / `RESUME_*` |
/// | 不可重试 | 取消 | 取消方写入的原因，缺省 [`CANCEL_FALLBACK`] |
///
/// 工具层的 `Tool::retryable` 与本函数无关：工具重试仍在 `ToolExecutor` 里按
/// `[tools] max_retries` 就地完成，Run 级重试不重复它的判断。
pub(crate) fn retryable(error: &LlmError) -> bool {
    matches!(
        error,
        LlmError::Upstream { .. }
            | LlmError::Network { .. }
            | LlmError::Truncated { .. }
            | LlmError::IdleTimeout { .. }
    )
}
