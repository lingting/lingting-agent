//! `skill.tui`（P5-014 的取证）：面板清单、`INVALID` 原因、启停、重载、详情与断连提示。
//!
//! 口径：数据只来自界面状态与生成的 SDK 接口（`api/skill/*`）；启停与重载后都要与紧随其后的
//! `api/skill/list` 逐项对齐，证明面板显示的确实是服务端返回的清单，而不是本地猜测。

use anyhow::{Result, ensure};
use lib_bin_cli::sdk::{SkillStatusKind, SkillVO};
use lib_bin_tui::testing::{App, Connection, Harness, KeyCode, KeyModifiers};
use std::time::Duration;

use super::{case, find, list, serve_with, write_reference, write_skill};
use crate::case::{Case, CaseFuture, CaseKind};
use crate::mock::llm::MockServer;
use crate::util::context::TestContext;
use crate::util::server;

/// 面板打开到清单可读的上限：P5-014 的验收标准是 3 秒（本地服务）。
const PANEL_TIMEOUT: Duration = Duration::from_secs(3);
/// 一次动作（启停、重载、详情）的上限。
const ACT_TIMEOUT: Duration = Duration::from_secs(10);
/// 用例渲染帧的宽度与高度：宽到能放下清单与正文两栏。
const SCREEN_WIDTH: u16 = 140;
const SCREEN_HEIGHT: u16 = 30;

/// 可用技能：两节正文，用于详情渲染。
const ALPHA: &str =
    "---\nname: alpha\ndescription: 第一份技能\n---\n\n# 步骤\n\n1. 先读模板。\n\n## 细节\n\n再查数据。\n";
/// 坏技能：缺 `name`，判 `INVALID`，面板必须带着可定位原因出现。
const BROKEN: &str = "---\ndescription: 坏技能\n---\n\n# 步骤\n\n1. 什么也不做。\n";
/// 重载后新增的技能：声明一个必然不在的工具，面板行因此带缺失标记。
const GAMMA: &str =
    "---\nname: gamma\ndescription: 重载后新增\nrequires_tools:\n  - pdf_render\n---\n\n# 步骤\n\n1. 只有一步。\n";

/// 技能面板用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.tui", CaseKind::SelfServed, tui)]
}

/// 面板的读屏断言 + 与服务端清单对齐：两者一起构成 C-1 / C-2 的证据。
fn tui(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = MockServer::start().await?;
        let home = context.home().to_path_buf();
        write_skill(&home, "alpha", ALPHA)?;
        write_skill(&home, "broken", BROKEN)?;

        // 断连路径不依赖服务端：面板照常可开、错误只占提示行一行、Esc 仍能关掉。
        let offline = disconnected_panel().await?;
        serve_with(context, &mock, async { scope(context, offline).await }).await
    })
}

/// 服务端不在时的面板：打开后只看到一行提示，面板本身不受阻。
async fn disconnected_panel() -> Result<String> {
    let port = server::free_port()?;
    let mut harness = Harness::start(format!("http://127.0.0.1:{port}"))?;
    ensure!(
        harness.wait_until_async(ACT_TIMEOUT, |app| app.notice.is_some()).await,
        "服务端不可达时界面应在提示行给出错误"
    );
    harness.press(KeyCode::F(2), KeyModifiers::NONE);
    ensure!(
        harness
            .wait_until_async(ACT_TIMEOUT, |app| app.notice.is_some() && app.skills.is_some())
            .await,
        "断连时打开面板应保留一行错误提示，且面板保持打开"
    );
    harness.press(KeyCode::Esc, KeyModifiers::NONE);
    ensure!(harness.app().skills.is_none(), "断连时面板仍应能被 Esc 关闭");
    harness.close().await;
    Ok("断连：面板提示一行错误且可关闭".to_string())
}

/// 服务端存活期间：清单、启停、重载、详情四条路径。
async fn scope(context: &TestContext, offline: String) -> Result<Vec<String>> {
    let home = context.home().to_path_buf();
    let mut harness = Harness::start(context.base_url().to_string())?;
    ensure!(
        harness
            .wait_until_async(ACT_TIMEOUT, |app| matches!(app.connection, Connection::Online { .. }))
            .await,
        "界面未在 {} 秒内连上服务端",
        ACT_TIMEOUT.as_secs()
    );

    // 1. 打开面板：3 秒内出清单（P5-014 的验收标准）。
    harness.press(KeyCode::F(2), KeyModifiers::NONE);
    ensure!(
        harness.wait_until_async(PANEL_TIMEOUT, |app| loaded(app, 2)).await,
        "面板打开后 {} 秒内未出清单",
        PANEL_TIMEOUT.as_secs()
    );
    let names = panel_names(&harness);
    ensure!(
        names == ["alpha", "broken"],
        "清单顺序应与接口一致（name 升序）：{names:?}"
    );
    let alpha = panel_skill(&harness, "alpha")?;
    ensure!(
        alpha.status == SkillStatusKind::Ready && alpha.enabled,
        "alpha 应为可用且已启用：{:?}",
        alpha.status
    );
    let broken = panel_skill(&harness, "broken")?;
    let error = broken.error.clone().unwrap_or_default();
    ensure!(
        broken.status == SkillStatusKind::Invalid && error.contains("SKILL.md:"),
        "INVALID 项应带可定位原因：{error}"
    );
    let screen = harness.screen(SCREEN_WIDTH, SCREEN_HEIGHT);
    ensure!(
        screen.contains("技能（2）") && screen.contains("INVALID") && screen.contains("SKILL.md:"),
        "面板应显示数量、INVALID 与原因：\n{screen}"
    );

    // 2. Enter 切换启用：面板显示与接口清单逐项相等。
    harness.press(KeyCode::Enter, KeyModifiers::NONE);
    ensure!(
        harness
            .wait_until_async(ACT_TIMEOUT, |app| enabled_of(app, "alpha") == Some(false))
            .await,
        "Enter 后 alpha 应变为禁用"
    );
    let listed = list(context).await?;
    ensure!(!find(&listed, "alpha")?.enabled, "面板显示禁用，但接口清单仍是启用");

    // 3. 再按一次 Enter 回到启用。
    harness.press(KeyCode::Enter, KeyModifiers::NONE);
    ensure!(
        harness
            .wait_until_async(ACT_TIMEOUT, |app| enabled_of(app, "alpha") == Some(true))
            .await,
        "再次 Enter 后 alpha 应恢复启用"
    );
    let listed = list(context).await?;
    ensure!(find(&listed, "alpha")?.enabled, "面板显示启用，但接口清单仍是禁用");

    // 4. `r` 重载：磁盘上新写的技能无需重启就出现在清单里。
    write_skill(&home, "gamma", GAMMA)?;
    write_reference(&home, "gamma", "notes.md", "参考资料正文\n")?;
    harness.press(KeyCode::Char('r'), KeyModifiers::NONE);
    ensure!(
        harness.wait_until_async(ACT_TIMEOUT, |app| loaded(app, 3)).await,
        "重载后新增的 gamma 未出现在清单里"
    );
    let after = list(context).await?;
    ensure!(
        panel_names(&harness) == after.iter().map(|skill| skill.name.as_str()).collect::<Vec<_>>(),
        "重载后面板清单与 api/skill/list 不一致"
    );
    let gamma = find(&after, "gamma")?;
    ensure!(
        gamma.missing_tools == ["pdf_render"],
        "gamma 声明了不存在的工具，缺失清单应为 [pdf_render]：{:?}",
        gamma.missing_tools
    );
    let screen = harness.screen(SCREEN_WIDTH, SCREEN_HEIGHT);
    ensure!(screen.contains("缺 1 个工具"), "清单行应标出缺失工具数：\n{screen}");

    // 5. `d` 详情：正文与引用清单都能读到（清单顺序 alpha / broken / gamma）。
    harness.press(KeyCode::Down, KeyModifiers::NONE);
    harness.press(KeyCode::Down, KeyModifiers::NONE);
    ensure!(
        panel_selected(&harness).as_deref() == Some("gamma"),
        "两次 ↓ 应把选中项移到 gamma，实际 {:?}",
        panel_selected(&harness)
    );
    harness.press(KeyCode::Char('d'), KeyModifiers::NONE);
    ensure!(
        harness
            .wait_until_async(ACT_TIMEOUT, |app| detail_body(app, "gamma").is_some())
            .await,
        "详情请求未在 {} 秒内返回",
        ACT_TIMEOUT.as_secs()
    );
    let body = detail_body(harness.app(), "gamma").expect("详情应已到手");
    ensure!(body.contains("1. 只有一步。"), "详情正文不对：{body}");
    let screen = harness.screen(SCREEN_WIDTH, SCREEN_HEIGHT);
    ensure!(
        screen.contains("正文：gamma") && screen.contains("skills/gamma/references/notes.md"),
        "详情应渲染正文与参考资料：\n{screen}"
    );

    // 6. Esc 先回清单、再关面板；关闭后聊天输入照常可用。
    harness.press(KeyCode::Esc, KeyModifiers::NONE);
    ensure!(
        harness
            .app()
            .skills
            .as_ref()
            .is_some_and(|panel| panel.detail.is_none()),
        "详情的 Esc 应回到清单而不是直接关面板"
    );
    harness.press(KeyCode::Esc, KeyModifiers::NONE);
    ensure!(harness.app().skills.is_none(), "清单的 Esc 应关闭面板");
    harness.type_text("面板关闭后继续输入");
    ensure!(
        harness.app().input.lines().join("\n") == "面板关闭后继续输入",
        "面板关闭后输入框应可用"
    );
    harness.close().await;

    Ok(vec![
        offline,
        "面板清单与 api/skill/list 一致（含 INVALID 与原因）".to_string(),
        "Enter 启停后与接口清单一致；r 重载后新增项出现".to_string(),
        "d 详情含正文与参考资料；Esc 先回清单再关面板".to_string(),
        "面板打开 / 关闭不影响输入框".to_string(),
    ])
}

/// 清单是否已到手且条目数为 `count`。
fn loaded(app: &App, count: usize) -> bool {
    app.skills
        .as_ref()
        .is_some_and(|panel| panel.loaded && panel.items.len() == count)
}

/// 清单里的名字（按面板顺序）。
fn panel_names(harness: &Harness) -> Vec<&str> {
    harness
        .app()
        .skills
        .as_ref()
        .map(|panel| panel.items.iter().map(|skill| skill.name.as_str()).collect())
        .unwrap_or_default()
}

/// 当前选中项的名字。
fn panel_selected(harness: &Harness) -> Option<String> {
    harness
        .app()
        .skills
        .as_ref()
        .and_then(|panel| panel.selected_skill().map(|skill| skill.name.clone()))
}

/// 面板清单里取一条（返回接口原始条目，方便逐字段断言）。
fn panel_skill(harness: &Harness, name: &str) -> Result<SkillVO> {
    let skill = harness
        .app()
        .skills
        .as_ref()
        .and_then(|panel| panel.items.iter().find(|skill| skill.name == name))
        .cloned();
    skill.ok_or_else(|| anyhow::anyhow!("面板清单里没有 {name}"))
}

/// 面板里某条技能的启用标记。
fn enabled_of(app: &App, name: &str) -> Option<bool> {
    app.skills
        .as_ref()?
        .items
        .iter()
        .find(|skill| skill.name == name)
        .map(|skill| skill.enabled)
}

/// 面板里的详情正文。
fn detail_body(app: &App, name: &str) -> Option<String> {
    let detail = app.skills.as_ref()?.detail.as_ref()?;
    (detail.name == name).then(|| detail.body.clone())
}
