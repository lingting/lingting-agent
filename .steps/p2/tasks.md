# P2 任务清单

**状态：已开工（2026-11-20）：Batch A（P2-001 ~ P2-005）完成、Batch B（P2-006 ~ P2-009）完成、Batch C（P2-010 ~ P2-014）已完成并验收（`persist` / `runtime` / `session` / `web` 用例与 `make check`、`cargo build --workspace` 全绿），结论见 `decisions.md`；Batch D 待开工。** 任务状态维护规则见 `.steps/README.md` 第六节；阶段边界、模块落位与配置面见本目录 [README.md](README.md)，判据见 [acceptance.md](acceptance.md)，结论见 [decisions.md](decisions.md)。

批次之间强序，批次内可并行（每批开头给出依赖与并行关系）。**每个任务的「测试要求」只允许在 `crates/bin-tests` 落地用例；其他 Crate 不得写 `#[cfg(test)]`**（仓库 `AGENTS.md` 第 28 条）。

> 上一版方案（35 个任务）已作废，编号不复用：本版按 P1.1 的客户端连接与会话运行时模型重新编排。

---

## Batch A：架构与技术决策（P2-001 ~ P2-005）

> 依赖：P1.1 验收通过。**本批次不写业务代码**，结论回填 `README.md` 与 `decisions.md`。

- [x] **P2-001 Rig 适配边界勘察**

  **目标**：确认 `rig-core` 0.46 的调用面、流式面与错误面，钉死「Rig 只做 Provider 传输与协议编解码」的边界。
  **涉及文件 / Crate**：`decisions.md`（D-P2-001、OPEN O-1 / O-2）；不写代码。
  **实施步骤**：
  1. 核实 `driver::Model::{call, stream}` 的签名、`Streamed<Completion>` 的消费方式与 `finish()` 的返回值。
  2. 核实 OpenAI 兼容路径（`providers::openai::{OpenAIConfig, Route, OpenAI}`、`with_base_url` / `with_route`）。
  3. 核实 `ProviderError` 的变体与可映射字段（认证失败 / 上游错误 / 网络 / 截断 / JSON）。
  4. 实测「drop `LlmStream` 后底层连接是否立即关闭」（O-1），记录方法与本机结果。
  **验收标准**：
  - [x] 三段调用面（构造 / 流式 / 错误）各有源码出处与最小示例。
  - [x] O-1 有实测结论或明确的「未实测 + 影响范围」标注。
  **完成产物**：D-P2-001 定稿、O-1 结论。
  **测试要求**：无。

- [x] **P2-002 SQLite 选型与 Schema 草案**

  **目标**：定稿 `sqlx` 版本、feature 组合、表结构与索引，作为 P2-011 的唯一依据。
  **涉及文件 / Crate**：`Cargo.toml` 的 `[workspace.dependencies]`（P2-010 落地时改）、`decisions.md`（D-P2-002、D-P2-003、D-P2-004、D-P2-005、O-3）。
  **实施步骤**：
  1. 复核 `sqlx` 版本（0.8 现状声明 vs 0.9 的 `rust-version`），定稿 feature：`runtime-tokio-rustls` / `sqlite` / `json` / `migrate`，去掉 `macros` 与 `any`。
  2. 草案表结构：`sessions` / `messages`（`seq` + `UNIQUE(session_id, seq)`）/ `runs`，字段与 P1 契约类型一一对应（`*_at_ms` 用 INTEGER）。
  3. 定稿终态条件更新语句形态（`WHERE id=? AND status IN ('QUEUED','RUNNING')`）。
  4. 记录数据库位置、连接参数（`max_connections` / WAL / `busy_timeout` / `foreign_keys`）与测试隔离方式（`AGENT_HOME`）。
  **验收标准**：
  - [x] 版本与 feature 组合唯一确定，无 `macros`。
  - [x] 表结构覆盖 Session / Message / Run 的全部契约字段，含唯一约束与外键。
  - [x] 终态更新语句与索引一并写明。
  **完成产物**：D-P2-002 ~ D-P2-005 定稿、Schema 草案。
  **测试要求**：无。

- [x] **P2-003 Context Budget 与 Token 估算策略**

  **目标**：定稿 Token 预算来源、估算公式、裁剪顺序与超限失败行为。
  **涉及文件 / Crate**：`decisions.md`（D-P2-012、D-P2-013、O-4）；不写代码。
  **实施步骤**：
  1. 预算来源：`[[provider.models]]` 的 `context_window` / `max_output_tokens`，缺省 8192 / 1024。
  2. 估算公式：`tokens ≈ ceil(ascii_chars / 4) + non_ascii_chars + 4`；`available = context_window - max_output_tokens - safety_tokens - system_prompt_tokens`。
  3. 裁剪顺序：先按 `context_max_messages` 截断最旧，再逐条丢弃直到不超预算；系统提示与最新用户消息永不裁。
  4. 超限失败：最新用户消息单条即超预算 → Run `FAILED`、`error` = 「上下文超出模型预算」，不发起上游请求。
  **验收标准**：
  - [x] 公式、缺省值、裁剪顺序、失败文案四处唯一确定。
  - [x] 估算器为 trait + 默认实现，构造点唯一。
  **完成产物**：D-P2-012、D-P2-013 定稿。
  **测试要求**：无。

- [x] **P2-004 Run 执行模型定稿（继承 P1.1 通路）**

  **目标**：在 P1.1 的执行队列之上钉死「P2 只换执行内容」的接入方式、状态转换表、持久化时序、并发与取消，作为 P2-022 ~ P2-026 的唯一依据。
  **涉及文件 / Crate**：`decisions.md`（D-P2-028、D-P2-029；修订 D-P2-007、D-P2-008、D-P2-009、D-P2-010、D-P2-011）。
  **实施步骤**：
  1. 记录 P1.1 事实：Run 由「会话被占用 + 消息创建」触发；`RuntimeSession` 内的 `RunExecutor` 串行执行；事件先落盘 flush 再转发；`api/run/stream` 已删除。
  2. 定执行端口：`service-core` 定义 trait（输入：Run、用户提示、`mpsc::Sender<RunEvent>`、取消信号；输出：终态结果），`agent-runtime` 实现，`lib-bin-web` 注册；`service-core` 不依赖 `agent-*`。
  3. 状态转换表（起点 / 终点 / 触发方 / 守卫）：
     | 起点 | 终点 | 触发方 | 守卫 |
     |---|---|---|---|
     | — | QUEUED | `api/message/create` | 单事务写用户消息 + Run |
     | QUEUED | RUNNING | 会话执行队列（占用存在） | `UPDATE ... WHERE status='QUEUED'` |
     | RUNNING | COMPLETED | 执行器成功收尾 | 事务内条件更新 + 插入助手消息 |
     | RUNNING | FAILED | 执行器失败收尾 | 条件更新 + 脱敏 error |
     | QUEUED / RUNNING | CANCELLED | `api/run/cancel` / 客户端断连 / 服务关闭 / 启动自愈 | 条件更新，影响 0 行即放弃 |
     | 任意终态 | 任意 | — | 禁止；条件更新影响 0 行视为「已终态」 |
  4. 持久化时序：`message/create` 单事务；流式期间不写库；成功 = 助手消息 + `COMPLETED` 同事务，提交后才发 `run_completed`；失败 = `FAILED` 且不写助手消息，发 `run_failed`；取消 = `CANCELLED` 且发 `run_cancelled`；关闭 = `CANCELLED` + `server_shutdown`。
  5. 终态事件归属：**执行层负责每条 Run 的终态事件**；P1.1 写者的「停止时兜底补写」只在最后一条事件不是终态时触发，作为安全网保留。
  6. 并发：同会话串行由 P1.1 执行队列保证；P2 只加全局 `Semaphore(max_concurrent_runs)`，获取顺序「先队列执行（已串行）→ 再全局信号量」；排队超时 `run_queue_timeout_ms` 到点落 `FAILED`「执行排队超时」。
  7. 取消归属：每 Run 一个 `CancellationToken`；`api/run/cancel` 先条件更新（数据库权威）再触发 token；断连/关闭由 P1.1 的 `SessionStop` 触发，与取消同权；模型流中断方式 = `select!` 命中后 drop 流。
  8. 启动自愈：`RUNNING → FAILED`（「服务重启，执行中的 Run 已终止」）、`QUEUED → CANCELLED`（「服务重启，排队中的 Run 已取消」），在迁移之后、监听之前执行一次；占用表不恢复（D-P1.1-007）。
  **技术要求**：只写结论与时序，不写实现；所有判定必须能转成用例。
  **验收标准**：
  - [x] 执行端口的输入输出与注入点唯一确定，依赖方向不与 `docs/workspace.md` 冲突。
  - [x] 状态转换表完整，含终态不可再转与幂等取消。
  - [x] 六条持久化路径各有明确写时序，且标明终态事件由谁发出。
  - [x] 并发、排队与取消参数齐备；与 P1.1 的断连/关闭链路无冲突。
  **完成产物**：D-P2-028、D-P2-029 新增；D-P2-007 ~ D-P2-011 修订。
  **测试要求**：无。

- [x] **P2-005 API 契约、密钥策略与决策归档**

  **目标**：把 P2 对契约的影响定成零变更结论，给出密钥规则，并归档 `docs/workspace.md` 变更清单。
  **涉及文件 / Crate**：`decisions.md`（D-P2-014 ~ D-P2-018、D-P2-021 ~ D-P2-027）；`docs/workspace.md` 变更清单。
  **实施步骤**：
  1. 逐条核对 25 条接口（探针 12 + 会话/消息/Run 6 + 客户端 4 + 占用 3）的 method / path / 入参 / 出参：确认 P2 不改。
  2. 明确 P2 **不新增**接口、DTO 字段、`RunEventKind` 取值；`run_failed` 由「无产出方」变为「真实产出」；`heartbeat` 保留但不产出（P1.1 已定）。
  3. 明确新增的**拒绝路径**（不新增错误码）：`session/create` / `message/create` 指定的 `model` 未在配置中出现 → 400 `模型不可用：<id>`；上下文超预算 → Run `FAILED`（HTTP 仍 200）。
  4. 记录 `run_started.inputTokens` 语义：P1 是字符数估算，P2 用预算估算器；`run_completed` 的 token 用 Provider 真实 usage（缺失时留空）。字段名与类型不变。
  5. 密钥策略：只存 `config.toml`（明文）或环境变量；不新增 Provider 配置接口；`api_key` 不进入任何 DTO；`Debug` / `Display` 脱敏为 `***`；错误文案只允许 provider id 与 base_url 的 host；日志不记录请求头。
  6. 归档：把 D-P2-xxx 全部状态定稿；把需要在实施中修改 `docs/workspace.md` 的条目独立成清单（Crate 新增、四条依赖边、`holder` 归属、§8.1 / §8.2 状态）。
  7. 记录 CI 策略：本地 `path` 依赖不变，P2 不碰 GitHub Actions；`make ci` 失败不作为阻塞条件，但 `make gen-check` / `make check` / `make ui-check` 必须本地通过。
  **验收标准**：
  - [x] 25 条接口逐条标注「不变」。
  - [x] 新增拒绝路径有 doc 与用例归属（P2-017 / P2-026 / P2-031）。
  - [x] 密钥规则可执行（脱敏位置、禁止出现的结构、可检索断言）。
  - [x] `docs/workspace.md` 需改动的位置列成清单。
  **完成产物**：D-P2-014 ~ D-P2-018、D-P2-021 ~ D-P2-027 定稿 + `workspace.md` 变更清单。
  **测试要求**：无。

---

## Batch B：Agent 领域模型与类型迁移（P2-006 ~ P2-009）

> 依赖：Batch A。P2-006 → P2-007 → P2-008；P2-009 与 P2-006 可并行（只碰 `types-core` 的配置类型）。

- [x] **P2-006 新建 `agent-core` 并迁移 Session / Message / Run 契约类型**

  **目标**：建立 Agent 领域最底层 Crate，把 `types-core::session` 整体迁入，类型名、字段、serde 行为零差异。
  **前置依赖**：P2-005。
  **涉及文件 / Crate**：新增 `crates/agent-core/`（`src/{lib,session/{mod,message,run,event,request,summary}}.rs`）；删除 `crates/types-core/src/session/`；`crates/types-core/src/lib.rs`；workspace `Cargo.toml`。
  **实施步骤**：
  1. 原样搬迁契约类型与 `auto_*` 宏标注（`Message` / `MessageRole` / `Run` / `RunStatus` / `RunEvent` / `RunEventKind` / `SessionCreatePO` / `SessionSummary` / `MessageSendPO` / `MessageSendVO` / `MessageListParams` / `RunGetParams` / `RunCancelPO`）。
  2. `types-core` 保留 `runtime` 模块与 Settings / Provider 等非 Agent 类型；移除 `session` 并修正引用。
  3. 全仓替换 `types_core::session` → `agent_core::session`，按需在各 Crate 的 `Cargo.toml` 增加 `agent-core`。
  **技术要求**：不加 `version` 约束（`path` 依赖，D-015）；`agent-core` 只依赖 `lib-core` + serde / specta / strum / `framework-proc-*`。
  **验收标准**：
  - [x] `cargo tree -p agent-core --depth 1` 无 tokio / rig / axum / sqlx（tokio 仅经 `lib-core → framework-bus` 传递，非直接依赖）。
  - [x] `grep -rn "types_core::session" crates` 为空。
  - [x] `make gen` 后两份 SDK 字段名、类型、可选性、枚举取值与迁移前一致（声明顺序按 `full_name` 变化，见下方偏差）。
  **测试要求**：`bin-tests` 的 `runtime.contract` 等既有契约用例继续通过，不新增用例。
  **完成产物**：`agent-core` 建立，契约类型唯一来源迁移完成。
  **落地记录（2026-11-20）**：
  - 实际文件划分为沿用原模块结构：`agent-core/src/session/{mod,dto,event,request}.rs`（任务书里的 `message` / `run` / `summary` 三分未采用，避免只做文件搬运的额外拆分）。
  - 偏差 1：新增依赖边 `types-core -> agent-core`（`ClientEvent.session_event` 需要 `RunEvent`），见 D-P2-031。
  - 偏差 2：SDK 生成物按类型 `full_name` 排序，`agent_core::*` 排在 `lib_web_core::*` 之前，逐字节比对不成立；内容逐项一致，`make gen` 幂等，见 D-P2-031。

- [x] **P2-007 Run 状态机与领域校验**

  **目标**：把 Run 的合法状态转换写成领域方法，非法转换有明确错误类型。
  **前置依赖**：P2-006。
  **涉及文件 / Crate**：`crates/agent-core/src/session/run.rs`、`crates/agent-core/src/error.rs`。
  **实施步骤**：
  1. `RunStatus::can_transition_to`（`QUEUED → RUNNING` / `RUNNING → COMPLETED|FAILED` / `QUEUED|RUNNING → CANCELLED`，终态不再转）。
  2. `AgentError { InvalidTransition, NotFound, Invalid, Internal }`；`Display` 文案对外可读、不含内部细节。
  3. 与 D-P2-004 的转换表逐条对照，覆盖 `run_cancelled`（P1.1 已有的事件值）。
  **验收标准**：
  - [x] 转换表与 D-P2-009（P2-004 定稿）一致；非法转换有错误变体与文案。
  - [x] 终态不可再转在状态机上显式否定（终态到任意状态均为 `false`）；端到端否定用例由 P2-026 的 `run.cancel` 覆盖。
  **测试要求**：不新增单元测试；行为由 `bin-tests` 的 `run.*` 用例覆盖。
  **完成产物**：领域状态机。
  **落地记录（2026-11-20）**：
  - 契约类型仍留在 `session/dto.rs`，`session/run.rs` 只承载 `RunStatus` 的固有 impl，不搬动类型（避免只做文件搬运的改动）。
  - `AgentError` 放在 `agent-core/src/error.rs`，手写 `Display` + `std::error::Error`，不引入 `thiserror`；`InvalidTransition` 的文案用 `RunStatus` 的 `Display`（`strum` 注入，取值即 wire 值 `QUEUED` / `RUNNING`）。
  - 除任务要求的 `can_transition_to` 外，补一个 `transition_to`，唯一用途是给 `InvalidTransition` 一个真实构造点，避免错误变体只有定义没有产出。

- [x] **P2-008 依赖切换与 SDK 生成回归**

  **目标**：确认类型迁移后生成物语义零差异，依赖方向成立。
  **前置依赖**：P2-006、P2-007。
  **涉及文件 / Crate**：workspace `Cargo.toml`、各 Crate 的 `Cargo.toml`、`docs/workspace.md`（P2-030 统一回填）。
  **实施步骤**：
  1. `make gen` → 记录接口清单与 DTO 比对表（与 P1.1 的生成物对照）。
  2. `cargo tree` 核对依赖方向：`agent-core` 无上层依赖；`service-core` 未依赖 `agent-runtime` 等。
  3. `make gen-check` 全流程通过。
  **验收标准**：
  - [x] 接口清单与 P1.1 一致（25 条），DTO 零差异（逐行内容集合比对，仅声明顺序按 `full_name` 变化）。
  - [x] `make gen-check` 退出码 0。
  **测试要求**：`make test ARGS="sdk"` 与 `make test ARGS="runtime.contract"` 通过。
  **完成产物**：迁移回归记录。
  **落地记录（2026-11-20）**：
  - 生成物比对基线：用同一份源码（HEAD）在迁移前生成一次并留档，与迁移后生成结果逐文件比对。`sdk-rust/src/{api,enums,types}.rs`、`sdk-ts/dist/{api,enums,types}.{js,d.ts}`、`sdk-ts/package.json` 十项**行集合完全一致**，差异只有声明顺序（排序键是类型的 `full_name`，`agent_core::*` 排到 `lib_web_core::*` 之前，见 D-P2-031）。
  - 规模核对：`make gen` 输出「路由 25 条，接口 25 条」，与 D-P2-014 的 25 条接口附表一致；Rust 侧 35 个 `pub struct`、枚举取值集合与迁移前一致。
  - 依赖方向（`cargo tree`，`--depth 1`）：`agent-core` 只依赖 `lib-core` + `framework-proc-*` + serde / serde_with / specta / strum；`agent-core` 的调用方是 `types-core` / `service-core` / `lib-web` / `bin-tests`；`agent-core` 自身不含 `types-core`（无循环）；`service-core` 只新增 `agent-core` 一条 `agent-*` 边，未依赖 `agent-runtime` / `agent-route` / `agent-context*`。
  - `make gen-check` 从零重建两份 SDK 后通过（清空 `packages/` → 重新生成 → 产物完整性 → `cargo check --workspace --all-targets`），退出码 0。
  - 测试（用户授权后执行，2026-11-20）：`make test ARGS="runtime.contract"` 与 `make test ARGS="sdk"` 均通过（`runtime.contract`、`sdk.generator` 各 1 个用例，报告在 `target/tests/<运行 ID>/`）。

- [x] **P2-009 Provider / Model / Agent 配置类型（`types-core`）**

  **目标**：定义 Provider / Model / Agent 配置的结构与校验，作为 P2-017 装载与 P2-015 解析的类型来源。
  **前置依赖**：P2-005。
  **涉及文件 / Crate**：`crates/types-core/src/config/{mod,provider,agent}.rs`、`crates/types-core/src/lib.rs`。
  **实施步骤**：
  1. `ProviderConfig { id, kind, base_url, api_key: Secret, default_model, models: Vec<ModelConfig> }`、`ModelConfig { id, context_window, max_output_tokens }`。
  2. `AgentConfig { max_concurrent_runs, run_queue_timeout_ms, llm_idle_timeout_ms, context_max_messages, context_safety_tokens }`，全部有缺省值。
  3. `Secret(String)`：`Debug` / `Display` 输出 `***`，取值只能 `expose()`。
  **技术要求**：**不加 `auto_*` 宏**（内部配置类型，不参与 SDK 生成）；不新增接口。
  **验收标准**：
  - [x] 配置类型字段与 `README.md` 的配置面一一对应，缺省值明确（常量集中在模块顶部，`Default` 引用同一组常量）。
  - [x] `Secret` 的 `Debug` / `Display` 脱敏，且有唯一取值入口 `expose()`。
  **测试要求**：脱敏与缺省由 P2-017 的用例（`llm.security`）覆盖。
  **完成产物**：配置类型与 `Secret`。
  **落地记录（2026-11-20）**：
  - `types-core/src/config/{mod,provider,agent}.rs`：`ProviderConfig` / `ProviderKind`（`kebab-case`，只声明 `openai-compatible`）/ `ModelConfig` / `Secret` 在 `provider.rs`，`AgentConfig` 与五个缺省值常量在 `agent.rs`。
  - 数值类型按现有配置风格取原生类型（`usize` 计数与 token、`u64` 毫秒），不用契约层的 `i64`——本模块不进 wire，装载方是 `lib-bin-cli`。
  - 缺省值：`context_window = 8192` / `max_output_tokens = 1024` 落在 `ModelConfig` 字段上（`#[serde(default = ...)]`）；`AgentConfig` 用结构级 `#[serde(default)]` + `Default`，段缺省与字段缺省都能回落。
  - `ProviderConfig` 的 `id` / `kind` / `base_url` / `default_model` 不给缺省值：这些值没有文档依据，取值来源与「缺段是否可启动」由 P2-017 决定，避免在类型层凭空编造默认值。
  - `Secret` 用 `#[serde(transparent)]` 承载明文 `String`，`Debug` / `Display` 手写为 `***`，`expose()` 是唯一取值入口；未预置 `is_empty()` 之类便捷方法，需要时由调用方走 `expose()`。
  - 本任务只定结构与缺省，**不做校验**（`kind` 取值由 serde 拒绝、`default_model` 必须出现在 `models`、数值范围校验都归 P2-017）。

---

## Batch C：SQLite 与 Repository（P2-010 ~ P2-014）

> 依赖：Batch B。010 → 011 → 012 → 013 → 014 强序。

- [x] **P2-010 新建 `lib-db`（连接、迁移、事务、错误）**

  **目标**：SQLite 基础设施独立成 Crate，`service-core` 只见 Repository 接口。
  **前置依赖**：P2-002、P2-008。
  **涉及文件 / Crate**：新增 `crates/lib-db/`（`src/{lib,config,error,migrate,pool,transaction}.rs`）；workspace `Cargo.toml`。
  **实施步骤**：
  1. `Database::open(path, Config)`：`SqlitePoolOptions`（`max_connections=5`）、`PRAGMA journal_mode=WAL` / `busy_timeout=5000` / `foreign_keys=ON`。
  2. `migrate()` 执行内嵌迁移（`sqlx::migrate!` 或等价的手写迁移表）。
  3. 统一错误 `DbError`（打开 / 迁移 / 查询 / 事务），对外只暴露语义。
  4. 事务句柄 `Transaction`：`Database::begin` 开启、`commit` / `rollback` 收尾，未提交即 `Drop` 回滚（D-P2-034）。
  **技术要求**：`lib-db` 不依赖 `types-core` / `service-core` / `agent-*`；不写业务 SQL。
  **验收标准**：
  - [x] `cargo tree -p lib-db` 只有 `lib-core` + `sqlx` + `tokio`。
  - [x] 打开失败 / 迁移失败给出可读错误，退出码非 0（由 P2-013 装配路径验证）。
  **测试要求**：不新增单元测试；由 `persist.*` 用例覆盖。
  **完成产物**：`lib-db`。

- [x] **P2-011 Schema 与迁移脚本**

  **目标**：把 D-P2-002 的草案落成可重复执行的迁移。
  **前置依赖**：P2-010。
  **涉及文件 / Crate**：`crates/lib-db/migrations/0001_init.sql`（及 `lib-db` 内的迁移入口）。
  **实施步骤**：
  1. `sessions(id PK, title, model, created_at_ms, updated_at_ms)`。
  2. `messages(id PK, session_id FK, run_id NULL, role, content, model NULL, input_tokens NULL, output_tokens NULL, seq, created_at_ms, UNIQUE(session_id, seq))` + 索引 `(session_id, seq)`。
  3. `runs(id PK, session_id FK, status, model, error NULL, input_tokens NULL, output_tokens NULL, created_at_ms, updated_at_ms, finished_at_ms NULL)` + 索引 `(session_id, created_at_ms)` / `(status)`。
  4. 迁移幂等：重复执行不报错、不改数据。
  **验收标准**：
  - [x] 约束（PK / FK / UNIQUE / 必要 CHECK）齐全，启动日志无约束错误。
  - [x] 连续两次启动同一 data 目录不报错（P2-014 用例）。
  **测试要求**：`persist.migration`（P2-014）。
  **完成产物**：迁移脚本。

- [x] **P2-012 Session / Message / Run Repository**

  **目标**：`service-core` 的 `repository` 层落地，承载全部 SQL 与行映射。
  **前置依赖**：P2-011、P2-006。
  **涉及文件 / Crate**：`crates/service-core/src/repository/{mod,session,message,run}.rs`；`crates/service-core/Cargo.toml`（新增 `lib-db`、`agent-core`）。
  **实施步骤**：
  1. Session：create / list（含 `COUNT(*)` 消息数）/ get / touch。
  2. Message：insert（`seq = COALESCE(MAX(seq),0)+1`，同事务）/ list。
  3. Run：create（`QUEUED`）/ start（`WHERE status='QUEUED'`）/ complete（事务内插助手消息 + `WHERE status='RUNNING'`）/ fail / cancel / get / `heal_orphans`。
  4. 事务边界：`message_create`、完成收尾、失败、取消各自一个事务。
  **技术要求**：SQL 只出现在 `repository`；Service 不拼 SQL；条件更新影响 0 行返回「已终态」而非错误。
  **验收标准**：
  - [x] 全部读写有 Repository 方法，无裸 SQL 出现在 Service / Handler。
  - [x] 终态写入全部为条件更新。
  **测试要求**：由 P2-014 与 P2-026 的用例覆盖。
  **完成产物**：Repository 层。

- [x] **P2-013 用 Repository 替换内存 Store、删除 `holder`、改为显式注入**

  **目标**：`service-core` 变成「Service + Repository」，进程级单例改为启动期显式注入；进程内运行时随会话业务一起落到装配层 `manager-agent`（D-P2-035），`RuntimeState` 在装配层内持有 `SessionService`。
  **前置依赖**：P2-012。
  **涉及文件 / Crate**：`crates/service-core/src/session/{mod,service,error}.rs`（删除 `store.rs`）、删除 `crates/service-core/src/holder/` 与整个 `crates/service-core/src/runtime/`（迁入 `manager-agent/src/runtime/`）；`crates/manager-agent/src/{assemble,holder,runner,session}.rs`；`crates/lib-web/src/{session/api.rs,runtime/*.rs,probe/api.rs}`；`crates/lib-bin-web/src/{lib,server,shutdown}.rs` 与 `Cargo.toml`。
  **实施步骤**：
  1. `SessionService::new(Arc<Repository>)`；方法签名与 `SessionError` 语义不变。
  2. 删除 `service-core::holder` 与 `service-core::runtime`（整体迁到 `manager-agent::runtime`）；`RuntimeState` 增加 `session: Arc<SessionService>`（`RuntimeState::new(session)`），占用校验、执行队列与 Run 取消都用它。
  3. `manager-agent` 新增显式 holder：`OnceLock<AgentHolder { session: Arc<SessionService>, runtime: RuntimeState }>` + `install(...)` + 访问器；Handler 改用它。
  4. `lib-bin-web::serve` 装配：调 `manager_agent::assemble`（`directorys()` → `Database::open(<data>/agent.db)` → 迁移 → 自愈 → `Repository` → `SessionService` → `RuntimeState`）→ `install` → 服务；任一步失败给可读错误 + 退出码 1。
  5. `probe_health` 在未安装时返回 `runningRunCount = 0`（`lib-web/examples/api_probe_server.rs` 必须仍可独立启动）。
  **技术要求**：Handler 的签名与 path 不变；`lib-web` 不写业务规则；`service-core` 不反向依赖 `lib-web`。
  **验收标准**：
  - [x] 服务可从零启动并创建 / 写入 / 读取会话数据，数据落在 `<AGENT_HOME>/data/agent.db`。
  - [x] 进程内不再有 `Mutex<HashMap>` 会话存储与 `service-core::holder`。
  - [x] `cargo run -p lib-web --example api_probe_server` 可启动，`probe/health` 返回 200。
  - [x] P1.1 的 `runtime.*` / `session.*` / `web.*` 用例全部通过（Mock 行为仍在）。
  **测试要求**：`make test ARGS="runtime"`、`make test ARGS="session"`、`make test ARGS="web"` 全绿。
  **完成产物**：持久化存储接入、注入链路成形。

- [x] **P2-014 持久化、重启恢复与状态自愈用例**

  **目标**：用端到端用例证明「重启不丢数据、迁移可重复、残留状态被自愈、占用不恢复」。
  **前置依赖**：P2-013。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/persist.rs`（新增领域）、`cases/mod.rs`、`server.rs`（同 data 目录重启辅助）。
  **实施步骤**：
  1. `persist.session`：建会话 + 发消息（占用后跑完）→ 停止服务端 → 同 `AGENT_HOME` 重启 → 会话与消息内容、顺序一致。
  2. `persist.migration`：同一 data 目录连续启动两次，第二次不报错且数据保留。
  3. `persist.queued_heal`：发消息得到 `QUEUED` Run（不占用）→ 重启 → 该 Run 为 `CANCELLED` 且 `finishedAtMs` 有值。
  4. `persist.occupancy_reset`：重启后 `occupancy/get` 返回 404 `会话未被占用`，重新注册 + 重新占用后同一会话可继续发消息。
  **技术要求**：用例只通过生成的 SDK；重启复用同一端口；失败信息含实际状态与条数。
  **验收标准**：
  - [x] `make test ARGS="persist"` 全部通过，报告写入 `target/tests/<运行 ID>/persist.md`。
  - [x] 消息顺序稳定（`seq` 与内容一一对应）。
  - [x] 迁移重复执行无错误日志。
  **完成产物**：`persist.*` 用例。

---

## Batch D：Provider 与 Rig Adapter（P2-015 ~ P2-018）

> 依赖：P2-005、P2-009。015 → 016；017 依赖 009 + 015；**018 可与 015 / 016 并行**。

- [ ] **P2-015 新建 `agent-route`：`LlmProvider` 抽象与 Provider / Model 解析**

  **目标**：把「用哪个模型、怎么调」从 Runtime 中隔离出来，Rig 类型不外泄。
  **前置依赖**：P2-009。
  **涉及文件 / Crate**：新增 `crates/agent-route/`（`src/{lib,provider,model,error,rig/}`）。
  **实施步骤**：
  1. 抽象：`LlmProvider` trait（`stream(request) -> LlmStream`）、`LlmRequest { model, messages, max_output_tokens }`、`LlmStreamEvent { Delta, Usage, Done }`、`LlmError`（含 `public_message()`）。
  2. Provider / Model 解析：按配置解析 `default_model` 与 `[[provider.models]]`；未知模型 → `LlmError::UnknownModel(id)`（由调用方映射 400 `模型不可用：<id>`）。
  3. `rig` 子目录之外不得出现 `rig_core` 类型（连编译期类型引用也不允许）。
  **技术要求**：`agent-route` 只依赖 `agent-core`、`types-core`、`lib-core`、`rig-core`；不依赖 `agent-runtime` / `service-core` / `lib-web*`。
  **验收标准**：
  - [ ] `cargo tree -i rig-core` 只命中 `agent-route`。
  - [ ] 抽象类型可被 `agent-runtime` 使用而不引入 Rig 依赖。
  **测试要求**：由 P2-016 / P2-018 的 `llm.*` 用例覆盖。
  **完成产物**：`agent-route` 抽象层。

- [ ] **P2-016 Rig Adapter：OpenAI 兼容 + 流式 + Usage + 错误映射**

  **目标**：实现 `LlmProvider`，把 `rig-core` 的流式与错误映射成项目自有类型。
  **前置依赖**：P2-015、P2-001。
  **涉及文件 / Crate**：`crates/agent-route/src/rig/{mod,openai,stream,error}.rs`。
  **实施步骤**：
  1. 构造：`OpenAIConfig` + `with_base_url` + `Route::Chat`；`api_key` 只从 `Secret` 取值。
  2. 流式：把 `Streamed<Completion>` 映射为 `LlmStreamEvent`；usage 缺失时不构造假值。
  3. 错误映射：`ProviderError` → `LlmError`（认证 / 上游 / 网络 / 截断 / 解析 / 空闲超时），`public_message()` 固定脱敏文案。
  4. 中断：取消时 drop 流；按 O-1 的实测结论记录连接是否随之关闭。
  **验收标准**：
  - [ ] 流式增量拼接等于完整回复（由 P2-026 用例断言）。
  - [ ] 认证失败 / 上游 5xx / 非法 JSON / 中途断连各有 `LlmError` 变体，文案不含内部细节。
  **测试要求**：`llm.stream` / `llm.auth_error` / `llm.upstream_error` / `llm.invalid_json`（P2-018 提供 Mock 能力，P2-026 组装断言）。
  **完成产物**：Rig Adapter。

- [ ] **P2-017 Provider 配置装载与密钥安全**

  **目标**：把配置装载进 `service-core` 的只读 `provider` 服务，并保证密钥不泄漏。
  **前置依赖**：P2-009、P2-015、P2-013。
  **涉及文件 / Crate**：`crates/lib-bin-cli/src/config.rs`（新增 `[provider]` / `[[provider.models]]` / `[agent]` 段与模块文档示例）、`crates/service-core/src/provider/{mod,service}.rs`、`crates/lib-bin-web/src/server.rs`。
  **实施步骤**：
  1. 配置解析与校验：`kind` 只支持 `openai-compatible`；`default_model` 必须出现在 `models` 中；数值项校验范围。
  2. 环境变量覆盖（清单见 `README.md`）；环境变量优先于文件。
  3. `ProviderService` 只读：装载一次、提供 `provider()` / `model(id)`；未知模型返回 `UnknownModel`。
  4. 脱敏：`Secret` 的 `Debug` / `Display`；错误只带 provider id 与 base_url 的 host；日志不打印请求头。
  **验收标准**：
  - [ ] 缺省配置可启动（`api_key` 为空时 `chat` 调用失败但不 panic，错误可读）。
  - [ ] 未知模型 → 400 `模型不可用：<id>`（用例 `session.error` 扩展）。
  - [ ] 密钥不出现在日志、响应、`Debug` 输出中。
  **测试要求**：`llm.config`（环境变量覆盖与缺省值）、`llm.security`（canary 检索）。
  **完成产物**：Provider 配置服务。

- [ ] **P2-018 `bin-tests` 内置 OpenAI 兼容 Mock Server**

  **目标**：让 P2 的用例不依赖付费 API，且能精确构造异常场景。
  **前置依赖**：P2-005。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/llm/`（Mock Server 与用例注册）、`crates/bin-tests/src/server.rs`（如需要启动辅助）。
  **实施步骤**：
  1. 手写最小 HTTP/1.1 服务（`tokio::net::TcpListener`，不引入 Web 框架）：`POST /v1/chat/completions`（JSON 与 SSE 两种响应）、`GET /v1/models`、`GET /mock/last-request`、`GET /mock/stats`。
  2. 场景开关（按 prompt 关键字或路径参数）：正常、认证失败、上游 5xx、非法 JSON、中途断连、静默 stall、慢速分块。
  3. 记录请求体与连接关闭计数，供「增量拼接」「取消真正断连」类断言使用。
  **技术要求**：不创建 `bin-mock-server`；不引入 `lib-web` / `service-core` 作为 Mock 实现依赖（`bin-tests` 的 `service-core` 依赖只用于进程内驱动运行时内核）。
  **验收标准**：
  - [ ] 七种场景可被用例按需选择，SSE 分块间隔可控。
  - [ ] `/mock/last-request` 能返回完整的 messages 数组（供 Context 断言）。
  **测试要求**：`llm.mock`（自身可用性）。
  **完成产物**：Mock LLM Server。

---

## Batch E：Context 管理（P2-019 ~ P2-021）

> 依赖：P2-003、P2-007。019 → 020 → 021。

- [ ] **P2-019 新建 `agent-context-core`**

  **目标**：定义 Context 的稳定抽象，供 P3 的 Tool / Skill / MCP 贡献内容。
  **前置依赖**：P2-007。
  **涉及文件 / Crate**：新增 `crates/agent-context-core/`（`src/{lib,context,item,source,policy,error}.rs`）。
  **实施步骤**：
  1. `Context`（有序 Item 集合 + metadata）、`ContextItem { role, content, source }`、`ContextSource`（System / History / User）。
  2. `ContextPolicy`（预算、最大消息数、safety tokens）、`TokenEstimator` trait 的抽象位置。
  3. 纯数据结构与纯函数，`async fn` 一律不出现。
  **验收标准**：
  - [ ] `cargo tree -p agent-context-core` 只有 `agent-core` + `lib-core`。
  - [ ] 无 IO、无 async、无框架依赖。
  **测试要求**：由 P2-021 覆盖。
  **完成产物**：Context 抽象。

- [ ] **P2-020 新建 `agent-context`：Context Builder 与裁剪**

  **目标**：把「System Prompt + 历史 + 本次输入」组装成一次请求的 messages，并按预算裁剪。
  **前置依赖**：P2-019、P2-012（历史读取）。
  **涉及文件 / Crate**：新增 `crates/agent-context/`（`src/{lib,builder,estimate,trim}.rs`）。
  **实施步骤**：
  1. `ContextBuilder::build(session, history, prompt, policy, model)` → `Context`。
  2. 顺序：System Prompt（最前）→ 历史（按 `seq` 升序）→ 本次用户消息（最后）；角色映射 `USER` / `ASSISTANT`（跳过 `SYSTEM` / `TOOL` 的历史消息）。
  3. 裁剪：按 `context_max_messages` 截断最旧 → 逐条丢弃直到估算不超预算；系统提示与最新用户消息不裁；被裁条数写进 metadata（只进日志）。
  4. `HeuristicEstimator` 实现 `TokenEstimator`；构造点唯一。
  **验收标准**：
  - [ ] 顺序、角色映射、裁剪顺序与 D-P2-012 / D-P2-013 一致。
  - [ ] 最新用户消息单条超预算时返回明确错误（由调用方落 `FAILED`）。
  **测试要求**：`chat.truncate`（P2-021）。
  **完成产物**：Context Builder。

- [ ] **P2-021 Context 用例与可替换性核对**

  **目标**：证明「历史按序进入请求、系统提示在最前、超预算被裁剪、最新消息保留、估算器可替换」。
  **前置依赖**：P2-020、P2-018。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/chat.rs`（新增 `chat.context` / `chat.truncate`）。
  **实施步骤**：
  1. `chat.context`：三轮对话，断言 `/mock/last-request` 的 messages 顺序、首条 system、末条为本次输入。
  2. `chat.truncate`：把 `AGENT_CONTEXT_MAX_MESSAGES` 压到 2、`AGENT_CONTEXT_SAFETY_TOKENS` 调大，断言请求体只含被保留的消息且包含本次输入。
  3. 源码核对：`TokenEstimator` 为 trait，构造点唯一。
  **验收标准**：
  - [ ] 两条用例通过，报告落 `target/tests/<运行 ID>/chat.md`。
  - [ ] 裁剪行为有可复现的配置组合。
  **完成产物**：`chat.*` 用例（Context 部分）。

---

## Batch F：Agent Runtime 接入（P2-022 ~ P2-026）

> 依赖：Batch C + Batch D + Batch E。022 → 023 → 024；025 在 022 之后可并行；026 依赖 023 ~ 025。

- [ ] **P2-022 新建 `agent-runtime`：Agent Loop（单轮）与执行端口实现**

  **目标**：实现 `service-core` 定义的执行端口：取出 Run → 组装 Context → 调 LLM → 流式产出事件 → 收尾（落库 + 终态事件）。
  **前置依赖**：P2-004、P2-016、P2-020、P2-012。
  **涉及文件 / Crate**：新增 `crates/agent-runtime/`（`src/{lib,loop_,port,registry,error}.rs`）；`crates/service-core/src/session/{driver.rs}`（端口 trait）。
  **实施步骤**：
  1. `service-core` 定义端口 trait：`RunDriver::drive(&self, input: RunInput) -> RunOutcome`，`RunInput { run, prompt, events: mpsc::Sender<RunEvent>, cancel: CancellationToken }`。
  2. `agent-runtime` 实现：`run_started` → Context → LLM 流（每段 `message_delta`）→ 成功收尾（调用 `service-core` 的完成事务）→ `run_completed`；失败 → `run_failed`。
  3. `RunRegistry`：每 Run 一个 `CancellationToken`；Run 结束即移除。
  4. 不实现 Tool Calling：请求体不含 tools，代码中无 tool loop。
  **技术要求**：`agent-runtime` 可依赖 `agent-core`、`agent-context`、`agent-route`、`service-core`、`lib-core`；不依赖 `lib-web*` / `bin-*`；事件只能经 `Sender<RunEvent>` 产出。
  **验收标准**：
  - [ ] `cargo tree -p agent-runtime` 无 `lib-web` / `lib-bin-*` / `bin-*`；`service-core` 不依赖 `agent-runtime`。
  - [ ] 一次成功 Run 产出 `run_started` → `message_delta`+ → `run_completed`，`seq` 连续。
  **测试要求**：由 P2-026 覆盖。
  **完成产物**：`agent-runtime` 与端口 trait。

- [ ] **P2-023 执行端口接入 `service-core`（替换 Mock 生产方）**

  **目标**：删掉 Mock 生产方，执行队列改为调用已注入的端口实现；启动装配完成注册。
  **前置依赖**：P2-022、P2-013。
  **涉及文件 / Crate**：`crates/service-core/src/session/{run_trigger.rs,stream.rs,mod.rs}`、`crates/service-core/src/runtime/{registry,session}.rs`、`crates/lib-bin-web/src/server.rs`、`crates/lib-web/src/holder.rs`。
  **实施步骤**：
  1. `run_trigger::drain` 的 `run_one` 改为「调用端口」；删除 `session/stream.rs` 的 Mock 生产方。
  2. 端口实现由 `lib-bin-web` 装配时注册（`SessionService` 或 `RuntimeState` 持 `Arc<dyn RunDriver>`）；未注册时（`api_probe_server` 与部分用例）退化为「直接落 `FAILED`「执行器未装配」」，并在日志给出原因。
  3. 事件仍写进 `RuntimeSession::producer()`，写者先落盘再转发；`notify_run` 的行为不变。
  **技术要求**：`service-core` 不依赖 `agent-*`；执行队列的串行与停止语义不变（`RunExecutor::stop` 仍先中止再取消）。
  **验收标准**：
  - [ ] 仓库中不再有 Mock 回复文本（`本段由 P1 的 Mock`）。
  - [ ] 占用中的会话发消息后真实调用 LLM，事件按序落盘并转发到 `api/client/stream`。
  - [ ] 未占用会话的 Run 仍停在 `QUEUED`（P1.1 语义不变）。
  **测试要求**：`session.trigger`（既有）继续通过；`run.execute`（P2-026）覆盖真实执行。
  **完成产物**：执行端口接通，Mock 生产方下线。

- [ ] **P2-024 取消、断连与关闭联动（含终态事件归属）**

  **目标**：三条终止路径都真正中断模型调用，且事件文件与数据库的终态一致。
  **前置依赖**：P2-023。
  **涉及文件 / Crate**：`crates/agent-runtime/src/{loop_,registry}.rs`、`crates/service-core/src/session/{run_trigger.rs,service.rs}`、`crates/service-core/src/runtime/{session.rs,handler.rs}`、`crates/lib-bin-web/src/shutdown.rs`。
  **实施步骤**：
  1. `api/run/cancel`：条件更新 `CANCELLED` → 触发 token → 执行器 drop LLM 流 → 发 `run_cancelled`（`error` = 「用户取消执行」）；助手消息不写。
  2. 客户端断连 / 占用释放：`SessionStop::ClientDisconnected|OccupancyReleased` 停执行队列 → 取消在跑 Run → 执行器发 `run_cancelled`（沿用 P1.1 的三个固定原因文本之一）。
  3. 服务关闭：`ServerShutdown` → 执行器发 `server_shutdown`（`status=CANCELLED`）；写者兜底逻辑保留但正常情况下不再触发。
  4. 竞态：终态条件更新影响 0 行即静默退出，不发第二条终态事件。
  **技术要求**：取消必须在 `select!` 中 drop 流；`RunRegistry` 的清理随 Run 结束；不新增事件枚举值。
  **验收标准**：
  - [ ] 取消后 `run/get` 为 `CANCELLED` + `finishedAtMs`，无助手消息，事件文件有 `run_cancelled`。
  - [ ] 断连与关闭路径的事件文件终态分别正确，且不重复追加。
  - [ ] Mock LLM 侧记录到连接关闭（O-1 结论决定断言强度）。
  **测试要求**：`run.cancel` / `run.disconnect` / `run.shutdown`（P2-026）。
  **完成产物**：三条终止路径闭环。

- [ ] **P2-025 全局并发上限与排队超时**

  **目标**：多会话并发受全局上限约束，等待超时落 `FAILED`。
  **前置依赖**：P2-022。
  **涉及文件 / Crate**：`crates/agent-runtime/src/{limit.rs,loop_.rs}`、`crates/lib-bin-web/src/server.rs`（装配配置）。
  **实施步骤**：
  1. 执行入口前 `Semaphore::acquire`（`max_concurrent_runs`），超时 `run_queue_timeout_ms` 到点落 `FAILED`「执行排队超时」并发 `run_failed`。
  2. 同会话串行继续由 P1.1 执行队列保证，不引入 Session 租约。
  3. 排队不产生事件；`probe/health` 的 `runningRunCount` 只统计 `RUNNING`。
  **验收标准**：
  - [ ] `AGENT_MAX_CONCURRENT_RUNS=1` 时两个会话的 Run 不同时进入 `RUNNING`，且都最终落终态。
  - [ ] 排队超时分支可被用例触发（把超时压到毫秒级）。
  **测试要求**：`run.concurrency` / `run.queue_timeout`（P2-026）。
  **完成产物**：并发与排队。

- [ ] **P2-026 真实执行端到端用例（`chat.*` / `run.*`）**

  **目标**：用用例证明真实执行、流式、失败、取消、并发、断连在 P1.1 通路上闭环。
  **前置依赖**：P2-023 ~ P2-025、P2-018、P2-021。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/{chat.rs,run.rs}`、`cases/mod.rs`。
  **实施步骤**：
  1. `chat.multiturn`：注册 → 建流 → 占用 → 三轮发消息 → 断言增量拼接等于完整回复、历史进入请求体、`run/get` 终态与 usage。
  2. `run.failed_auth` / `run.failed_upstream` / `run.failed_timeout`：`run_failed` + 脱敏 `error` + 无助手消息。
  3. `run.cancel` / `run.disconnect` / `run.shutdown`：三条终止路径 + 事件文件终态。
  4. `run.concurrency` / `run.queue_timeout`：全局上限与排队超时。
  5. `run.journal_consistency`：同一 Run 的下行事件序列与事件文件序列一致（`event` 与 `seq` 一一对应）。
  **技术要求**：全部经生成的 SDK；Mock LLM 由用例进程内的 Mock Server 提供（服务端通过 `AGENT_PROVIDER_BASE_URL` 指向它）。
  **验收标准**：
  - [ ] `make test ARGS="chat"` 与 `make test ARGS="run"` 全绿，报告落 `target/tests/<运行 ID>/`。
  - [ ] 每条用例的断言能区分「Mock 行为」与「真实执行」。
  **完成产物**：真实执行用例集。

---

## Batch G：客户端与遗留项（P2-027 ~ P2-030）

> 依赖：Batch F。027 与 028 可并行；029 / 030 无代码依赖。

- [ ] **P2-027 TUI 展示失败、取消与排队态**

  **目标**：真实执行后 TUI 能区分「排队中 / 运行中 / 失败 / 取消」，不重写连接管理（P1.1 形态不变）。
  **前置依赖**：P2-026。
  **涉及文件 / Crate**：`crates/lib-bin-tui/src/{worker.rs,state.rs,ui.rs}`。
  **实施步骤**：
  1. `run_started` 之前显示「等待执行…」；`run_failed` 把脱敏 `error` 显示在消息末尾并写提示行。
  2. `run_cancelled` / `client_closed` 走既有收尾提示，不新增交互。
  3. 不新增接口调用，不改心跳与重连逻辑。
  **验收标准**：
  - [ ] `run.failed_*` 的服务端行为在 TUI 上表现为可读错误，界面不卡在流式阶段。
  - [ ] 排队中（未 `run_started`）有明确提示。
  **测试要求**：`tui.*` 既有用例继续通过；新增 `tui.run_failed` 覆盖失败态展示。
  **完成产物**：TUI 状态展示。

- [ ] **P2-028 WebUI 展示失败、取消与排队态 + W-1（console error 断言）**

  **目标**：WebUI 与 TUI 口径一致，并把 P1 遗留 W-1（浏览器 console error 断言）处置完毕。
  **前置依赖**：P2-026。
  **涉及文件 / Crate**：`lingting-agent-ui/src/pages/chat/`、`lingting-agent-ui/src/api/clientStream.ts`、`lingting-agent-ui/tests/`、`crates/bin-tests/src/cases/web/`。
  **实施步骤**：
  1. `run_failed` / `run_cancelled` 在气泡与提示条上呈现；排队中显示「等待执行…」。
  2. W-1：Playwright 用例收集 `console.error` / `pageerror`，正常对话与关闭场景均断言为空（允许白名单中的已知网络错误文案，需在用例中显式列出）。
  3. 不做本地半条消息拼接（沿用 D-011 第 4 条口径）。
  **验收标准**：
  - [ ] `web.e2e` 覆盖失败与取消场景，console 断言生效。
  - [ ] `web.ui` 全绿，`make ui-check` 通过。
  **测试要求**：`web.e2e` / `web.ui` / `web.sdk-only`。
  **完成产物**：WebUI 状态展示 + W-1 结案。

- [ ] **P2-029 P1 遗留项处置（E-3 / E-4 / A-1）**

  **目标**：把 P1 报告里挂账的三条遗留项结案。
  **前置依赖**：P2-026。
  **涉及文件 / Crate**：`docs/reports/p1-020-acceptance.md`（结案标注，由 P2-030 统一提交）、`.steps/p2/decisions.md`（D-P2-020 / D-P2-022）。
  **实施步骤**：
  1. **E-3（心跳）**：P1.1 已用客户端 ping + `: keepalive` 覆盖；`RunEventKind::Heartbeat` 保留但不产出，本项标注「契约保留值，无需验证」。
  2. **E-4（`run_failed`）**：由 `run.failed_*` 用例结案。
  3. **A-1（生成物过期检测）**：维持 D-P2-022 的 `REJECTED` 与等价门禁（`make gen-check`）。
  4. 若发现遗留项与原判据表述冲突，写清「原判据 / 处置 / 归属」，不留无归属项。
  **验收标准**：
  - [ ] 三项各有结论与证据（用例名或命令）。
  - [ ] P1 报告中对应条目的标注在 P2-030 一并提交。
  **测试要求**：复用 `run.failed_*`，不新增用例。
  **完成产物**：遗留项结案记录。

- [ ] **P2-030 文档回填（代码编辑完成后统一提交）**

  **目标**：把 P2 落地后的架构、接口与阶段状态写回文档，保持单一事实来源。
  **前置依赖**：P2-005 的清单 + 各落地任务完成。
  **涉及文件 / Crate**：`docs/workspace.md`、`docs/develop.md`、`docs/api/{readme,session,provider}.md`（如需新增 provider 说明则新建）、`.steps/README.md`、`.steps/p2/*`、`docs/reports/p1-020-acceptance.md`、`docs/reports/p2-032-acceptance.md`（P2-032 创建）。
  **实施步骤**：
  1. `workspace.md`：Crate 新增、依赖边、§8.1 阶段范围、§8.2 剩余阶段、`holder` 归属。
  2. `develop.md`：Provider 配置面、执行端口与 Run 触发口径、Context 预算参数。
  3. `docs/api/session.md`：`run_failed` 的产出条件、token 语义（估算 vs 真实 usage）、排队与终态说明。
  4. `.steps/README.md`：P2 状态与 P3 输入清单；`p2/tasks.md` 状态。
  5. P1 报告遗留项结案标注。
  **验收标准**：
  - [ ] 文档与代码一致，无第二份契约定义。
  - [ ] `docs/workspace.md` 的依赖表与 `cargo tree` 实际一致。
  **测试要求**：无。
  **完成产物**：文档回填。

---

## Batch H：端到端验收（P2-031 ~ P2-032）

> 依赖：全部批次。

- [ ] **P2-031 端到端验收**

  **目标**：按 `acceptance.md` 的 15 类条目逐条验证。
  **涉及文件 / Crate**：`crates/bin-tests`（补齐缺口用例）。
  **验收标准**：`acceptance.md` 逐条通过（含 Build / Architecture / … / End-to-End）。
  **测试要求**：`make test ARGS="--all"`、前端 `pnpm exec vp test run` 与 `pnpm exec playwright test`；全部用例仍在 `bin-tests`，其他 Crate 不写 `#[cfg(test)]`。

- [ ] **P2-032 验收报告与阶段状态**

  **目标**：沉淀结论、偏差与下游输入，并更新阶段状态。
  **涉及文件 / Crate**：`docs/reports/p2-032-acceptance.md`（新建）、`.steps/p2/{README,tasks,acceptance}.md`、`.steps/README.md`。
  **实施步骤**：
  1. 报告结构沿用 P1.1：逐条判据 + 证据 + 偏差与遗留 + 下游（P3）输入清单。
  2. 记录 `rig-core` / `sqlx` 对编译时间与 release 体积的影响（只记录不设阈值）。
  3. CI 失败原因单独记录，明确「与 P2 功能无关，P6 统一处理」。
  4. 更新阶段状态与 P3 的开工前置说明。
  **验收标准**：
  - [ ] 报告覆盖 15 类条目，每条有证据。
  - [ ] 偏差逐条写「处置 + 归属阶段」，无无归属项。
  - [ ] `.steps/README.md` 的阶段进度与 P2 文档状态一致。
  **测试要求**：无。
