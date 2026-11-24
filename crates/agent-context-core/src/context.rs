//! 组装结果：有序 Item 集合与只进日志的 metadata。

use crate::item::ContextItem;

/// 一次请求的完整 Context。
///
/// Item 顺序即发送顺序：系统提示词（存在时）在最前，历史按 `seq` 升序居中，本次用户输入在最后。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// 按发送顺序排列的 Item。
    items: Vec<ContextItem>,
    /// 组装过程的可观测信息。
    metadata: ContextMetadata,
}

impl Context {
    /// 用已排好序的 Item 与 metadata 构造。
    pub fn new(items: Vec<ContextItem>, metadata: ContextMetadata) -> Self {
        Self { items, metadata }
    }

    /// 按发送顺序取 Item。
    pub fn items(&self) -> &[ContextItem] {
        &self.items
    }

    /// 组装过程的可观测信息。
    pub fn metadata(&self) -> &ContextMetadata {
        &self.metadata
    }

    /// 取出 Item 集合；调用方转成 Provider 侧的请求体。
    pub fn into_items(self) -> Vec<ContextItem> {
        self.items
    }
}

/// Context 组装过程的可观测信息。
///
/// 只进日志，不进 wire、不进数据库（D-P2-013 第 5 条）：裁剪条数与估算 token 用于排查
/// 「为什么这段历史没进请求」，不需要对外可见。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMetadata {
    /// 本次组装的会话 ID。
    pub session_id: String,
    /// 本次执行使用的模型标识。
    pub model: String,
    /// 因条数上限或预算被裁掉的历史**消息**条数。
    ///
    /// 裁剪以「工具调用组」为单位（D-P3-025），被整组丢弃时组内每条消息都计入这里。
    pub trimmed_messages: usize,
    /// 全部 Item 的估算 token 数。
    pub estimated_tokens: usize,
}
