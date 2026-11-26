//! `skills.toml` 的装载与校验（P5-002 的 `skill.config`）：缺省值、逐字段落地、非法配置的文案，
//! 以及「非法配置是启动期硬错误」（退出码 2）。
//!
//! 分两段：
//!
//! - 进程内：直接把 TOML 文本喂给 `lib_bin_cli::config::parse_skills`（服务端启动时走同一条
//!   装载路径），核对缺省值与每类非法输入的「字段名 + 原因」，不读任何日志；
//! - 进程级：写一份非法的 `skills.toml` 到隔离的 `AGENT_HOME`，起一次 `bin-agent server` 子进程
//!   并断言退出码 2（失败原因只写在子进程 stderr 与日志里，用例**不读**它们）。

use std::path::Path;

use anyhow::{Context, Result, ensure};
use lib_bin_cli::config::parse_skills;
use types_core::config::SkillsConfig;
use types_core::config::skills::{
    DEFAULT_MAX_DESCRIPTION_CHARS, DEFAULT_MAX_INDEX_CHARS, DEFAULT_MAX_INLINE_CHARS, DEFAULT_MAX_SKILL_BYTES,
    DEFAULT_ROOT,
};

use super::case;
use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;
use crate::util::server::{free_port, server_binary};
use crate::util::support::temp_directory;

/// `skills.toml` 的完整样例：每个字段都写出来。
const SAMPLE: &str = r#"enabled = true
root = "skills"
disabled = ["log-triage", "db-migrate"]
max_index_chars = 3000
max_description_chars = 120
max_skill_bytes = 32768
max_inline_chars = 5000
"#;

/// 配置装载用例。
pub(super) fn cases() -> Vec<Case> {
    vec![case("skill.config", CaseKind::SelfServed, config)]
}

/// 缺省值、样例落地、非法输入文案与启动期退出码。
fn config(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();

        // 一、文件缺失 = 全部缺省（Skill 功能照常启用）。
        let defaults = SkillsConfig::default();
        ensure!(defaults.enabled, "缺省应启用 Skill");
        ensure!(defaults.root == Path::new(DEFAULT_ROOT), "缺省 root 应为 skills");
        ensure!(defaults.disabled.is_empty(), "缺省 disabled 应为空");
        ensure!(
            defaults.max_index_chars == DEFAULT_MAX_INDEX_CHARS
                && defaults.max_description_chars == DEFAULT_MAX_DESCRIPTION_CHARS
                && defaults.max_skill_bytes == DEFAULT_MAX_SKILL_BYTES
                && defaults.max_inline_chars == DEFAULT_MAX_INLINE_CHARS,
            "四个上限的缺省值不符"
        );
        notes.push(format!(
            "缺省：enabled={}、root={}、四个上限 {}/{}/{}/{}",
            defaults.enabled,
            defaults.root.display(),
            defaults.max_index_chars,
            defaults.max_description_chars,
            defaults.max_skill_bytes,
            defaults.max_inline_chars
        ));

        // 二、样例的每一项都落地。
        let sample = parse_skills(SAMPLE)?;
        ensure!(sample.enabled, "enabled 未落地");
        ensure!(sample.root == Path::new("skills"), "root 未落地");
        ensure!(
            sample.disabled == vec!["log-triage".to_owned(), "db-migrate".to_owned()],
            "disabled 未落地：{:?}",
            sample.disabled
        );
        ensure!(
            sample.max_index_chars == 3000
                && sample.max_description_chars == 120
                && sample.max_skill_bytes == 32768
                && sample.max_inline_chars == 5000,
            "四个上限未落地"
        );
        notes.push("样例：7 个字段逐项落地".to_owned());

        // 三、只写一个字段时，其余字段回落到缺省值。
        let partial = parse_skills("max_inline_chars = 100\n")?;
        ensure!(partial.max_inline_chars == 100, "写下的字段未生效");
        ensure!(
            partial.enabled
                && partial.root == Path::new(DEFAULT_ROOT)
                && partial.disabled.is_empty()
                && partial.max_index_chars == DEFAULT_MAX_INDEX_CHARS
                && partial.max_description_chars == DEFAULT_MAX_DESCRIPTION_CHARS
                && partial.max_skill_bytes == DEFAULT_MAX_SKILL_BYTES,
            "未写下的字段应回落到缺省值"
        );
        notes.push("只写一个字段：其余回落到缺省值".to_owned());

        // 四、非法配置：启动期硬错误，文案点名字段。
        let rejected: Vec<(&str, String, &str)> = vec![
            ("空文件", String::new(), "skills.toml 不能为空"),
            ("TOML 语法错", "enabled = \n".to_owned(), "解析 skills.toml 失败"),
            ("root 空串", "root = \"\"\n".to_owned(), "skills.toml 的 root 不能为空"),
            (
                "root 类型错",
                "root = 1\n".to_owned(),
                "skills.toml 的 root 必须是字符串",
            ),
            (
                "enabled 类型错",
                "enabled = \"yes\"\n".to_owned(),
                "skills.toml 的 enabled 必须是布尔值",
            ),
            (
                "max_index_chars 为 0",
                "max_index_chars = 0\n".to_owned(),
                "skills.toml 的 max_index_chars 必须大于 0",
            ),
            (
                "max_description_chars 为 0",
                "max_description_chars = 0\n".to_owned(),
                "skills.toml 的 max_description_chars 必须大于 0",
            ),
            (
                "max_skill_bytes 为 0",
                "max_skill_bytes = 0\n".to_owned(),
                "skills.toml 的 max_skill_bytes 必须大于 0",
            ),
            (
                "max_inline_chars 为 0",
                "max_inline_chars = 0\n".to_owned(),
                "skills.toml 的 max_inline_chars 必须大于 0",
            ),
            (
                "max_skill_bytes 类型错",
                "max_skill_bytes = \"big\"\n".to_owned(),
                "skills.toml 的 max_skill_bytes 必须是整数",
            ),
            (
                "描述上限大于索引上限",
                "max_index_chars = 100\nmax_description_chars = 200\n".to_owned(),
                "skills.toml 的 max_description_chars 不能大于 max_index_chars",
            ),
            (
                "disabled 不是数组",
                "disabled = \"log-triage\"\n".to_owned(),
                "skills.toml 的 disabled 必须是字符串数组",
            ),
            (
                "disabled 含空串",
                "disabled = [\"\"]\n".to_owned(),
                "skills.toml 的 disabled 不能包含空名字",
            ),
            (
                "disabled 含非法名字",
                "disabled = [\"Log_Triage\"]\n".to_owned(),
                "skills.toml 的 disabled 含非法名字",
            ),
        ];
        for (label, text, expected) in rejected {
            let message = parse_skills(&text)
                .err()
                .map(|error| format!("{error:#}"))
                .unwrap_or_default();
            ensure!(
                message.contains(expected),
                "{label} 的报错应点名 `{expected}`，实际：{message}"
            );
            notes.push(format!("{label}：{message}"));
        }

        // 五、进程级：非法 `skills.toml` 让服务端起不来，退出码 2。
        let home = temp_directory("skill-config")?;
        write_file(&home.join("skills.toml"), "\n")?;
        let code = startup_exit_code(&home).await?;
        ensure!(code == Some(2), "非法 skills.toml 的退出码应为 2，实际 {code:?}");
        notes.push("非法 skills.toml：启动退出码 2".to_owned());

        Ok(notes)
    })
}

/// 写入用例自己的文件（统一走测试目录，不碰仓库里的配置）。
fn write_file(path: &Path, text: &str) -> Result<()> {
    std::fs::write(path, text).with_context(|| format!("写入测试文件失败：{}", path.display()))
}

/// 起一次 `bin-agent server` 并等它退出：只用于「启动期硬错误」的退出码断言。
async fn startup_exit_code(home: &Path) -> Result<Option<i32>> {
    let port = free_port()?;
    let output = tokio::process::Command::new(server_binary()?)
        .arg("server")
        .arg("--addr")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--log-level")
        .arg("warn")
        .env(lib_core::directorys::ENV_HOME, home)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .context("启动服务端子进程失败")?;
    Ok(output.status.code())
}
