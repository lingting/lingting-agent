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
/// 工具轮次用尽：已经达到 `[agent] max_tool_rounds` 而模型仍在请求工具调用（P3-014）。
pub(crate) const TOOL_ROUNDS_EXCEEDED: &str = "工具调用轮次超过上限";
/// 工具轨迹落库失败：本轮 assistant 调用与 TOOL 结果没有提交（D-P3-024）。
pub(crate) const TOOL_TRACE_STORAGE: &str = "工具轨迹落库失败";
/// 工具轨迹不完整：历史里出现多处未配对的调用或孤儿结果（D-P3-023）。
pub(crate) const TOOL_TRACE_BROKEN: &str = "工具轨迹不完整";
/// 检查点写入失败：可恢复性无法证明，Run 立即落 `FAILED`，不降级继续（D-P6-010）。
pub(crate) const CHECKPOINT: &str = "检查点写入失败";
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
