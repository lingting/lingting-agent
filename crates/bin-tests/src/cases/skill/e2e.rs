//! `skill.e2e`（P5-019）：示例 Skill 真的能驱动模型按步骤走完一轮。
//!
//! 三条证据：
//!
//! - 三份示例 Skill 都能原样被解析器接受（进程内 `agent_skill::parse`）；
//! - 端到端：Mock 剧本按正文步骤跨两轮发出 `skill_load` → `file_read`，审计里的 `tool_round`
//!   与返回文本证明「先读模板再读数据」；
//! - `file_read` 的参数正是正文第一步给出的那份模板路径。
//!
//! 数据来源是 `api/tool/audit/list`、客户端事件与 Mock 的 `/mock/last-request`；不读日志。

use std::path::Path;

use anyhow::{Context, Result, ensure};
use lib_bin_cli::sdk::{RunEventKind, ToolCallStatus};
use serde_json::{Value, json};

use super::{case, serve_with, write_file};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::cases::tool::audit;
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::flow::Occupied;
use crate::util::paths::repo_root;

/// 示例 Skill 的目录名与文件清单（相对 `crates/agent-skill/assets/skills/<dir>`）。
const EXAMPLES: [(&str, &[&str]); 3] = [
    (
        "pdf-report",
        &["SKILL.md", "references/template.md", "references/data.md"],
    ),
    ("db-migrate", &["SKILL.md", "references/rollback.md"]),
    ("log-triage", &["SKILL.md"]),
];
/// 正文第一步给出的模板路径（相对示例 Skill 的目录）。
const TEMPLATE: &str = "references/template.md";
/// 模板文件的内容特征（`file_read` 的返回里应出现它）。
const TEMPLATE_MARK: &str = "报表模板";
/// 正文三步的关键词（顺序即 `skill_load` 返回里的顺序）。
const STEPS: [&str; 3] = ["先读模板", "再查数据", "最后汇总"];

/// 端到端用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.e2e", CaseKind::SelfServed, e2e)]
}

/// 示例解析 + 一轮跨轮剧本。
fn e2e(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let home = context.home().to_path_buf();
        let mut notes = vec![examples(&home)?];
        let mock = MockServer::start().await?;
        notes.extend(serve_with(context, &mock, async { scope(context, &mock).await }).await?);
        Ok(notes)
    })
}

/// 一、三份示例 Skill 原样被解析器接受，名字与依赖工具都对得上。
fn examples(home: &Path) -> Result<String> {
    for (directory, files) in EXAMPLES {
        for file in files {
            let source = repo_root()?
                .join("crates/agent-skill/assets/skills")
                .join(directory)
                .join(file);
            let text = std::fs::read_to_string(&source)
                .with_context(|| format!("读取示例 Skill 失败：{}", source.display()))?;
            write_file(&home.join("skills").join(directory).join(file), &text)?;
        }
        let text = std::fs::read_to_string(home.join("skills").join(directory).join("SKILL.md"))?;
        let parsed = agent_skill::parse(
            &format!("skills/{directory}/SKILL.md"),
            text.as_bytes(),
            agent_skill::SkillLimits::default().max_skill_bytes,
        )
        .map_err(|error| anyhow::anyhow!("示例 {directory} 应被解析器接受：{error}"))?;
        ensure!(
            parsed.metadata.name == directory && !parsed.metadata.description.is_empty(),
            "示例 {directory} 的 frontmatter 不符：{:?}",
            parsed.metadata
        );
        ensure!(!parsed.sections.is_empty(), "示例 {directory} 应有章节");
    }
    Ok(format!("示例：{} 份 Skill 原样解析通过", EXAMPLES.len()))
}

/// 二、跨轮剧本：第一轮 `skill_load`，第二轮 `file_read`，第三轮文本。
async fn scope(context: &TestContext, mock: &MockServer) -> Result<Vec<String>> {
    let script = json!({"rounds": [
        {"calls": [{"name": "skill_load", "arguments": {"name": "pdf-report"}}]},
        {"calls": [{"name": "file_read", "arguments": {"path": format!("skills/pdf-report/{TEMPLATE}")}}]},
    ]});
    let mut occupied = Occupied::start(context, "skill.e2e").await?;
    let (sent, drained) = occupied
        .say(
            context,
            &format!(
                "按技能做一份报表（{}）
tool:{script}",
                context.token()
            ),
        )
        .await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCompleted,
        "端到端 Run 应完成：{:?}",
        drained.terminal.event
    );

    // 一、工具调用顺序：`skill_load` 在第一轮、`file_read` 在第二轮。
    let records = audit(context, &sent.run.id).await?;
    ensure!(records.len() == 2, "两轮剧本应有两条审计：{records:?}");
    let load = records
        .iter()
        .find(|record| record.tool_name == "skill_load")
        .context("应有 skill_load 的审计")?;
    let read = records
        .iter()
        .find(|record| record.tool_name == "file_read")
        .context("应有 file_read 的审计")?;
    ensure!(
        load.tool_round == 1 && read.tool_round == 2,
        "调用顺序应是 skill_load（第 1 轮）→ file_read（第 2 轮）：{} / {}",
        load.tool_round,
        read.tool_round
    );
    ensure!(
        load.status == ToolCallStatus::Ok && read.status == ToolCallStatus::Ok,
        "两次调用都应成功：{records:?}"
    );

    // 二、`skill_load` 交出的正文里三步按顺序出现（模型的下一步正是照着它走的）。
    let loaded = load.result.clone().unwrap_or_default();
    ensure!(
        loaded.starts_with("# 技能：pdf-report"),
        "`skill_load` 的首行应是技能名：{loaded}"
    );
    let mut position = 0;
    for step in STEPS {
        let found = loaded
            .find(step)
            .with_context(|| format!("`skill_load` 的正文里应有「{step}」：{loaded}"))?;
        ensure!(
            found >= position,
            "三节的顺序应是 {:?}，实际「{step}」的位置 {found} 早于上一步 {position}",
            STEPS
        );
        position = found;
    }

    // 三、`file_read` 的参数正是正文第一步给出的模板路径，结果是模板内容。
    let arguments: Value = serde_json::from_str(&read.arguments).context("file_read 的参数应是 JSON")?;
    ensure!(
        arguments["path"] == Value::String(format!("skills/pdf-report/{TEMPLATE}")),
        "file_read 应读取正文第一步给出的模板：{arguments}"
    );
    let content = read.result.clone().unwrap_or_default();
    ensure!(content.contains(TEMPLATE_MARK), "file_read 应返回模板内容：{content}");

    // 四、最后一轮是文本：请求历史里没有第三轮的调用，下行流里有非空正文。
    ensure!(
        !drained.content.is_empty(),
        "最后一轮应有助手正文（Mock 回显最后一条消息）"
    );
    let request = crate::mock::llm::observe::last_request(mock).await?;
    let rounds = tool_rounds(&request);
    ensure!(
        rounds == vec![vec!["skill_load".to_owned()], vec!["file_read".to_owned()]],
        "请求历史里的工具调用应是两轮、顺序固定：{rounds:?}"
    );
    occupied.release(context).await?;
    Ok(vec![
        "顺序：skill_load（第 1 轮）→ file_read（第 2 轮），正文三步按序出现".to_owned(),
        format!("file_read 读到模板：{}", content.lines().next().unwrap_or_default()),
    ])
}

/// 请求历史里每一轮的调用名（按 assistant 消息里 `tool_calls` 的顺序）。
fn tool_rounds(request: &Value) -> Vec<Vec<String>> {
    request["messages"]
        .as_array()
        .map(|messages| {
            messages
                .iter()
                .filter_map(|message| message["tool_calls"].as_array())
                .filter(|calls| !calls.is_empty())
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|call| call["function"]["name"].as_str().map(str::to_owned))
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default()
}
