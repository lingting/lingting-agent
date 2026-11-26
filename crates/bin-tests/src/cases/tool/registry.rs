//! 注册表运行期可变与三级权限判定（P4-003）。
//!
//! 纯进程内：注册表本身就是内存结构，直接用内置工具当样本（它们的风险等级与自述是常量），
//! 不需要服务端、不需要 Mock，也不读日志。断言覆盖 `tool` 域的三条口径：
//!
//! - 注册 / 注销立刻反映在 `model_definitions()`，顺序只由名字字典序决定；
//! - 权限判定 `[tools.permissions]` 显式值 > 条目默认 > `ToolRisk` 缺省；
//! - `deny` 的工具仍留在表里（执行期要能查到它并回 `DENIED`）。

use std::path::PathBuf;
use std::sync::Arc;

use agent_tool::builtin::{Calculator, CurrentTime, FileRead, HttpRequestTool};
use agent_tool::{RegistryError, Tool, ToolPermission, ToolPolicy, ToolRegistry, ToolRisk};
use anyhow::ensure;

use super::case;
use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;

/// 文件类工具的白名单上限（用例不读文件，只要构造出来）。
const FILE_MAX_BYTES: u64 = 64;
/// HTTP 工具的白名单与上限（用例不发请求，只要构造出来）。
const HTTP_MAX_BYTES: u64 = 64;

/// 注册表用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("tool.registry", CaseKind::Shared, registry)]
}

/// 一个「只读」样本：`ReadOnly` 风险，缺省权限是允许。
fn file_read() -> Arc<dyn Tool> {
    Arc::new(FileRead::new(vec![PathBuf::from(".")], FILE_MAX_BYTES))
}

/// 一个「触网」样本：`Network` 风险，缺省权限是拒绝。
fn http_request() -> Arc<dyn Tool> {
    Arc::new(HttpRequestTool::new(Vec::new(), HTTP_MAX_BYTES))
}

/// 清单里的名字，按注册表给出的顺序。
fn names(registry: &ToolRegistry) -> Vec<String> {
    registry
        .model_definitions()
        .into_iter()
        .map(|definition| definition.name)
        .collect()
}

fn registry(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();

        // 一、注册顺序不影响清单顺序：清单只按名字字典序。
        let table = ToolRegistry::new(ToolPolicy::empty());
        table.register(Arc::new(CurrentTime))?;
        table.register(Arc::new(Calculator))?;
        ensure!(
            names(&table) == ["calculator", "current_time"],
            "清单顺序应为名字字典序：{:?}",
            names(&table)
        );
        ensure!(
            table.names() == ["calculator", "current_time"],
            "`names()` 应为名字字典序：{:?}",
            table.names()
        );
        notes.push(format!("注册顺序无关，清单顺序：{:?}", names(&table)));

        // 二、注销立刻反映在清单与查找上；重复注册被拒。
        ensure!(
            matches!(
                table.register(Arc::new(Calculator)),
                Err(RegistryError::DuplicateName(_))
            ),
            "同名工具重复注册应被拒绝"
        );
        let removed = table.unregister("calculator");
        ensure!(
            removed.as_ref().map(|tool| tool.name().to_owned()).as_deref() == Some("calculator"),
            "注销应返回被移除的工具"
        );
        ensure!(
            table.lookup("calculator").is_none() && names(&table) == ["current_time"],
            "注销后清单与查找都应移除该工具"
        );
        ensure!(table.len() == 1 && !table.is_empty(), "注销后应只剩一个工具");
        notes.push("注销立刻反映在 `model_definitions` / `lookup` / `names`".to_owned());

        // 三、条目默认权限来自注册点（MCP 的 server 级权限走这条路径）。
        let by_entry = ToolRegistry::new(ToolPolicy::empty());
        // `FileRead` 的 `ReadOnly` 缺省是允许，条目默认改成「需确认」后应盖过风险缺省。
        by_entry.register_with_default(file_read(), Some(ToolPermission::Confirm))?;
        ensure!(
            by_entry.permission_of("file_read") == Some(ToolPermission::Confirm),
            "条目默认 `confirm` 应盖过风险缺省 `allow`：{:?}",
            by_entry.permission_of("file_read")
        );
        ensure!(names(&by_entry) == ["file_read"], "`confirm` 的工具仍应进模型可见清单");
        // 条目默认 `deny`：不进清单，但仍在表里（执行期回 `DENIED`）。
        by_entry.register_with_default(Arc::new(Calculator), Some(ToolPermission::Deny))?;
        ensure!(
            by_entry.permission_of("calculator") == Some(ToolPermission::Deny),
            "条目默认 `deny` 应生效"
        );
        ensure!(
            !names(&by_entry).contains(&"calculator".to_owned()),
            "`deny` 的工具不应进模型可见清单"
        );
        ensure!(
            by_entry.lookup("calculator").is_some(),
            "`deny` 的工具仍应能查到（执行期二次判定）"
        );
        notes.push("条目默认：`confirm` 盖过风险缺省、`deny` 摘出清单但保留在表里".to_owned());

        // 四、显式配置永远优先于条目默认与风险缺省。
        let by_policy = ToolRegistry::new(ToolPolicy::new([
            ("file_read".to_owned(), ToolPermission::Deny),
            ("calculator".to_owned(), ToolPermission::Allow),
        ]));
        by_policy.register_with_default(file_read(), Some(ToolPermission::Allow))?;
        by_policy.register(Arc::new(Calculator))?;
        ensure!(
            by_policy.permission_of("file_read") == Some(ToolPermission::Deny),
            "显式 `deny` 应盖过条目 `allow`"
        );
        ensure!(
            by_policy.permission_of("calculator") == Some(ToolPermission::Allow),
            "显式 `allow` 应盖过风险缺省"
        );
        notes.push("显式 `[tools.permissions]` 优先于条目默认与风险缺省".to_owned());

        // 五、没有条目默认时行为与 P3 完全一致（只看风险等级）。
        let by_risk = ToolRegistry::new(ToolPolicy::empty());
        by_risk.register(file_read())?;
        by_risk.register(http_request())?;
        ensure!(
            by_risk.permission_of("file_read") == Some(ToolPermission::Allow)
                && by_risk.permission_of("http_request") == Some(ToolPermission::Deny),
            "无条目默认时应按风险等级判定：ReadOnly → 允许、Network → 拒绝"
        );
        ensure!(
            names(&by_risk) == ["file_read"],
            "缺省配置下 `http_request` 不应进清单：{:?}",
            names(&by_risk)
        );
        ensure!(
            by_risk.lookup("http_request").map(|tool| tool.risk()) == Some(ToolRisk::Network),
            "`http_request` 仍是 `Network` 风险"
        );
        ensure!(
            by_risk.permission_of("nope").is_none(),
            "未注册的工具没有权限结论（调用方按拒绝处理）"
        );
        notes.push("无条目默认：按 `ToolRisk` 判定，未注册返回 None".to_owned());

        Ok(notes)
    })
}
