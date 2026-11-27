//! Skill 用例（P5）：配置装载、管理接口与指标。
//!
//! 断言口径：数据只来自进程内的公开契约（`lib_bin_cli::config::parse_skills`）、生成的 SDK 接口响应
//! （`api/skill/*`）与 `metrics` 快照；**不读日志**。解析与发现的用例在 P5-016 落地，本批次只有
//! `skill.config`、`skill.api`（P5-012 的取证）与 `skill.metrics`（P5-011 的取证）。

mod api;
mod config;
mod metrics;
mod tui;
mod web;

use std::future::Future;
use std::path::Path;

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
    cases.extend(api::cases());
    cases.extend(metrics::cases());
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
