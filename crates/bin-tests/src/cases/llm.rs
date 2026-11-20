//! LLM 配置用例：Provider / Agent 段的装载与校验（P2-017），以及密钥不泄漏的检索。
//!
//! 全部进程内执行，不依赖 Mock LLM Server：`llm.config` 把 TOML 文本直接喂给
//! `lib_bin_cli::config::parse_llm`（P2-017 起配置只来自 `config.toml` 与命令行，没有环境变量覆盖）；
//! `llm.security` 用 canary 密钥构造 `ProviderService`，检索 `Debug` / `Display` / 错误文案。
//! 服务端日志文件的 canary 检索归端到端验收 Z-1（P2-032）。

use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::{Context, Result, bail, ensure};
use lib_bin_cli::config::parse_llm;
use manager_agent::ProviderService;
use types_core::config::agent::{DEFAULT_CONTEXT_MAX_MESSAGES, DEFAULT_MAX_CONCURRENT_RUNS};
use types_core::config::provider::{DEFAULT_CONTEXT_WINDOW, DEFAULT_MAX_OUTPUT_TOKENS};
use types_core::config::{ModelConfig, ProviderConfig, ProviderKind, Secret};

/// `llm.security` 的 canary：只要出现在任何可输出文本里就算泄漏。
const CANARY: &str = "sk-canary-8b7f3d0c1e4a";

/// 一份完整合法的配置：Provider 两个模型（一个全字段、一个只写 id 走缺省值）+ 全量 Agent 参数。
const FULL_CONFIG: &str = r#"
[server]
addr = "127.0.0.1"
port = 20100
log_level = "warn"

[provider]
id = "default"
kind = "openai-compatible"
base_url = "https://api.example.com/v1"
api_key = "sk-file-plaintext"
default_model = "model-a"

[[provider.models]]
id = "model-a"
context_window = 128000
max_output_tokens = 4096

[[provider.models]]
id = "model-b"

[agent]
max_concurrent_runs = 2
run_queue_timeout_ms = 5000
llm_idle_timeout_ms = 30000
context_max_messages = 10
context_safety_tokens = 128
"#;

/// 拼一份只含 `[provider]` 段（给定内容）与最小 `[agent]` 段的配置文本。
fn with_provider(section: &str) -> String {
    format!("{section}\n[agent]\nmax_concurrent_runs = 2\n")
}

/// LLM 配置领域用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "llm.config",
            kind: CaseKind::Shared,
            prep: &[],
            run: config,
        },
        Case {
            name: "llm.security",
            kind: CaseKind::Shared,
            prep: &[],
            run: security,
        },
    ]
}

fn config(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();

        // 1) 没有 [provider] 段：Provider 视为未配置，Agent 参数取缺省值（服务端照常启动）。
        let minimal = parse_llm("")?;
        ensure!(minimal.provider.is_none(), "[provider] 缺省时应视为未配置 Provider");
        ensure!(
            minimal.agent.max_concurrent_runs == DEFAULT_MAX_CONCURRENT_RUNS,
            "Agent 参数缺省值与 types-core 不一致"
        );
        ensure!(
            minimal.agent.context_max_messages == DEFAULT_CONTEXT_MAX_MESSAGES,
            "Context 消息条数缺省值与 types-core 不一致"
        );
        notes.push(format!(
            "缺 [provider] 段：provider=None，max_concurrent_runs={}",
            minimal.agent.max_concurrent_runs
        ));

        // 2) 完整配置：字段逐个落地，模型缺省值补齐。
        let full = parse_llm(FULL_CONFIG)?;
        let provider = full.provider.as_ref().context("[provider] 段存在时应装载出配置")?;
        ensure!(provider.id == "default", "[provider] id 装载不符：{}", provider.id);
        ensure!(
            provider.default_model == "model-a",
            "default_model 装载不符：{}",
            provider.default_model
        );
        ensure!(provider.models.len() == 2, "模型数量不符：{}", provider.models.len());
        let model_b = &provider.models[1];
        ensure!(
            model_b.context_window == DEFAULT_CONTEXT_WINDOW && model_b.max_output_tokens == DEFAULT_MAX_OUTPUT_TOKENS,
            "只写 id 的模型应取 types-core 的缺省值"
        );
        ensure!(
            full.agent.max_concurrent_runs == 2 && full.agent.context_safety_tokens == 128,
            "[agent] 段装载不符：{:?}",
            full.agent
        );
        notes.push(format!(
            "完整配置：provider={}，models={}，max_concurrent_runs={}",
            provider.id,
            provider.models.len(),
            full.agent.max_concurrent_runs
        ));

        // 3) 非法取值全部是启动期硬错误，报错要指出字段。
        let rejected = [
            (
                "default_model 不在 models 中",
                with_provider(
                    r#"[provider]
id = "default"
kind = "openai-compatible"
base_url = "https://api.example.com/v1"
default_model = "missing"

[[provider.models]]
id = "model-a"
"#,
                ),
            ),
            (
                "模型 id 重复",
                with_provider(
                    r#"[provider]
id = "default"
kind = "openai-compatible"
base_url = "https://api.example.com/v1"
default_model = "model-a"

[[provider.models]]
id = "model-a"

[[provider.models]]
id = "model-a"
"#,
                ),
            ),
            (
                "context_window 必须大于 0",
                with_provider(
                    r#"[provider]
id = "default"
kind = "openai-compatible"
base_url = "https://api.example.com/v1"
default_model = "model-a"

[[provider.models]]
id = "model-a"
context_window = 0
"#,
                ),
            ),
            (
                "max_output_tokens 不能大于 context_window",
                with_provider(
                    r#"[provider]
id = "default"
kind = "openai-compatible"
base_url = "https://api.example.com/v1"
default_model = "model-a"

[[provider.models]]
id = "model-a"
context_window = 100
max_output_tokens = 200
"#,
                ),
            ),
            (
                "base_url 不能为空",
                with_provider(
                    r#"[provider]
id = "default"
kind = "openai-compatible"
base_url = ""
default_model = "model-a"

[[provider.models]]
id = "model-a"
"#,
                ),
            ),
            (
                "models 不能为空",
                with_provider(
                    r#"[provider]
id = "default"
kind = "openai-compatible"
base_url = "https://api.example.com/v1"
default_model = "model-a"
"#,
                ),
            ),
            (
                "openai-compatible",
                with_provider(
                    r#"[provider]
id = "default"
kind = "mock"
base_url = "https://api.example.com/v1"
default_model = "model-a"

[[provider.models]]
id = "model-a"
"#,
                ),
            ),
        ];
        for (expected, text) in &rejected {
            let message = expect_failure(text)?;
            ensure!(
                message.contains(expected),
                "错误文案应指出问题（期望包含「{expected}」）：{message}"
            );
        }

        let zero_agent = expect_failure("[agent]\nmax_concurrent_runs = 0\n")?;
        ensure!(
            zero_agent.contains("[agent] max_concurrent_runs 必须大于 0"),
            "[agent] 数值 0 应被拒绝：{zero_agent}"
        );
        notes.push(format!("非法配置全部拒绝：{} 类", rejected.len() + 1));

        Ok(notes)
    })
}

fn security(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let provider = ProviderService::new(Some(canary_provider()));
        ensure!(provider.configured(), "写了 [provider] 段时应视为已配置");

        let config = provider.provider().context("已配置时必须能取到 Provider 配置")?;
        let missing = provider
            .model(Some("no-such-model"))
            .err()
            .context("未知模型必须解析失败")?;

        let rendered = [
            format!("{provider:?}"),
            format!("{config:?}"),
            format!("{}", config.api_key),
            format!("{:?}", config.api_key),
            format!("{missing}"),
            format!("{missing:?}"),
            missing.public_message(),
        ];
        for text in &rendered {
            ensure!(!text.contains(CANARY), "密钥出现在可输出文本里：{text}");
        }

        // 未配置 Provider：文案可读，同样不携带任何密钥。
        let unconfigured = ProviderService::new(None);
        ensure!(!unconfigured.configured(), "没有 [provider] 段时应视为未配置");
        let message = unconfigured
            .model(None)
            .err()
            .context("未配置 Provider 时解析模型必须失败")?
            .public_message();
        ensure!(
            message == "调用模型失败：未配置 Provider",
            "未配置 Provider 的文案不符：{message}"
        );

        Ok(vec![
            format!("已检索 {} 处可输出文本，canary 未出现", rendered.len()),
            format!("未配置 Provider：{message}"),
        ])
    })
}

/// 断言一份配置文本必须装载失败，返回错误文案。
fn expect_failure(text: &str) -> Result<String> {
    match parse_llm(text) {
        Ok(_) => bail!("这份配置应当被拒绝：{text}"),
        Err(error) => Ok(format!("{error:#}")),
    }
}

/// 带 canary 密钥的 Provider 配置：密钥确实进了结构体，检索才有意义。
fn canary_provider() -> ProviderConfig {
    ProviderConfig {
        id: "canary".to_string(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.example.com/v1".to_string(),
        api_key: Secret::new(CANARY),
        default_model: "canary-model".to_string(),
        models: vec![ModelConfig {
            id: "canary-model".to_string(),
            context_window: DEFAULT_CONTEXT_WINDOW,
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
        }],
    }
}
