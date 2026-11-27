//! Skill 用例（P5）：配置装载、解析与发现、索引与加载、预算、格式拒绝、重载、接口、安全、指标、
//! 端到端与客户端入口。
//!
//! 断言口径：数据只来自进程内的公开契约（`agent_skill::*`、`lib_bin_cli::config::parse_skills`、
//! `manager_agent::skill`）、生成的 SDK 接口响应（`api/skill/*`）、客户端事件与 Mock 的
//! `/mock/last-request`；**不读日志**。每条用例的取证来源写在文件头的注释里。

mod api;
mod budget;
mod config;
mod discover;
mod e2e;
mod index;
mod invalid;
mod load;
mod metrics;
mod parse;
mod reload;
mod security;
mod tools_missing;
mod tui;
mod web;

use std::future::Future;
use std::path::Path;
use std::sync::Arc;

use agent_skill::{SkillEntry, SkillLimits, SkillLoad, SkillRegistry, SkillSnapshot, SkillStatus, ToolAvailability};
use agent_tool::{Tool, ToolContext};
use anyhow::{Context, Result, bail, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiClient, SkillAction, SkillDetailVO, SkillGetParams, SkillUpdatePO, SkillVO};

use crate::case::{Case, CaseFn, CaseKind};
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::server;
/// Skill 用例；排序由上层统一处理。
pub(crate) fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    cases.extend(config::cases());
    cases.extend(parse::cases());
    cases.extend(discover::cases());
    cases.extend(index::cases());
    cases.extend(load::cases());
    cases.extend(budget::cases());
    cases.extend(invalid::cases());
    cases.extend(reload::cases());
    cases.extend(security::cases());
    cases.extend(api::cases());
    cases.extend(metrics::cases());
    cases.extend(e2e::cases());
    cases.extend(tools_missing::cases());
    cases.extend(tui::cases());
    cases.extend(web::cases());
    cases
}

/// 声明一个用例：Skill 用例都不需要额外的前置产物。
pub(super) fn case(name: &'static str, kind: CaseKind, run: CaseFn) -> Case {
    Case {
        name,
        kind,
        prep: &[],
        run,
    }
}

/// 起一次私有服务端：`[provider]` 指向一个必然连不上的地址（用例不建会话）。
pub(super) async fn serve_plain<T>(context: &TestContext, body: impl Future<Output = Result<T>>) -> Result<T> {
    let text = server::config("http://127.0.0.1:9", "");
    server::serve_once(context, &text, body).await
}

/// 起一次私有服务端并绑定私有 Mock LLM：需要真的跑完一轮 Run 的用例走这里。
pub(super) async fn serve_with<T>(
    context: &TestContext,
    mock: &MockServer,
    body: impl Future<Output = Result<T>>,
) -> Result<T> {
    let text = server::config(mock.base_url(), "");
    server::serve_once(context, &text, body).await
}

/// 写一份 Skill：`<AGENT_HOME>/skills/<dir>/SKILL.md`。
pub(super) fn write_skill(home: &Path, dir: &str, text: &str) -> Result<()> {
    write_file(&home.join("skills").join(dir).join("SKILL.md"), text)
}

/// 写一份引用材料：`<AGENT_HOME>/skills/<dir>/references/<name>`。
pub(super) fn write_reference(home: &Path, dir: &str, name: &str, text: &str) -> Result<()> {
    write_file(&home.join("skills").join(dir).join("references").join(name), text)
}

/// 写入用例自己的文件（统一走用例目录，不碰仓库里的配置）。
pub(super) fn write_file(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("创建测试目录失败：{}", parent.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("写入测试文件失败：{}", path.display()))
}

/// 清单（`GET api/skill/list`）。
pub(super) async fn list(context: &TestContext) -> Result<Vec<SkillVO>> {
    Ok(context.client().skill_list().await.map_err(ClientError::from_call)?)
}

/// 详情（`GET api/skill/get`）。
pub(super) async fn get(context: &TestContext, name: &str) -> Result<SkillDetailVO> {
    let detail = context
        .client()
        .skill_get(SkillGetParams { name: name.to_owned() })
        .await
        .map_err(ClientError::from_call)?;
    Ok(detail)
}

/// 执行一个动作（`POST api/skill/update`），返回动作完成后的整张清单。
pub(super) async fn update(context: &TestContext, name: Option<&str>, action: SkillAction) -> Result<Vec<SkillVO>> {
    let list = context
        .client()
        .skill_update(SkillUpdatePO {
            name: name.map(str::to_owned),
            action,
        })
        .await
        .map_err(ClientError::from_call)?;
    Ok(list)
}

/// 断言一个动作被拒成 400，返回固定文案。
pub(super) async fn update_rejected(context: &TestContext, name: Option<&str>, action: SkillAction) -> Result<String> {
    match context
        .client()
        .skill_update(SkillUpdatePO {
            name: name.map(str::to_owned),
            action,
        })
        .await
        .map_err(ClientError::from_call)
    {
        Err(ClientError::Status { code, message }) => {
            ensure!(code == 400, "应返回 400，实际 {code}（{message}）");
            Ok(message)
        }
        other => bail!("应返回固定 400，实际 {other:?}"),
    }
}

/// 断言 `get` 被拒成 400，返回固定文案。
pub(super) async fn get_rejected(context: &TestContext, name: &str) -> Result<String> {
    match context
        .client()
        .skill_get(SkillGetParams { name: name.to_owned() })
        .await
        .map_err(ClientError::from_call)
    {
        Err(ClientError::Status { code, message }) => {
            ensure!(code == 400, "应返回 400，实际 {code}（{message}）");
            Ok(message)
        }
        other => bail!("应返回固定 400，实际 {other:?}"),
    }
}

/// 清单里的名字（按接口返回的顺序）。
pub(super) fn names(list: &[SkillVO]) -> Vec<&str> {
    list.iter().map(|skill| skill.name.as_str()).collect()
}

/// 按名字取清单里的一条；不存在时失败并列出实际清单。
pub(super) fn find<'a>(list: &'a [SkillVO], name: &str) -> Result<&'a SkillVO> {
    list.iter()
        .find(|skill| skill.name == name)
        .with_context(|| format!("清单里没有 {name}：{:?}", names(list)))
}

/// 一份 `READY` 条目：正文由 `SKILL.md` 文本解析而来，章节表与字符数因此与发现层同源。
///
/// 进程内用例（索引 / 加载 / 预算）用它装配注册表，不碰磁盘也不启服务端。
pub(super) fn ready_entry(directory: &str, text: &str) -> Result<SkillEntry> {
    let path = format!("{directory}/SKILL.md");
    let file = agent_skill::parse(&path, text.as_bytes(), SkillLimits::default().max_skill_bytes)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(SkillEntry {
        name: file.metadata.name.clone(),
        directory: directory.to_owned(),
        status: SkillStatus::Ready,
        metadata: file.metadata,
        body_chars: file.body.chars().count(),
        body: file.body,
        sections: file.sections,
        references: file.references,
    })
}

/// 一个 `INVALID` 条目：解析失败时把错误原样装进状态，名字回落到目录名（与发现层同口径）。
pub(super) fn invalid_entry(directory: &str, text: &str) -> Result<SkillEntry> {
    let path = format!("{directory}/SKILL.md");
    let name = directory.rsplit('/').next().unwrap_or(directory).to_owned();
    let error = agent_skill::parse(&path, text.as_bytes(), SkillLimits::default().max_skill_bytes)
        .err()
        .with_context(|| format!("{directory} 应被判为 INVALID"))?;
    Ok(SkillEntry {
        name,
        directory: directory.to_owned(),
        status: SkillStatus::Invalid { error },
        metadata: Default::default(),
        body: String::new(),
        body_chars: 0,
        sections: Vec::new(),
        references: Vec::new(),
    })
}

/// 进程内注册表：把条目装进一个注册表（不启服务端）。
pub(super) fn registry_of(limits: SkillLimits, entries: Vec<SkillEntry>) -> Arc<SkillRegistry> {
    let registry = Arc::new(SkillRegistry::new(limits));
    registry.reload(SkillSnapshot::new(entries));
    registry
}

/// 固定的工具可用性判定：只认名单里的名字。
pub(super) struct Available(Vec<&'static str>);

impl Available {
    /// 以「此刻可用的工具名」建一个判定器。
    pub(super) fn new(names: &[&'static str]) -> Self {
        Self(names.to_vec())
    }
}

impl ToolAvailability for Available {
    fn available(&self, name: &str) -> bool {
        self.0.contains(&name)
    }
}

/// 一次工具调用的上下文：Skill 用例只关心应用根目录。
pub(super) fn tool_context(home: &Path) -> ToolContext {
    ToolContext {
        home: home.to_path_buf(),
        session_id: "s-skill".to_string(),
        run_id: "r-skill".to_string(),
        tool_call_id: "c-skill".to_string(),
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(10),
    }
}

/// 调一次 `skill_load`：返回（结果文本, 是否失败）。
pub(super) async fn call_load(tool: &SkillLoad, home: &Path, args: serde_json::Value) -> (String, bool) {
    match tool.call(&tool_context(home), args).await {
        Ok(output) => (output.text, output.is_error),
        Err(error) => (error.to_string(), true),
    }
}

/// 创建符号链接：Windows 上需要开发者模式或管理员权限，因此调用方必须处理 `Err`。
pub(super) fn symlink(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(from, to)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(from, to)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (from, to);
        Err(std::io::Error::other("当前平台不支持符号链接"))
    }
}

/// 一条请求消息的文本：`content` 可能是字符串，也可能是内容分段数组（Rig 把 system 文本包成分段）。
pub(super) fn message_text(message: &serde_json::Value) -> Option<String> {
    match &message["content"] {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join(""),
        ),
        _ => None,
    }
}

/// 请求体里的 system 消息文本（按顺序）。
pub(super) fn system_messages(request: &serde_json::Value) -> Vec<String> {
    request["messages"]
        .as_array()
        .map(|messages| {
            messages
                .iter()
                .filter(|message| message["role"] == "system")
                .filter_map(message_text)
                .collect()
        })
        .unwrap_or_default()
}

/// 请求体里的工具名（`tools[].function.name`）。
pub(super) fn tool_names(request: &serde_json::Value) -> Vec<String> {
    request["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
