//! `llm.config`：`[provider]` / `[[provider.models]]` / `[agent]` 段的装载与校验（P2-017）。
//!
//! 进程内执行，不依赖 Mock LLM Server：TOML 文本直接喂给
//! `lib_bin_cli::config::parse_llm`（P2-017 起配置只来自 `config.toml` 与命令行，
//! 没有环境变量覆盖）。

use crate::case::CaseFuture;
use crate::util::context::TestContext;
use anyhow::{Context, Result, bail, ensure};
use lib_bin_cli::config::parse_llm;
use types_core::config::agent::{
    Concurrency, DEFAULT_CONTEXT_MAX_MESSAGES, DEFAULT_MAX_CONCURRENT_RUNS, DEFAULT_RUN_MAX_RETRIES,
    DEFAULT_RUN_RETRY_BACKOFF_MS, MAX_RUN_MAX_RETRIES, MAX_RUN_RETRY_BACKOFF_MS,
};
use types_core::config::provider::{DEFAULT_CONTEXT_WINDOW, DEFAULT_MAX_OUTPUT_TOKENS};

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

pub(super) fn config(_context: &TestContext) -> CaseFuture<'_> {
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
            "缺 [provider] 段：provider=None，max_concurrent_runs={:?}",
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
            full.agent.max_concurrent_runs == Concurrency::Absolute(2) && full.agent.context_safety_tokens == 128,
            "[agent] 段装载不符：{:?}",
            full.agent
        );
        notes.push(format!(
            "完整配置：provider={}，models={}，max_concurrent_runs={:?}",
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

        // 4) 并发上限支持核数倍数（P2.1-015）：缺省 3C，倍数是纯函数，按注入核数断言。
        let default_cores = minimal.agent.max_concurrent_runs.resolve(8);
        ensure!(
            minimal.agent.max_concurrent_runs == Concurrency::Cores(3) && default_cores == 24,
            "[agent] 缺省并发应为 3C：8 核时解析为 {default_cores}"
        );

        let multiples = parse_llm("[agent]\nmax_concurrent_runs = \"2C\"\n")?;
        ensure!(
            multiples.agent.max_concurrent_runs == Concurrency::Cores(2)
                && multiples.agent.max_concurrent_runs.resolve(8) == 16,
            "\"2C\" 应按核数倍数装载：{:?}",
            multiples.agent.max_concurrent_runs
        );

        let absolute = parse_llm("[agent]\nmax_concurrent_runs = 4\n")?;
        ensure!(
            absolute.agent.max_concurrent_runs == Concurrency::Absolute(4)
                && absolute.agent.max_concurrent_runs.resolve(8) == 4,
            "整数写法应保持绝对值语义：{:?}",
            absolute.agent.max_concurrent_runs
        );
        notes.push(format!(
            "并发配置：缺省 3C（8 核 {default_cores}）、\"2C\"（8 核 {}）、整数 4",
            multiples.agent.max_concurrent_runs.resolve(8)
        ));

        // 5) 并发上限的非法取值同样点名字段。
        let rejected_concurrency = [
            (
                "[agent] max_concurrent_runs 必须大于 0",
                "[agent]\nmax_concurrent_runs = 0\n",
            ),
            (
                "[agent] max_concurrent_runs 必须大于 0",
                "[agent]\nmax_concurrent_runs = \"0C\"\n",
            ),
            (
                "[agent] max_concurrent_runs 取值无效",
                "[agent]\nmax_concurrent_runs = \"2X\"\n",
            ),
            (
                "[agent] max_concurrent_runs 不接受负数",
                "[agent]\nmax_concurrent_runs = -1\n",
            ),
            (
                "[agent] max_concurrent_runs 超过可用上限",
                "[agent]\nmax_concurrent_runs = 9223372036854775807\n",
            ),
        ];
        for (expected, text) in &rejected_concurrency {
            let message = expect_failure(text)?;
            ensure!(
                message.contains(expected),
                "错误文案应指出问题（期望包含「{expected}」）：{message}"
            );
        }

        // 6) Run 级重试的配置（D-P6-012）：缺省「不重试」，`0` 退避是「立即重试」，越界即硬错误。
        ensure!(
            minimal.agent.run_max_retries == DEFAULT_RUN_MAX_RETRIES
                && minimal.agent.run_retry_backoff_ms == DEFAULT_RUN_RETRY_BACKOFF_MS,
            "重试缺省值应与 types-core 一致（不重试 + 缺省退避）：{:?} / {}",
            minimal.agent.run_max_retries,
            minimal.agent.run_retry_backoff_ms
        );
        let immediate = parse_llm("[agent]\nrun_max_retries = 1\nrun_retry_backoff_ms = 0\n")?;
        ensure!(
            immediate.agent.run_max_retries == 1 && immediate.agent.run_retry_backoff_ms == 0,
            "0 退避应是「立即重试」的合法取值：{}",
            immediate.agent.run_retry_backoff_ms
        );
        let rejected_retry = [
            (
                "[agent] run_max_retries 超过上限",
                format!("[agent]\nrun_max_retries = {}\n", MAX_RUN_MAX_RETRIES + 1),
            ),
            (
                "[agent] run_retry_backoff_ms 超过上限",
                format!("[agent]\nrun_retry_backoff_ms = {}\n", MAX_RUN_RETRY_BACKOFF_MS + 1),
            ),
        ];
        for (expected, text) in &rejected_retry {
            let message = expect_failure(text)?;
            ensure!(
                message.contains(expected),
                "错误文案应指出问题（期望包含「{expected}」）：{message}"
            );
        }
        notes.push(format!(
            "非法配置全部拒绝：{} 类；重试缺省不重试 + 缺省退避，上限 {} 次 / {}ms",
            rejected.len() + rejected_concurrency.len() + rejected_retry.len(),
            MAX_RUN_MAX_RETRIES,
            MAX_RUN_RETRY_BACKOFF_MS
        ));
        Ok(notes)
    })
}

/// 拼一份只含 `[provider]` 段（给定内容）与最小 `[agent]` 段的配置文本。
fn with_provider(section: &str) -> String {
    format!("{section}\n[agent]\nmax_concurrent_runs = 2\n")
}

/// 断言一份配置文本必须装载失败，返回错误文案。
fn expect_failure(text: &str) -> Result<String> {
    match parse_llm(text) {
        Ok(_) => bail!("这份配置应当被拒绝：{text}"),
        Err(error) => Ok(format!("{error:#}")),
    }
}
