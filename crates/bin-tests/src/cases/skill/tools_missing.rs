//! `skill.tools_missing`（P5-006 / P5-019）：声明的工具不可用时，索引与正文都明确标注。
//!
//! 证据两段：
//!
//! - 进程内：用装配层的 `build_skills` + 真实 `ToolRegistry` 装配一次，核对索引行后缀、`skill_load`
//!   结果里的「缺少的工具」小节与那句约束；权限 `deny` 与运行期注册新工具都在下一次渲染时生效，
//!   **不需要重载 Skill**；
//! - 请求级：私有服务端 + 私有 Mock 跑一轮，确认缺失的工具不在 `tools` 里、模型拿到的是
//!   「无法执行」的约束而不是「已执行」的表述。
//!
//! 数据来源是接口 / 契约返回值、`api/tool/audit/list`、客户端事件与 Mock 的 `/mock/last-request`；
//! 不读日志。

use std::path::Path;
use std::sync::Arc;

use agent_tool::builtin::{Calculator, CurrentTime};
use agent_tool::{ToolPermission, ToolPolicy, ToolRegistry};
use anyhow::{Context, Result, ensure};
use lib_bin_cli::sdk::RunEventKind;
use serde_json::json;
use types_core::config::SkillsConfig;

use super::{case, serve_with, system_messages, tool_context, tool_names, write_file};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::cases::tool::audit;
use crate::mock::llm::{MockServer, observe};
use crate::util::context::TestContext;
use crate::util::flow::Occupied;
use crate::util::paths::repo_root;

/// 示例 `db-migrate`：声明一个可用工具（`calculator`）与一个此刻不存在的工具（`sqlite3`）。
const DB_MIGRATE: &str = "db-migrate";
/// 第二个技能：只声明 `current_time`，用于「运行期注册新工具」的路径。
const NEEDS_TIME: &str = "---\nname: needs-time\ndescription: 需要当前时间\nrequires_tools: [current_time]\n---\n\n\
                          # 步骤\n\n1. 取当前时间。\n";
/// 缺失小节的标题与约束语（文案的唯一产点在 `agent-skill/src/availability.rs`）。
const MISSING_HEADING: &str = "## 缺少的工具";
const CONSTRAINT: &str = "涉及这些工具的步骤必须停止，并明确告知用户无法执行。";

/// 缺工具用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.tools_missing", CaseKind::SelfServed, tools_missing)]
}

/// 进程内两段 + 请求级一段。
fn tools_missing(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let home = crate::util::paths::skill_home("skill-tools-missing")?;
        copy_example(&home, DB_MIGRATE)?;
        write_file(&home.join("skills/needs-time/SKILL.md"), NEEDS_TIME)?;
        let mut notes = assembly_scope(&home).await?;
        notes.extend(config_scope(&home)?);

        let mock = MockServer::start().await?;
        copy_example(context.home(), DB_MIGRATE)?;
        notes.extend(serve_with(context, &mock, async { request_scope(context, &mock).await }).await?);
        Ok(notes)
    })
}

/// 一次装配：只注册 `calculator`（缺 `sqlite3` 与 `current_time`）。
fn assemble(home: &Path) -> Result<(Arc<ToolRegistry>, Arc<manager_agent::SkillManager>)> {
    let tools = Arc::new(ToolRegistry::new(ToolPolicy::empty()));
    tools
        .register(Arc::new(Calculator))
        .map_err(|error| anyhow::anyhow!("注册 calculator 失败：{error}"))?;
    let config = SkillsConfig::default();
    let manager = manager_agent::build_skills(&config, home, &tools)?;
    Ok((tools, manager))
}

/// 一、索引行后缀、`skill_load` 结果里的缺失小节与约束语。
async fn assembly_scope(home: &Path) -> Result<Vec<String>> {
    let (tools, manager) = assemble(home)?;
    let mut notes = Vec::new();

    let entries = manager.list();
    let db = entries
        .iter()
        .find(|entry| entry.name == DB_MIGRATE)
        .context("清单里应有 db-migrate")?;
    ensure!(
        db.requires_tools == ["calculator", "sqlite3"] && db.missing_tools == ["sqlite3"],
        "声明的工具与缺失的工具不符：{db:?}"
    );
    let time = entries
        .iter()
        .find(|entry| entry.name == "needs-time")
        .context("清单里应有 needs-time")?;
    ensure!(time.missing_tools == ["current_time"], "缺 current_time：{time:?}");

    let index = manager.source().context("启用状态下应注入索引来源")?.index();
    ensure!(
        index.section.contains("（缺少工具：sqlite3）") && index.section.contains("（缺少工具：current_time）"),
        "索引行应标注缺失的工具：{}",
        index.section
    );
    ensure!(
        !index.section.contains("calculator）"),
        "已注册的工具不应被算作缺失：{}",
        index.section
    );
    notes.push(format!(
        "索引：shown={} hidden={} chars={}",
        index.shown, index.hidden, index.chars
    ));

    // `skill_load` 的结果里：同一份缺失清单 + 那句约束。
    let tool = tools.lookup("skill_load").context("装配后应注册 skill_load")?;
    let (loaded, error) = match tool.call(&tool_context(home), json!({"name": DB_MIGRATE})).await {
        Ok(output) => (output.text, output.is_error),
        Err(error) => (error.to_string(), true),
    };
    ensure!(!error, "缺工具不应阻断加载：{loaded}");
    ensure!(
        loaded.contains(MISSING_HEADING) && loaded.contains("- sqlite3") && loaded.contains(CONSTRAINT),
        "加载结果应带缺失小节与约束语：{loaded}"
    );
    notes.push("skill_load：带「缺少的工具」小节与约束语".to_owned());
    Ok(notes)
}

/// 二、权限 `deny` 与运行期注册新工具都在下一次渲染时生效（不重载 Skill）。
fn config_scope(home: &Path) -> Result<Vec<String>> {
    let (tools, manager) = assemble(home)?;
    let source = manager.source().context("启用状态下应注入索引来源")?.clone();
    let mut notes = Vec::new();

    // 1) `deny` 也算不可用：把 calculator 换成 deny 之后，下一次渲染立刻把它算进缺失。
    tools.unregister("calculator");
    tools
        .register_with_default(Arc::new(Calculator), Some(ToolPermission::Deny))
        .map_err(|error| anyhow::anyhow!("重注册 calculator 失败：{error}"))?;
    let denied = source.index();
    ensure!(
        denied.section.contains("（缺少工具：calculator、sqlite3）"),
        "deny 的工具应算作缺失：{}",
        denied.section
    );

    // 2) 改回 allow：重新渲染即消失，Skill 没有重载过。
    tools.unregister("calculator");
    tools
        .register(Arc::new(Calculator))
        .map_err(|error| anyhow::anyhow!("恢复 calculator 失败：{error}"))?;
    let restored = source.index();
    ensure!(
        restored.section.contains("（缺少工具：sqlite3）") && !restored.section.contains("calculator、"),
        "改回 allow 后缺失标记应消失：{}",
        restored.section
    );
    notes.push("权限：deny → allow 的往返只影响下一次渲染".to_owned());

    // 3) 运行期注册新工具（MCP 连上的等价动作）：needs-time 的缺失标记自动消失。
    tools
        .register(Arc::new(CurrentTime))
        .map_err(|error| anyhow::anyhow!("注册 current_time 失败：{error}"))?;
    let registered = source.index();
    ensure!(
        !registered.section.contains("current_time"),
        "运行期注册的新工具应让缺失标记消失：{}",
        registered.section
    );
    let entries = manager.list();
    let time = entries
        .iter()
        .find(|entry| entry.name == "needs-time")
        .context("清单里应有 needs-time")?;
    ensure!(
        time.missing_tools.is_empty(),
        "清单里的缺失工具也应按现状判定：{time:?}"
    );
    ensure!(
        entries
            .iter()
            .any(|entry| entry.name == DB_MIGRATE && entry.missing_tools == ["sqlite3"]),
        "其余 Skill 的缺失标记应保持不变"
    );
    notes.push("运行期注册：current_time 的缺失标记下一次渲染即消失".to_owned());
    Ok(notes)
}

/// 三、请求级：缺失的工具不在 `tools` 里，模型拿到的表述是「无法执行」。
async fn request_scope(context: &TestContext, mock: &MockServer) -> Result<Vec<String>> {
    let mut occupied = Occupied::start(context, "skill.tools_missing").await?;
    let script = json!({"calls": [{"name": "skill_load", "arguments": {"name": DB_MIGRATE}}]});
    let (sent, drained) = occupied
        .say(
            context,
            &format!(
                "按技能做迁移（{}）
tool:{script}",
                context.token()
            ),
        )
        .await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCompleted,
        "缺工具不应让 Run FAILED：{:?}",
        drained.terminal.event
    );
    let records = audit(context, &sent.run.id).await?;
    ensure!(records.len() == 1, "剧本只有一次调用：{records:?}");
    let text = records[0].result.clone().unwrap_or_default();
    ensure!(
        text.contains(MISSING_HEADING) && text.contains("- sqlite3") && text.contains(CONSTRAINT),
        "模型收到的应是缺失清单与约束语：{text}"
    );

    let request = observe::last_request(mock).await?;
    let tools = tool_names(&request);
    ensure!(
        !tools.contains(&"sqlite3".to_owned()),
        "缺失的工具不应出现在请求的工具清单里：{tools:?}"
    );
    ensure!(
        tools.contains(&"skill_load".to_owned()) && tools.contains(&"calculator".to_owned()),
        "`skill_load` 与已注册的工具都应在工具清单里：{tools:?}"
    );
    let system = system_messages(&request);
    ensure!(
        system.len() == 1 && system[0].contains("（缺少工具：sqlite3）"),
        "索引应标注缺失的工具：{system:?}"
    );
    // 最终回复是「无法执行」的口径，而不是「已执行」。
    ensure!(
        !drained.content.contains("已执行"),
        "模型不应声称已执行缺失工具：{}",
        drained.content
    );
    ensure!(
        drained.content.contains("无法执行"),
        "最终回复应体现「无法执行」的约束：{}",
        drained.content
    );
    occupied.release(context).await?;
    Ok(vec![
        format!("请求级：tools={tools:?}（无 sqlite3）"),
        "最终回复：带「无法执行」的约束，没有「已执行」的表述".to_owned(),
    ])
}

/// 把示例 Skill 的文件复制进用例的应用根目录。
fn copy_example(home: &Path, directory: &str) -> Result<()> {
    let source = repo_root()?.join("crates/agent-skill/assets/skills").join(directory);
    for file in ["SKILL.md", "references/rollback.md"] {
        let path = source.join(file);
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("读取示例文件失败：{}", path.display()))?;
        write_file(&home.join("skills").join(directory).join(file), &text)?;
    }
    Ok(())
}
