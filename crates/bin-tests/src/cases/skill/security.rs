//! `skill.security`（P5-018）：Skill 目录里的文件不获得任何执行路径，路径也不外泄。
//!
//! 四条口径：
//!
//! - 引用清单里没有 `..` 与绝对路径，符号链接不跟随（Windows 上创建链接需要权限，缺权限时写
//!   「未采集」）；
//! - 技能目录下的可执行文件不产生任何工具（请求里的 `tools` 清单与内置工具一致）；
//! - 一次普通 Run 里没有任何工具被调用（审计为空）——Skill 只提供文本；
//! - 索引、清单、详情与错误文案里都不出现应用根目录的绝对路径。
//!
//! 数据来源是接口响应、Mock 的 `/mock/last-request` 与 `api/tool/audit/list`；不读日志。

use std::path::Path;

use agent_skill::{SkillLimits, SkillLoad};
use anyhow::{Result, ensure};
use lib_bin_cli::sdk::RunEventKind;

use super::{
    Available, call_load, case, get, list, ready_entry, registry_of, serve_with, symlink, system_messages, tool_names,
    write_file, write_reference, write_skill,
};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::cases::tool::audit;
use crate::mock::llm::{MockServer, observe};
use crate::util::context::TestContext;
use crate::util::flow::Occupied;

/// 内置工具名（`[tools.permissions]` 认可的那五个），外加本阶段唯一的 `skill_load`。
const KNOWN_TOOLS: [&str; 6] = [
    "calculator",
    "current_time",
    "file_read",
    "file_search",
    "http_request",
    "skill_load",
];
/// `alpha`：声明一个可用工具，正文里给出引用材料的相对路径。
const ALPHA: &str = "---\nname: alpha\ndescription: 第一份技能\nrequires_tools: [calculator]\n---\n\n\
                     # 步骤\n\n1. 用 `references/notes.md` 里的说明。\n";
/// 引用材料（正常文件）。
const NOTES: &str = "skills/alpha/references/notes.md";
/// 技能目录下的「可执行文件」：它只是普通文本，不应产生任何执行路径。
const SCRIPT: &str = "#!/bin/sh\necho 这条脚本不应被任何工具执行\n";

/// 安全边界用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.security", CaseKind::SelfServed, security)]
}

/// 四条口径。
fn security(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let home = context.home().to_path_buf();
        write_skill(&home, "alpha", ALPHA)?;
        write_reference(&home, "alpha", "notes.md", "先读模板再查数据。")?;
        write_file(&home.join("skills/alpha/scripts/run.sh"), SCRIPT)?;
        write_file(&home.join("skills/alpha/scripts/run.cmd"), SCRIPT)?;
        let escaped = symlink_reference(&home)?;

        let mut notes = traversal(&home).await?;
        let mock = MockServer::start().await?;
        notes.extend(serve_with(context, &mock, async { scope(context, &mock, &escaped).await }).await?);
        notes.extend(cwd_scope(&home)?);
        Ok(notes)
    })
}

/// 服务端存活期间：引用清单、工具清单、审计与路径泄露。
async fn scope(context: &TestContext, mock: &MockServer, escaped: &Escaped) -> Result<Vec<String>> {
    let mut notes = Vec::new();

    // 一、引用清单：只有相对路径，没有 `..`，符号链接不跟随。
    let detail = get(context, "alpha").await?;
    ensure!(
        detail.references == [NOTES],
        "引用清单应只含普通文件的相对路径：{:?}",
        detail.references
    );
    for item in &detail.references {
        ensure!(
            !item.split('/').any(|part| part == ".." || part == ".") && !item.starts_with('/') && !item.contains("\\"),
            "引用路径不得越权：{item}"
        );
    }
    match escaped {
        Escaped::Skipped => notes.push("引用清单：符号链接被跳过".to_owned()),
        Escaped::Unavailable(reason) => notes.push(format!("未采集（符号链接）：{reason}")),
    }

    // 二、工具清单：技能目录下的可执行文件不产生工具。
    let mut occupied = Occupied::start(context, "skill.security").await?;
    let (sent, drained) = occupied
        .say(context, &format!("照着技能做一遍（{}）", context.token()))
        .await?;
    ensure!(
        drained.terminal.event == RunEventKind::RunCompleted,
        "普通 Run 应完成：{:?}",
        drained.terminal.event
    );
    let request = observe::last_request(mock).await?;
    let tools = tool_names(&request);
    ensure!(
        tools.iter().all(|name| KNOWN_TOOLS.contains(&name.as_str())),
        "请求里的工具只能是内置工具与 skill_load：{tools:?}"
    );
    ensure!(
        tools.iter().any(|name| name == "skill_load"),
        "`skill_load` 应在请求的工具清单里：{tools:?}"
    );
    ensure!(
        !tools
            .iter()
            .any(|name| name.contains("run") || name.contains("scripts")),
        "技能目录下的脚本不应变成工具：{tools:?}"
    );

    // 三、一次普通 Run 里没有任何工具被调用：Skill 只提供文本。
    let records = audit(context, &sent.run.id).await?;
    ensure!(records.is_empty(), "普通 Run 里不应有任何工具调用：{records:?}");
    notes.push(format!("工具：{:?}；审计为空（Skill 不产生执行路径）", tools));
    occupied.release(context).await?;

    // 四、绝对路径不外泄：索引、清单、详情与错误文案里都没有应用根目录。
    let absolute = context.home().to_string_lossy().to_string();
    let system = system_messages(&request);
    ensure!(
        system.len() == 1 && !system[0].contains(&absolute) && system[0].contains("- alpha：第一份技能"),
        "索引小节不得出现绝对路径：{system:?}"
    );
    let listed = serde_json::to_string(&list(context).await?)?;
    ensure!(!listed.contains(&absolute), "清单不得出现绝对路径：{listed}");
    let detail = serde_json::to_string(&detail)?;
    ensure!(!detail.contains(&absolute), "详情不得出现绝对路径：{detail}");
    let broken = super::get_rejected(context, "ghost").await?;
    ensure!(!broken.contains(&absolute), "错误文案不得出现绝对路径：{broken}");
    notes.push("路径：索引 / 清单 / 详情 / 错误文案都没有应用根目录".to_owned());
    Ok(notes)
}

/// 工作目录侧的静态核对：脚本没有被当作 Skill 收进清单。
fn cwd_scope(home: &Path) -> Result<Vec<String>> {
    let scripts = home.join("skills/alpha/scripts");
    ensure!(
        scripts.join("run.sh").is_file(),
        "脚本应真实存在（用于证明它没被执行路径化）"
    );
    Ok(vec!["结构：技能目录下的 run.sh / run.cmd 只是普通文件".to_owned()])
}

/// 符号链接的采集结果。
enum Escaped {
    /// 链接建好了，扫描时被跳过。
    Skipped,
    /// 平台不允许建链接：显式记「未采集」。
    Unavailable(String),
}

/// 在引用目录里建一个指向应用目录外的链接；目录本身是链接时按「没有引用材料」处理。
fn symlink_reference(home: &Path) -> Result<Escaped> {
    let outside = crate::util::support::temp_directory("skill-security-outside")?;
    write_file(&outside.join("secret.md"), "越权材料")?;
    let link = home.join("skills/alpha/references/escape.md");
    match symlink(&outside.join("secret.md"), &link) {
        Ok(()) => Ok(Escaped::Skipped),
        Err(error) => Ok(Escaped::Unavailable(format!("{error}"))),
    }
}

/// 五、参数无法构造出技能目录之外的读取路径：`name` 与 `section` 都按名字匹配，不拼路径。
async fn traversal(home: &Path) -> Result<Vec<String>> {
    let mut entry = ready_entry("skills/alpha", ALPHA)?;
    entry.references = vec![NOTES.to_owned()];
    let tool = SkillLoad::new(
        registry_of(SkillLimits::default(), vec![entry]),
        std::sync::Arc::new(Available::new(&[])),
    );
    for name in ["../../AGENTS.md", "/etc/passwd", "skills/alpha/SKILL.md"] {
        let (text, error) = call_load(&tool, home, serde_json::json!({"name": name})).await;
        ensure!(
            error && text == format!("工具执行失败：技能不存在：{name}"),
            "越权 name 应按「技能不存在」拒绝（{name}）：{text}"
        );
    }
    for section in ["../../AGENTS.md", "/etc/passwd"] {
        let (text, error) = call_load(&tool, home, serde_json::json!({"name": "alpha", "section": section})).await;
        ensure!(
            error && text.starts_with("工具执行失败：技能没有这个章节：") && !text.contains("root:"),
            "越权 section 应按「没有这个章节」拒绝（{section}）：{text}"
        );
    }
    Ok(vec![
        "越权：name / section 的 `../` 与绝对路径都按名字匹配拒绝，不产生目录外读取".to_owned(),
    ])
}
