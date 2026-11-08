# P2 任务清单

**状态：未开工，等待用户确认本路线图。** 任务状态维护规则见 `.steps/README.md` 第六节。

执行前必读：本目录 `README.md`（边界与限制）、`decisions.md`（已定决策与 OPEN 项）。凡任务依赖未决项的，先把决策补成 `DECIDED` 再动手。

批次之间强序，批次内可并行（每批开头给出依赖与并行关系）。**每个任务的「测试要求」只允许在 `crates/bin-tests` 落地用例；其他 Crate 不得写 `#[cfg(test)]`**（仓库 `AGENTS.md` 第 28 条）。

---

## Batch A：架构与技术决策（P2-001 ~ P2-005）

> 批次内 P2-001 / P2-002 / P2-003 可并行；P2-004 依赖 P2-001；P2-005 依赖全部。**本批次不写业务代码。**

- [ ] **P2-001 Rig 适配边界勘察**

  **目标**：把 `rig-core` 0.46 的 Provider / Completion / Streaming 能力变成书面事实清单，钉死 `agent-route` 的使用面与禁用面。
  **前置依赖**：P2 路线图确认。
  **涉及文件 / Crate**：`.steps/p2/decisions.md`（D-P2-001）；不写代码。
  **实施步骤**：
  1. 读 `rig-core 0.46` 源码：`providers::openai::{OpenAI, OpenAIConfig, Route}`、`driver::Model::{call, stream}`、`streaming::{Item, StreamEvent, CompletionStream}`、`completion::{CompletionRequest, CompletionResponse, Usage, Message}`、`error::ProviderError`。
  2. 记录构造方式：`OpenAIConfig::new(api_key).with_base_url(url).with_route(Route::Chat).client()` → `OpenAI`；`openai.chat(model)` → `Model<Chat>`；`model.stream(request)` → `CompletionStream`。
  3. 记录请求映射字段：`CompletionRequest::new(prompt).preamble(system).messages(history).max_tokens(n)`、`Message::{system, user, assistant}`。
  4. 记录流式读取与终态：`Item<StreamEvent>` 的文本增量、结束信号、`Streamed::finish()` 的 `CompletionResponse.usage`。
  5. 记录错误分类：`InvalidAuthentication` / `ProviderResponse` / `Http` / `Stalled` / `Truncated` / `Json`。
  6. **明确禁用面**：`rig` facade 的 `agent` 特性、`rig_agent::{Agent, AgentBuilder, AgentRun}`、任何 tool loop API。项目自己实现 Agent Loop（P2-025 只做单轮）。
  7. 列出「已读源码确认」与「待实测」两类条目；待实测每条标注承接任务编号（P2-016 / P2-035）。
  **技术要求**：只读勘察；依赖形态写死为 `rig-core = { version = "0.46", default-features = false, features = ["reqwest", "rustls"] }`（`reqwest` 不是 `rig-core` 默认特性）。
  **测试要求**：无（本任务不写代码）。
  **验收标准**：
  - [ ] 决策文档含 Rig 事实清单（构造 / 请求 / 流式 / 错误四段）。
  - [ ] 禁用 API 清单明确，且说明「Agent Loop 由项目实现」的边界。
  - [ ] 待实测项每条都有承接任务编号。
  **完成产物**：D-P2-001 定稿。

- [ ] **P2-002 SQLite 选型与 Schema 草案**

  **目标**：定稿 SQLite 驱动、feature 组合、连接参数、schema 与迁移策略。
  **前置依赖**：无（可与 P2-001 并行）。
  **涉及文件 / Crate**：`.steps/p2/decisions.md`（D-P2-002 ~ D-P2-005）。
  **实施步骤**：
  1. 对比 `sqlx`（0.8 现状声明 vs 0.9 最新）与 `rusqlite`：Tokio 集成、异步读写、连接池、事务、迁移、三平台支持、编译期依赖、体积、开发复杂度。
  2. 记录现状事实：`Cargo.toml` 的 `[workspace.dependencies]` 已声明 `sqlx 0.8`（`any/json/macros/runtime-tokio-rustls/sqlite`），但 `Cargo.lock` 无 `sqlx`，仓库无任何使用点——属「预留未落地」。
  3. 定稿 feature 组合：`runtime-tokio-rustls` + `sqlite`（bundled amalgamation，需要 cc）+ `json` + `migrate`；**不启用 `macros`**（避免编译期需要 `DATABASE_URL` / `.sqlx` 离线数据），查询一律用运行时 `sqlx::query` / `query_as`。
  4. 定稿连接策略：`SqlitePoolOptions` 的 `max_connections`、`PRAGMA journal_mode=WAL`、`busy_timeout`、`foreign_keys=ON`。
  5. 定稿数据库文件位置：`lib_core::directorys()` 的 `data` 目录下 `agent.db`；测试用 `AGENT_HOME` 隔离（沿用 P1）。
  6. 定稿 schema 草案（表 / 字段 / 类型 / 索引 / 约束 / 外键）：
     - `sessions(id TEXT PK, title TEXT, model TEXT, created_at_ms INTEGER, updated_at_ms INTEGER)`
     - `messages(id TEXT PK, session_id TEXT NOT NULL REFERENCES sessions(id), run_id TEXT, seq INTEGER NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, model TEXT, input_tokens INTEGER, output_tokens INTEGER, created_at_ms INTEGER, UNIQUE(session_id, seq))`
     - `runs(id TEXT PK, session_id TEXT NOT NULL REFERENCES sessions(id), status TEXT NOT NULL, model TEXT NOT NULL, error TEXT, input_tokens INTEGER, output_tokens INTEGER, created_at_ms INTEGER, updated_at_ms INTEGER, finished_at_ms INTEGER)`
     - 索引：`messages(session_id, seq)`、`runs(session_id, created_at_ms)`、`runs(status)`
     - 会话消息数用 `COUNT(*)` 子查询，不冗余计数列（避免双写不一致）。
  7. 定稿迁移：`crates/lib-db/migrations/` 下 `0001_init.sql` 起，命名 `<序号>_<snake_case>.sql`，用 `sqlx::migrate!` 内嵌执行，幂等。
  **技术要求**：结论必须落到可执行参数，不写「视情况而定」。
  **测试要求**：无（结论在 P2-010 / P2-014 验证）。
  **验收标准**：
  - [ ] 选型结论 + `rusqlite` 的 `REJECTED` 理由齐备。
  - [ ] feature 组合与不启用 `macros` 的理由写明。
  - [ ] 完整 schema（含索引、约束、外键）与迁移命名规则写明。
  - [ ] `sqlx` 版本结论明确（0.8 落地或升级到 0.9 并说明理由）。
  **完成产物**：D-P2-002 ~ D-P2-005 定稿。

- [ ] **P2-003 Context Budget 与 Token 估算策略**

  **目标**：定稿 Token 预算来源、估算方式、裁剪算法与失败语义。
  **前置依赖**：无（可与 P2-001 / P2-002 并行）。
  **涉及文件 / Crate**：`.steps/p2/decisions.md`（D-P2-012、D-P2-013）。
  **实施步骤**：
  1. 定稿预算来源：`provider.models[].context_window` 与 `max_output_tokens`（配置显式给出，P2 不引入模型目录）；缺省 `8192` / `1024`。
  2. 定稿估算公式：`tokens ≈ ceil(ascii_chars / 4) + non_ascii_chars + 4`（每条消息固定开销 4）。
  3. 定稿预算公式：`budget = context_window - max_output_tokens - safety_tokens(默认 256) - system_prompt_tokens`。
  4. 定稿裁剪算法（伪代码级）：先按 `context_max_messages`（默认 40）截断，再从最旧消息开始逐条丢弃直到不超预算；**系统提示最优先保留**；**最新一条用户消息必须保留**。
  5. 定稿边界语义：最新用户消息单条即超预算 → 该 Run 落 `FAILED`，`error` 为可读文案（不新增 HTTP 错误码）。
  6. 定稿可替换点：`TokenEstimator` trait + `HeuristicEstimator` 默认实现，通过构造参数注入。
  **技术要求**：算法必须能逐条判定，参数集中在配置结构体，不散落魔数。
  **测试要求**：无（结论在 P2-021 / P2-031 验证）。
  **验收标准**：
  - [ ] 伪代码级算法与参数表齐备。
  - [ ] 四类边界（空历史 / 单条超长 / 恰好等于预算 / 系统提示本身超预算）各自给出结论。
  - [ ] 「超预算 → Run `FAILED`」的对外文案定稿。
  **完成产物**：D-P2-012、D-P2-013 定稿。

- [ ] **P2-004 Run 执行模型：并发、串行、取消与关闭联动**

  **目标**：用状态转换表 + 持久化时序钉死 Run 的执行模型，作为 P2-022 ~ P2-026 的唯一依据。
  **前置依赖**：P2-001。
  **涉及文件 / Crate**：`.steps/p2/decisions.md`（D-P2-006 ~ D-P2-011、D-P2-019）。
  **实施步骤**：
  1. 记录 P1 事实：`service-core/src/session/service.rs::subscribe_run` 在订阅时把 `QUEUED → RUNNING` 并启动生产任务；`bin-tests/src/cases/session.rs::message` 断言「未订阅前 Run 应保持 `Queued`」。→ 结论：**P2 保留「订阅触发执行」**，不做「创建即执行」。
  2. 状态转换表（起点 / 终点 / 触发方 / 守卫）：
     | 起点 | 终点 | 触发方 | 守卫 |
     |---|---|---|---|
     | — | QUEUED | `api/message/create` | 事务内写用户消息 + Run |
     | QUEUED | RUNNING | `api/run/stream` 订阅 | 注册表占位成功 + 获得 Session 租约 + `UPDATE ... WHERE status='QUEUED'` |
     | RUNNING | COMPLETED | 执行器成功收尾 | 事务内 `WHERE status='RUNNING'` + 插入助手消息 |
     | RUNNING | FAILED | 执行器失败收尾 | `WHERE status='RUNNING'` + 脱敏 error |
     | QUEUED / RUNNING | CANCELLED | 取消接口 / 断线 / 关闭 / 启动自愈 | `WHERE status IN ('QUEUED','RUNNING')` |
     | 任意终态 | 任意 | — | 禁止；条件更新影响 0 行即视为「已终态」，不报错、不覆盖 |
  3. 持久化时序（六条路径写清楚「何时写什么」）：成功 / 失败 / 取消 / 断线 / 服务关闭 / 进程重启自愈。核心约定：**流式期间不写库**；助手消息与 `COMPLETED` 同一事务；取消与失败不写半成品助手消息；`run_completed` 必须在事务提交之后发送。
  4. 并发模型：全局 `Semaphore(max_concurrent_runs)` + 每 Session 租约（`tokio::sync::Mutex`，公平等待实现 FIFO）→ 同 Session 单 Run；获取顺序固定「先 Session 租约，后全局信号量」；排队超时 `run_queue_timeout_ms` 到点落 `FAILED`。排队期间只发心跳。
  5. Cancellation 归属：每 Run 一个 `CancellationToken`，由执行器持有、注册表存 clone；取消传播到 LLM 流的做法是 `select!` + drop 流；断线（`sender.closed()`）等价于取消；关闭用 Runtime 级广播（由 `lib-bin-web` 关闭钩子调用 `runtime.shutdown()`）。
  6. 启动自愈：`RUNNING → FAILED`（`error` = 「服务重启，执行已中断」）、`QUEUED → CANCELLED`（`error` 同义），在 `lib-db` 迁移之后、开始服务之前执行一次。
  7. 记录 `RunEventStream` 与 `holder` 的归属迁移（D-P2-019），以及「取消与完成同时发生时以数据库条件更新的结果为准」。
  **技术要求**：只写结论与时序，不写实现；所有判定必须能转成用例。
  **测试要求**：无。
  **验收标准**：
  - [ ] 状态转换表完整，含终态不可再转与幂等取消。
  - [ ] 六条持久化路径各有明确写时序。
  - [ ] 并发与排队参数齐备。
  - [ ] 竞态结论明确（谁获胜、失败方的行为）。
  **完成产物**：D-P2-006 ~ D-P2-011、D-P2-019 定稿。

- [ ] **P2-005 API 契约、密钥策略与决策归档**

  **目标**：把 P2 对 P1 契约的影响定成零变更结论，并给出密钥规则；同时归档全部决策与 `docs/workspace.md` 变更清单。
  **前置依赖**：P2-001 ~ P2-004。
  **涉及文件 / Crate**：`.steps/p2/decisions.md`（D-P2-014 ~ D-P2-018、D-P2-020 ~ D-P2-027）；`docs/workspace.md` 变更清单。
  **实施步骤**：
  1. 逐条核对 P1 的 19 条接口（探针 12 条 + 会话 7 条）的 method / path / 入参 / 出参：确认 P2 不改。
  2. 明确 P2 **不新增**接口、不新增 DTO 字段、不新增 `RunEventKind`；`run_failed` / `heartbeat` 由「无产出方」变为「真实产出」，字段语义不变。
  3. 明确新增的**拒绝路径**（不新增错误码）：`session/create` 或 `message/create` 指定的 `model` 未在配置中出现 → 400，文案 `模型不可用：<id>`；上下文超预算 → Run `FAILED`（HTTP 仍 200）。
  4. 记录 `run_started.inputTokens` 的语义微调：P1 是「按字符数估算」，P2 是「按 Context 预算估算器估算」；`run_completed.inputTokens/outputTokens` 用 Provider 真实 usage（缺失时留空）。字段名与类型不变。
  5. 密钥策略：只存 `config.toml`（明文）或环境变量；**不新增 Provider 配置接口**；`api_key` 不进入任何 DTO；`Debug` 输出脱敏为 `***`；错误消息只允许出现 provider id 与 base_url 的 host；日志不记录请求头。
  6. 归档：把 D-P2-xxx 全部状态定稿；把需要在实施中修改 `docs/workspace.md` 的条目独立成清单（Crate 新增、四条依赖边、`RunEventStream` / `holder` 归属、§8.1 阶段状态）。
  7. 记录 CI 策略：本地 `path` 依赖不变，P2 不碰 GitHub Actions；`make ci` 失败不作为阻塞条件，但 `make gen-check` / `make check` / `make ui-check` 必须本地通过。
  **技术要求**：契约结论要能被 P2-035 逐条复核。
  **测试要求**：无。
  **验收标准**：
  - [ ] 19 条接口逐条标注「不变」。
  - [ ] 新增拒绝路径有 doc 与用例归属（P2-017 / P2-031）。
  - [ ] 密钥规则可执行（脱敏位置、禁止出现的结构、可检索断言）。
  - [ ] `docs/workspace.md` 需改动的位置列成清单。
  **完成产物**：D-P2-014 ~ D-P2-018、D-P2-020 ~ D-P2-027 定稿 + workspace.md 变更清单。

---

## Batch B：Agent 领域模型与类型迁移（P2-006 ~ P2-009）

> P2-006 是批次入口；P2-007 依赖 P2-006；P2-008 依赖 P2-006、P2-007；**P2-009 可与 P2-006/007 并行**（只新增 `types-core` 内部类型，不动契约类型）。

- [ ] **P2-006 新建 `agent-core` 并迁移 Session / Message / Run 契约类型**

  **目标**：按 `docs/workspace.md` 6.2 把契约类型从 `types-core::session` 整体迁入 `agent-core`，类型名、字段名、serde 行为零变化。
  **前置依赖**：P2-005。
  **涉及文件 / Crate**：新增 `crates/agent-core/`；`Cargo.toml`（workspace members + `[workspace.dependencies]`）；删除 `crates/types-core/src/session/`。
  **实施步骤**：
  1. 新建 `crates/agent-core`，`Cargo.toml` 依赖与现 `types-core` 一致：`lib-core`、`framework-proc-auto(collect)`、`framework-proc-core(collect)`、`serde`、`serde_with`、`specta`、`strum`、`strum_macros`。
  2. 文件组织：`src/lib.rs`（模块与再导出）+ `message.rs`（`Message`、`MessageRole`）+ `run.rs`（`Run`、`RunStatus`、`SessionSummary`、`MessageSendVO`）+ `event.rs`（`RunEvent`、`RunEventKind`）+ `request.rs`（`*PO` / `*Params`）+ `error.rs`（`AgentError`，P2-007 填充）。
  3. 迁移时**逐字保留**：类型名、字段名与顺序、`#[auto_type]` / `#[auto_enum]` 参数（`clone`、`default`）、`#[serde(rename_all)]`、`#[strum(serialize_all)]`、文档注释中的契约说明。
  4. `types-core` 删除 `session` 模块与对应依赖清理（不删除 `types-core` 本身，Provider/Model 类型由 P2-009 填充）。
  5. 更新 `docs/workspace.md` 的成员清单与 §8.1 阶段状态（本任务执行时同步，不推迟）。
  **技术要求**：不新建 `types-api`，不复制 DTO；`agent-core` 不依赖 tokio / rig / axum / 数据库 / 其他 `agent-*`。
  **测试要求**：无新增用例；验证靠编译与生成物比对（P2-008）。
  **验收标准**：
  - [ ] `cargo build --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check` 通过。
  - [ ] `agent-core` 的依赖树只有 `lib-core` + 宏/序列化依赖（`cargo tree -p agent-core` 为证据）。
  - [ ] 迁移前后类型清单逐项一致（名称 / 字段 / 参数 / serde 属性）。
  **完成产物**：`crates/agent-core/` 可编译；`types-core::session` 移除；workspace 文档同步。

- [ ] **P2-007 Run 状态机与领域校验**

  **目标**：把 `QUEUED → RUNNING → {COMPLETED, CANCELLED, FAILED}` 的状态规则做成领域内可复用、可判定的代码。
  **前置依赖**：P2-006。
  **涉及文件 / Crate**：`crates/agent-core/src/run.rs`、`crates/agent-core/src/error.rs`。
  **实施步骤**：
  1. `RunStatus::is_terminal()`、`RunStatus::can_transition_to(next)`，转换表与 P2-004 一致。
  2. `AgentError` 至少覆盖：`InvalidTransition { from, to }`、`NotFound`、`Invalid`、`Internal`；`Display` 文案对外可读、不含内部细节。
  3. 提供 `Run::transition(next, now_ms) -> Result<(), AgentError>` 之类的纯函数式变更入口（不触碰数据库）。
  4. 明确「幂等取消」在领域层的表达：终态收到取消请求时返回 `InvalidTransition`，由 Service 层转成 P1 的 400 语义（文案保持不变）。
  **技术要求**：纯同步、无 async、无 IO；不引入 tokio / async-trait。
  **测试要求**：无单元测试（`AGENTS.md` 第 28 条）；规则通过 P2-012 / P2-023 的调用点与 P2-032 用例验证。
  **验收标准**：
  - [ ] 转换表在代码与 `decisions.md` D-P2-006 一致。
  - [ ] 非法转换被 `can_transition_to` 拒绝，且终态不可再转。
  - [ ] `AgentError` 的对外文案不含 SQL / 路径 / 堆栈。
  **完成产物**：状态机与错误类型就绪，供 P2-012 / P2-023 引用。

- [ ] **P2-008 依赖切换与 SDK 生成回归**

  **目标**：`service-core` / `lib-web` 改用 `agent-core`，并证明 SDK 生成的契约语义零差异。
  **前置依赖**：P2-006、P2-007。
  **涉及文件 / Crate**：`crates/service-core/Cargo.toml`、`crates/lib-web/Cargo.toml`、`crates/lib-web/src/session/api.rs`、`crates/lib-web/src/probe/api.rs`、`crates/agent-core/src/lib.rs`。
  **实施步骤**：
  1. `service-core`、`lib-web` 的 `use types_core::session::…` 全部改为 `agent_core::…`，并从依赖表加入 `agent-core`。
  2. `Cargo.toml` 的 `[workspace.dependencies]` 增加 `agent-core = { path = "crates/agent-core" }`，`members` 增加成员。
  3. 复核 `docs/workspace.md`：`service-core -> agent-core` 只允许依赖类型层；`lib-web -> agent-core` 同步登记。
  4. 记录 `lib-bin-cli` 的 `types-core` 依赖为**未使用的历史依赖**：本任务不删除（避免顺手扩大改动），只记入 `decisions.md` 并在 P2-035 复核。
  5. 跑 `make gen`，对比迁移前后生成物：DTO 名称、字段名、字段类型与可选性、枚举取值逐项一致（允许文件内顺序变化）；把对比方法与结果写入任务记录。
  **技术要求**：不修改任何 handler 的签名与 path；不改 `docs/api/*.md`（契约未变）。
  **测试要求**：`make gen-check`（清空重建 + 产物校验 + 生成代码参与编译）必须通过。
  **验收标准**：
  - [ ] `make gen`、`make gen-check`、`make check` 通过，生成物字段语义零差异。
  - [ ] `crates/types-core` 不再出现 `session` 类型；仓库内无 `types_core::session` 引用。
  - [ ] `docs/workspace.md` 的成员与依赖边与代码一致。
  **完成产物**：类型迁移完成并可通过生成门禁。

- [ ] **P2-009 Provider / Model / Agent 配置类型（`types-core`）**

  **目标**：为 Provider 配置装载与 Runtime 参数提供类型，落在 `types-core`（内部类型，不参与 API 生成）。
  **前置依赖**：P2-005。
  **涉及文件 / Crate**：`crates/types-core/src/{lib.rs, provider.rs, settings.rs}`。
  **实施步骤**：
  1. `provider.rs`：`ProviderKind`（P2 只有 `OpenAiCompatible`，其余取值留给后续阶段但**不要提前实现**）、`ProviderConfig { id, kind, base_url, api_key: Secret, default_model }`、`ModelConfig { id, provider_id, context_window, max_output_tokens }`。
  2. `Secret` 新类型：`Debug` 输出 `***`、不实现 `Serialize`、提供 `expose()` 取原值（调用点显式）。
  3. `settings.rs`：`AgentSettings`（并发上限、排队超时、LLM idle 超时、心跳间隔、Context 参数）与 `ConfigError`。
  4. 明确这些类型**不加** `#[auto_type]` / `#[auto_enum]`（不参与 SDK 生成，避免密钥形状进入契约）。
  **技术要求**：不引入 tokio / rig / sqlx；默认值集中定义且与 `README.md` 的配置表一致。
  **测试要求**：无；字段与默认值在 P2-017 的装载逻辑与 P2-035 的配置核对中验证。
  **验收标准**：
  - [ ] `cargo tree -p types-core` 无 Agent / Web / DB 依赖。
  - [ ] `Secret` 的 `Debug` 与 `Display` 均不泄漏原值。
  - [ ] 默认值与 `README.md` 的配置表逐项一致。
  **完成产物**：配置类型就绪，供 P2-015 / P2-017 引用。

---

## Batch C：SQLite 与 Repository（P2-010 ~ P2-014）

> P2-010 → P2-011 → P2-012 → P2-013 → P2-014 串行。**C 批可与 E 批并行**。

- [ ] **P2-010 新建 `lib-db`（连接、迁移、事务、错误）**

  **目标**：把 SQLite 基础设施收敛到 `lib-db`，业务侧只见 Pool / 事务 / 迁移。
  **前置依赖**：P2-002。
  **涉及文件 / Crate**：新增 `crates/lib-db/`（`Cargo.toml`、`src/{lib.rs, database.rs, error.rs, migrate.rs}`）。
  **实施步骤**：
  1. 依赖：`lib-core`、`sqlx`（feature 组合由 P2-002 定稿）、`thiserror`。
  2. `Database::open(path) -> Result<Database>`：创建父目录、建立 Pool、设置 PRAGMA（WAL / busy_timeout / foreign_keys）。
  3. `Database::migrate()`：`sqlx::migrate!("./migrations")`，幂等；失败返回带迁移版本号的错误。
  4. `Database::pool()` / `begin()` / `close()`；`DbError` 分类（`Unavailable` / `Busy` / `Constraint` / `Query`），`Display` 不含连接串与 SQL 明文（SQL 只能进 `source`，由调用方决定是否记日志）。
  5. workspace 成员与 `[workspace.dependencies]` 同步；`docs/workspace.md` §8.1 登记 `lib-db` 已创建。
  **技术要求**：`lib-db` 不含任何业务表名与业务 SQL；不使用 `sqlx::query!` 宏（`macros` 特性已关闭）。
  **测试要求**：无单元测试；行为在 P2-014 用例中验证（服务端启动即触发 `open` + `migrate`）。
  **验收标准**：
  - [ ] `cargo build` / `clippy -D warnings` / `fmt --check` 通过。
  - [ ] `Database` 的公开面只有 open / migrate / pool / begin / close（无泄漏的 `sqlx` 泛型池类型出现在业务签名里）。
  - [ ] 错误类型不包含连接串与 SQL 明文。
  **完成产物**：`lib-db` 可用。

- [ ] **P2-011 Schema 与迁移脚本**

  **目标**：落地 P2-002 定稿的 schema，保证迁移可重复执行且约束真正生效。
  **前置依赖**：P2-010。
  **涉及文件 / Crate**：`crates/lib-db/migrations/0001_init.sql`。
  **实施步骤**：
  1. 按 P2-002 的表结构写 `0001_init.sql`：三张表、主键、外键、`status` 的 `CHECK` 约束、`messages(session_id, seq)` 唯一约束、索引。
  2. 时间戳一律 `INTEGER`（Unix 毫秒），id 一律 `TEXT`（雪花 ID 字符串，`lib_core::id::next_id`）。
  3. 不使用 `AUTOINCREMENT`；不使用 SQLite 的隐式 rowid 作为业务 ID。
  4. 在 `lib-db` 的 `migrate.rs` 写清「迁移文件只追加，不修改已发布的迁移」。
  **技术要求**：SQL 保持可读，字段注释用 `--`；一个文件只做一次 schema 变更。
  **测试要求**：无；重复执行与约束生效在 P2-014 验证。
  **验收标准**：
  - [ ] 迁移文件覆盖全部字段且与 `agent-core` 类型一一对应（逐个字段核对记录）。
  - [ ] 外键与唯一约束显式声明。
  - [ ] 索引覆盖 `messages(session_id, seq)`、`runs(status)`、`runs(session_id, created_at_ms)`。
  **完成产物**：`0001_init.sql`。

- [ ] **P2-012 Session / Message / Run Repository**

  **目标**：把 P1 内存 `Store` 的全部数据访问改写成 Repository，并把「终态唯一提交」下沉为条件更新。
  **前置依赖**：P2-011、P2-007。
  **涉及文件 / Crate**：`crates/service-core/src/repository/`（`mod.rs` + 按聚合拆分）、`crates/service-core/Cargo.toml`。
  **实施步骤**：
  1. 组织：`repository/session.rs`、`repository/message.rs`、`repository/run.rs`，`repository/mod.rs` 汇成 `Repository { db: Arc<Database> }`。
  2. Session：`create`、`get`、`list`（按 `updated_at_ms` 倒序，消息数用 `COUNT` 子查询）。
  3. Message：`append(tx, message)`（分配 `seq` = 当前会话 `MAX(seq)+1`）、`list(session_id)`（按 `seq` 升序）。
  4. Run：`insert(tx, run)`、`get`、`transition(run_id, expect: &[RunStatus], next, error, usage, now) -> bool`（`UPDATE ... WHERE id=? AND status IN (...)`，返回受影响行数）、`count_active()`、`heal_orphans(now)`（`RUNNING → FAILED`、`QUEUED → CANCELLED`）。
  5. `complete_run` 走单事务：插入助手消息 + `UPDATE runs SET status='COMPLETED' ...` + 更新 `sessions.updated_at_ms`，任一步失败整体回滚。
  6. 错误映射：`DbError → SessionError::Internal`，SQL 与连接信息只进 `source`（日志），对外固定文案。
  **技术要求**：Repository 只被本 Crate 的 Service 调用；Service 层不出现 SQL 字符串；`seq` 分配必须与插入在同一事务，避免顺序空洞或重复。
  **测试要求**：无单元测试；由 P2-014 / P2-031 / P2-032 用例覆盖。
  **验收标准**：
  - [ ] `Store` 的能力全部有对应 Repository 方法，无遗漏（逐个方法核对）。
  - [ ] 终态更新一律是条件更新，影响 0 行时返回"已终态"而非报错。
  - [ ] 助手消息与 `COMPLETED` 在同一事务内。
  - [ ] Service 层无 SQL 字面量（静态检索为证）。
  **完成产物**：Repository 就绪。

- [ ] **P2-013 用 Repository 替换内存 Store 并改为显式注入**

  **目标**：`service-core` 变成「Service + Repository」，删除内存 Mock 存储；进程级单例改为启动期显式注入。
  **前置依赖**：P2-012。
  **涉及文件 / Crate**：`crates/service-core/src/session/{mod.rs, service.rs, error.rs}`、删除 `store.rs` 与 `holder/`；新增 `crates/lib-web/src/holder.rs`；`crates/lib-web/src/session/api.rs`、`crates/lib-web/src/probe/api.rs`；`crates/lib-bin-web/src/{lib.rs, server.rs}` 与 `Cargo.toml`。
  **实施步骤**：
  1. `SessionService::new(Arc<Repository>)`；方法签名与返回类型保持不变（`SessionError` 语义不变）。
  2. `subscribe_run` 暂时仍返回 Mock 事件流（真实执行在 P2-026 接入），只把存储换成 Repository，保证本任务后服务可跑通 P1 行为。
  3. 删除 `service-core/src/holder/`；在 `lib-web` 新增 `holder`：`OnceLock<Arc<SessionService>>` + `install(service) -> Result<()>` + `session() -> Result<&Arc<SessionService>>` + `try_session()`。
  4. `lib-bin-web::serve` 装配：`lib_core::directorys()` → `lib_db::Database::open(<data>/agent.db)` → `migrate()` → `heal_orphans()` → `Repository` → `SessionService` → `lib_web::holder::install(...)` → 启动服务；任一步失败给可读错误 + 退出码 1。
  5. `probe_health` 在未安装时返回 `runningRunCount = 0`（`lib-web/examples/api_probe_server.rs` 必须仍可独立启动）。
  6. `lib-bin-web` 的 `Cargo.toml` 增加 `lib-db`、`service-core`（`agent-runtime` 在 P2-026 加）。
  **技术要求**：Handler 的签名与 path 不变；`lib-web` 不写业务规则；`service-core` 不反向依赖 `lib-web`。
  **测试要求**：`make test ARGS="session"` 与 `make test ARGS="web"` 必须全绿（行为与 P1 一致）。
  **验收标准**：
  - [ ] 服务可从零启动并创建/写入/读取会话数据，数据落在 `<AGENT_HOME>/data/agent.db`。
  - [ ] 进程内不再有 `Mutex<HashMap>` 存储与进程级 `LazyLock` 单例。
  - [ ] `cargo run -p lib-web --example api_probe_server` 可启动，`probe/health` 返回 200。
  - [ ] P1 的 `session.*` / `web.*` 用例全部通过。
  **完成产物**：持久化存储接入，注入链路成形。

- [ ] **P2-014 持久化、重启恢复与状态自愈用例**

  **目标**：用端到端用例证明「重启不丢数据、迁移可重复、残留状态被自愈」。
  **前置依赖**：P2-013。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/persist.rs`（新增领域）、`crates/bin-tests/src/cases/mod.rs`、`crates/bin-tests/src/server.rs`（如需要「同 data 目录重启」辅助）。
  **实施步骤**：
  1. `persist.session`（Isolated）：创建会话 + 发消息 → 停止服务端 → 用同一 `AGENT_HOME` 重启 → `session/list`、`message/list` 内容与顺序一致。
  2. `persist.migration`（Isolated）：同一 data 目录连续启动两次，第二次启动不报错且数据保留。
  3. `persist.queued_heal`（Isolated）：发消息得到 `QUEUED` Run → 不订阅直接重启 → 重启后该 Run 为 `CANCELLED` 且 `finishedAtMs` 有值。
  4. 用例只通过生成的 SDK 访问；重启复用同一端口（`free_port` 后再绑定同一端口），不使用共享服务端。
  **技术要求**：`RUNNING` 残留自愈由 P2-034 覆盖，本任务不越界；用例失败信息必须包含实际状态与会话/消息条数。
  **测试要求**：本任务的三条用例即验证手段。
  **验收标准**：
  - [ ] `make test ARGS="persist"` 全部通过，报告写入 `target/tests/<运行 ID>/persist.md`。
  - [ ] 重启后消息顺序稳定（断言 `seq` 顺序与内容一一对应）。
  - [ ] 迁移重复执行不产生错误日志。
  **完成产物**：`persist.*` 用例。

---

## Batch D：Provider 与 Rig Adapter（P2-015 ~ P2-018）

> P2-015 → P2-016；P2-017 依赖 P2-009 + P2-015；**P2-018 可与 P2-015/016 并行**（只依赖 P2-005 的配置面结论）。

- [ ] **P2-015 新建 `agent-route`：`LlmProvider` 抽象与 Provider / Model 解析**

  **目标**：定义项目自己的 LLM 调用抽象，Rig 只作为其后的实现细节。
  **前置依赖**：P2-009、P2-001。
  **涉及文件 / Crate**：新增 `crates/agent-route/`（`src/{lib.rs, provider.rs, request.rs, error.rs, resolver.rs}`）。
  **实施步骤**：
  1. `LlmRequest { model, system: Option<String>, messages: Vec<LlmMessage>, max_output_tokens: Option<u64>, temperature: Option<f32> }`、`LlmMessage { role: LlmRole, content: String }`（`LlmRole` 只含 `User` / `Assistant`）。
  2. `LlmStreamEvent { Delta(String), Usage(TokenUsage), Done }`；`LlmStream = Pin<Box<dyn Stream<Item = Result<LlmStreamEvent, LlmError>> + Send + 'static>>`。
  3. `#[async_trait] pub trait LlmProvider: Send + Sync { async fn stream(&self, request: LlmRequest) -> Result<LlmStream, LlmError>; }`。
  4. `LlmError` 分类：`Config` / `Auth` / `RateLimited` / `Timeout` / `Stalled` / `Upstream` / `Decode` / `Cancelled`；`LlmError::public_message() -> &'static str` 给对外文案。
  5. `ProviderResolver`：由 `AgentSettings` + model id 解析出 `(ProviderConfig, ModelConfig)`；未知 model 返回 `LlmError::Config` 并携带可读信息（Service 层转 400）。
  6. 本任务**不引入 rig 依赖**（保证抽象先定、实现后接）。
  **技术要求**：`agent-route` 不依赖 `agent-runtime` / `agent-context*` / `lib-web*`；不出现数据库类型。
  **测试要求**：无单元测试；行为由 P2-016 与 P2-028 覆盖。
  **验收标准**：
  - [ ] `cargo tree -p agent-route` 中无 `rig`（本任务结束时）。
  - [ ] 抽象里没有任何第三方 LLM 类型。
  - [ ] `LlmError::public_message()` 对每类错误都有不含内部细节的文案。
  **完成产物**：抽象层就绪。

- [ ] **P2-016 Rig Adapter：OpenAI 兼容 + 流式 + Usage + 错误映射**

  **目标**：用 `rig-core` 实现 `LlmProvider`，把 Rig 类型完全关在本模块内。
  **前置依赖**：P2-015、P2-001。
  **涉及文件 / Crate**：`crates/agent-route/src/rig/{mod.rs, openai.rs}`、`crates/agent-route/Cargo.toml`、`Cargo.toml`（workspace 依赖）。可选 `crates/agent-route/src/http.rs`（idle 超时包装）。
  **实施步骤**：
  1. workspace 增加 `rig-core = { version = "0.46", default-features = false, features = ["reqwest", "rustls"] }`。
  2. `RigProvider::new(ProviderConfig, ModelConfig)`：`OpenAIConfig::new(api_key).with_base_url(base_url).with_route(Route::Chat).client()` → `OpenAI`；按 model id 取 `Chat` 模型。
  3. 请求映射：`LlmRequest.system` → `CompletionRequest::preamble`；`messages` → `Message::user/assistant`；`max_output_tokens` → `max_tokens`；`temperature` 透传。
  4. 流映射：`model.stream(request)` 的 `Item<StreamEvent>` 文本增量 → `Delta`；`Streamed::finish()` 的 `usage` → `Usage`；结束补 `Done`。
  5. 错误映射：`ProviderError::InvalidAuthentication → Auth`、`ProviderResponse` 按状态码分 `RateLimited`（429）/ `Upstream`（5xx）/ `Auth`（401/403）、`Http → Timeout|Upstream`、`Stalled → Stalled`、`Truncated → Upstream`、`Json → Decode`。
  6. idle 超时：逐 chunk `tokio::time::timeout(llm_idle_timeout)`，到点返回 `LlmError::Stalled` 并 drop 流。
  7. **实测并回填**（P2-001 列出的待实测项）：drop 流后连接是否被关闭、usage 是否在流尾给出、`Stalled` 默认阈值行为；结论写回 `decisions.md`。
  **技术要求**：`rig` 只出现在 `agent-route/src/rig/` 下（可在 P2-035 用静态检索断言）；base_url 与 key 不出现在错误消息里；禁止使用 `rig_agent`。
  **测试要求**：无单元测试；由 P2-018 的 Mock Server + P2-028 用例覆盖正常/401/500/超时/断连/非法 JSON。
  **验收标准**：
  - [ ] `agent-route` 之外的 crate 依赖树中无 `rig-core`（`cargo tree -i rig-core` 为证据）。
  - [ ] 五类错误（鉴权 / 限流 / 上游 5xx / 超时 / 解码）都能映射到稳定的 `LlmError` 分类。
  - [ ] 待实测项已回填结论，未确认的仍标 `OPEN`。
  **完成产物**：真实 LLM 调用能力。

- [ ] **P2-017 Provider 配置装载与密钥安全**

  **目标**：从 `config.toml` + 环境变量装载 Provider / Model / Agent 参数，并保证密钥不泄漏。
  **前置依赖**：P2-009、P2-015。
  **涉及文件 / Crate**：`crates/service-core/src/provider/{mod.rs, config.rs, error.rs}`、`crates/service-core/Cargo.toml`（`toml`）、`crates/lib-bin-cli/src/config.rs`（只读复用装载约定，不复制逻辑）。
  **实施步骤**：
  1. 配置结构：`[provider]` 与 `[[provider.models]]`、`[agent]`，字段与 `README.md` 的配置表一致；未出现时取默认值。
  2. 装载优先级：环境变量 > `config.toml` > 默认值（与 P1 D-012 的方向一致；命令行不参与 Provider 配置）。
  3. 校验：`base_url` 必须是 `http(s)://`；`default_model` 必须能在 `models` 中找到；`api_key` 为空时**启动成功但 `/api/probe/health` 保持 200**，首次调用返回可读的 `Provider 未配置` 失败（Run `FAILED`，不是 500）。
  4. 对外暴露只读 `AgentSettings`；`api_key` 只在 `agent-route` 构造 Provider 时 `expose()` 一次。
  5. 脱敏：`Secret` 的 `Debug`/`Display` 都是 `***`；日志里只允许 provider id 与 base_url 的 host。
  6. 未知模型（`session/create` 或 `message/create` 传入未配置的 model）→ 400 `模型不可用：<id>`，由 `service-core` 判定、`lib-web` 映射（沿用 `SessionError::Invalid`）。
  7. 记录配置文件权限提示（Windows 上无 POSIX 权限，文案只提示「配置含密钥，勿提交」）。
  **技术要求**：不新增 HTTP 接口；不做热更新（改配置需重启）；`service-core` 不直接依赖 `rig`。
  **测试要求**：密钥相关的检索断言（日志 / 响应体）由 P2-028、P2-035 覆盖。
  **验收标准**：
  - [ ] 环境变量覆盖生效（用例以 env 注入 mock server 地址）。
  - [ ] `api_key` 不出现在任何 `Debug` 输出、日志、响应体（可检索断言）。
  - [ ] 未知模型返回 400 且文案含模型 id。
  - [ ] 缺 `api_key` 时错误可读，Run 落 `FAILED`。
  **完成产物**：Provider 配置装载完成。

- [ ] **P2-018 `bin-tests` 内置 OpenAI 兼容 Mock Server**

  **目标**：让全部 P2 用例不依赖真实付费 API 就能验证真实 HTTP + SSE 行为。
  **前置依赖**：P2-005（配置面）、P2-002（无强依赖，可早做）。
  **涉及文件 / Crate**：`crates/bin-tests/src/mock_llm/`（`mod.rs`、`http.rs`、`scenario.rs`）、`crates/bin-tests/src/cases/mockllm.rs`、`crates/bin-tests/src/server.rs`（支持注入额外环境变量）、`crates/bin-tests/Cargo.toml`。
  **实施步骤**：
  1. 用 `tokio::net::TcpListener` 手写最小 HTTP/1.1（不引入 Web 框架；`bin-tests` 不得依赖 `lib-web` / `service-core`）。
  2. 端点：`POST /v1/chat/completions`（`stream: true` 走 SSE，`false` 走 JSON）、`GET /v1/models`、`GET /mock/last-request`（把最近一次请求体回吐，供用例断言 Context 组装结果）、`GET /mock/stats`（连接数、被关闭连接数，供取消用例断言）。
  3. 场景由请求里的 `model` 字段选择：`ok` / `auth_error`（401）/ `server_error`（500）/ `slow`（首帧前静默 N ms，供心跳用例）/ `stall`（中途静默，供超时用例）/ `truncate`（中途断连）/ `invalid_json`。
  4. `Server::start` 支持传入额外环境变量（`AGENT_PROVIDER_BASE_URL` 等），使被测服务端指向 mock server；mock server 端口动态申请。
  5. 自检用例 `mockllm.protocol`：断言 JSON 响应、SSE 分帧（`data:` 行 + 空行）、401、500、`slow` 的时序、`truncate` 的断连、`last-request` 回吐。
  **技术要求**：mock server 在测试进程内以 tokio 任务运行，随运行结束退出；不写入 `packages/` / 仓库任何持久文件；不得成为产品代码依赖。
  **测试要求**：`make test ARGS="mockllm"` 通过。
  **验收标准**：
  - [ ] `mockllm.protocol` 六种场景断言通过。
  - [ ] `cargo tree -p bin-tests` 中不出现 `lib-web` / `service-core` / `agent-runtime`。
  - [ ] 被测服务端可通过环境变量指向 mock server 并完成一次真实调用（在 P2-026 后由 `chat.*` 用例体现）。
  **完成产物**：可复用的 LLM Mock 基础设施。

---

## Batch E：Context 管理（P2-019 ~ P2-021）

> P2-019 → P2-020 → P2-021 串行。**E 批可与 C 批并行**。

- [ ] **P2-019 新建 `agent-context-core`**

  **目标**：定义 Context 的稳定抽象，供 P2 的 `agent-context` 与 P3 之后的 Tool / Skill / MCP 共同遵循。
  **前置依赖**：P2-006。
  **涉及文件 / Crate**：新增 `crates/agent-context-core/`（`src/{lib.rs, item.rs, context.rs, policy.rs, error.rs}`）。
  **实施步骤**：
  1. `ContextItem { source: ContextSource, role: Option<MessageRole>, text: String, metadata: ContextMetadata }`。
  2. `ContextSource { System, History, User, Tool, Skill, Memory }`（P2 只用前三个，其余取值留给后续阶段，**不实现具体能力**）。
  3. `Context { items: Vec<ContextItem> }` + 只读访问器与顺序保证说明。
  4. `ContextPolicy { max_messages, token_budget, safety_tokens }`、`ContextError { Overflow, Empty }`。
  5. `ContextContribution` trait（贡献点占位，P2 不实现提供方）。
  **技术要求**：不依赖 rig / axum / sqlx / tokio / `agent-context` / `agent-runtime`；纯数据与同步代码。
  **测试要求**：无单元测试；由 P2-020 / P2-031 覆盖。
  **验收标准**：
  - [ ] `cargo tree -p agent-context-core` 只有 `agent-core` + `lib-core`。
  - [ ] 不出现 async 函数。
  - [ ] `ContextSource` 的后续取值未引入任何实现代码。
  **完成产物**：Context 抽象层。

- [ ] **P2-020 新建 `agent-context`：Context Builder**

  **目标**：从 System Prompt + 会话历史 + 当前用户消息组装 Context，顺序与角色映射正确。
  **前置依赖**：P2-019。
  **涉及文件 / Crate**：新增 `crates/agent-context/`（`src/{lib.rs, builder.rs, history.rs, prompt.rs, error.rs}`）。
  **实施步骤**：
  1. `ContextBuilder::new(policy, estimator)`，`build(BuildInput { system: Option<String>, history: &[Message], current: &Message }) -> Result<Context, ContextError>`。
  2. 组装顺序固定：`System` → `History`（按 `seq` 升序）→ `User`（当前消息，必须在最后）。
  3. 角色映射：`MessageRole::User → User`、`Assistant → Assistant`、`System → System`；`Tool` 在 P2 直接过滤（不报错）。
  4. `prompt.rs`：默认 System Prompt 常量（说明「你是 lingting agent 的助手」等最小内容），允许配置覆盖（配置项在 P2-017 的 `[agent]` 中预留 `system_prompt`）。
  5. Builder **不读数据库**：历史由调用方传入。
  **技术要求**：不依赖 `agent-runtime` / `agent-route` / `lib-web*`；不发起 IO。
  **测试要求**：无单元测试；顺序断言由 P2-031 的 `chat.multiturn`（读 mock 的 `last-request`）覆盖。
  **验收标准**：
  - [ ] 组装顺序可在代码里一眼读出（三个阶段顺序固定，无隐式插入）。
  - [ ] 历史严格按 `seq` 升序，不依赖调用方的顺序。
  - [ ] `ContextError` 对空历史（只有当前消息）不报错。
  **完成产物**：Context 组装实现。

- [ ] **P2-021 Token 预算与历史裁剪**

  **目标**：按 P2-003 的算法实现预算计算与裁剪，并把预算注入点做成可替换。
  **前置依赖**：P2-020、P2-003。
  **涉及文件 / Crate**：`crates/agent-context/src/{estimator.rs, budget.rs}`、`crates/agent-context-core/src/policy.rs`。
  **实施步骤**：
  1. `TokenEstimator` trait（`fn estimate(&self, text: &str) -> usize`）+ `HeuristicEstimator`（ASCII 4 字符 ≈ 1 token，非 ASCII 1 字符 ≈ 1 token，每条消息 +4）。
  2. `ContextBudget { context_window, max_output_tokens, safety_tokens }` → `available()`；调用方从 `ModelConfig` 构造。
  3. 裁剪：先按 `max_messages` 截断最旧消息，再逐条丢弃最旧历史直到 `estimate(context) <= available()`；系统提示与最新用户消息永不被裁。
  4. 超限：最新用户消息单条超预算 → `ContextError::Overflow`，由 Runtime 转成 Run `FAILED`（P2-023）。
  5. 参数全部来自 `ContextPolicy` / `ContextBudget`，代码中不出现魔法数字（固定开销 4 与公式常量集中定义）。
  **技术要求**：裁剪过程不修改入参 `history`；返回的 `Context` 明确记录被裁掉的消息数（metadata），供日志使用。
  **测试要求**：无单元测试；P2-031 的 `chat.truncate` 覆盖。
  **验收标准**：
  - [ ] 算法与 D-P2-012 / D-P2-013 完全一致。
  - [ ] 系统提示与最新用户消息在任何配置下都保留（代码路径可读 + 用例）。
  - [ ] 估算器可通过构造参数替换（trait object 或泛型均可，但调用点只有一处）。
  **完成产物**：预算与裁剪能力。

---

## Batch F：Agent Runtime（P2-022 ~ P2-026）

> P2-022 → P2-023 → {P2-024, P2-025} → P2-026。P2-024 与 P2-025 可并行（都改 `agent-runtime`，需协调同一批文件）。

- [ ] **P2-022 新建 `agent-runtime`：Run 注册表、并发闸门与 Session 租约**

  **目标**：建立 Runtime 骨架与调度约束，保证「一个 Run 一个生产方」「同 Session 串行」「全局并发有上限」。
  **前置依赖**：P2-007、P2-004。
  **涉及文件 / Crate**：新增 `crates/agent-runtime/`（`src/{lib.rs, runtime.rs, registry.rs, scheduler.rs, error.rs}`），`RunEventStream` 迁入（见 D-P2-019）。
  **实施步骤**：
  1. `RunEventStream` 从 `crates/service-core/src/session/stream.rs` 迁入 `agent-runtime/src/event/stream.rs`（对外形态保持 `recv()` 不变）。
  2. `RunRegistry { active: Mutex<HashMap<String, RunHandle>> }`，`RunHandle { cancel: CancellationToken, session_id, started_at_ms }`；`insert_once(run_id)` 原子占位，重复返回 `RuntimeError::AlreadySubscribed`。
  3. `SessionLeases`：`DashMap<String, Arc<tokio::sync::Mutex<()>>>`（公平等待 → FIFO）；获取顺序固定「先 Session 租约，后全局 `Semaphore`」。
  4. `AgentRuntime::subscribe(run: Run) -> Result<RunEventStream, RuntimeError>`：占位 → 取租约 → 取信号量 → 转 `RUNNING` → 起执行任务；排队超时落 `FAILED`。
  5. `cancel(run_id)`、`shutdown()`（广播）、`running_count()`（内存视角，仅供日志；对外的 `runningRunCount` 仍取数据库，见 P2-026）。
  6. **禁止** `service-core → agent-runtime` 反向引用（否则形成 Arc 环与依赖环）：Runtime 持有 `Arc<SessionService>`，Service 不持有 Runtime。
  **技术要求**：注册表在任务终态必须清理（RAII guard）；重复订阅的错误语义对应 P1 的 400 文案。
  **测试要求**：无单元测试；由 P2-033 覆盖。
  **验收标准**：
  - [ ] 同一 `run_id` 只能 `insert_once` 一次。
  - [ ] 同一 Session 的第二个 Run 在第一个终态前不进入 `RUNNING`。
  - [ ] `cargo tree` 无循环；`service-core` 不依赖 `agent-runtime`。
  **完成产物**：调度骨架。

- [ ] **P2-023 `RunExecutor` 主流程与事务一致性**

  **目标**：实现单轮 Agent Loop：`RUNNING → 组装 Context → 调 LLM → 累积 → 终态事务`。
  **前置依赖**：P2-022、P2-016、P2-021、P2-012。
  **涉及文件 / Crate**：`crates/agent-runtime/src/{executor.rs, outcome.rs}`。
  **实施步骤**：
  1. 执行步骤（与 D-P2-008 严格一致）：
     `QUEUED→RUNNING`（条件更新）→ 发 `run_started` → 读会话历史 + 当前用户消息 → `ContextBuilder::build` → `ProviderResolver` 解析 model → `LlmProvider::stream` → 循环累积增量并发 `message_delta` → 收尾。
  2. 成功收尾：单事务 `{INSERT assistant message, UPDATE run → COMPLETED + usage + finished_at, UPDATE session.updated_at}`；事务成功后发 `run_completed`；事务失败则改用失败路径。
  3. 失败收尾：`UPDATE run → FAILED`（`error` = `LlmError::public_message()` 或 `ContextError::Overflow` 的固定文案）→ 发 `run_failed`；**不写助手消息**。
  4. 上下文超限、未知模型、缺 key 都走失败路径（Run `FAILED`），不返回 HTTP 错误。
  5. 清理：无论结局如何都移除注册表条目、释放信号量与租约。
  **技术要求**：不在循环中写库；`Run.error` 不含 base_url、key、堆栈；助手消息 id 在发 `run_started` 之前生成（与 P1 一致：`run_started.messageId` 有值）。
  **测试要求**：无单元测试；由 P2-031 / P2-032 覆盖成功与失败路径。
  **验收标准**：
  - [ ] 成功路径：`run_completed` 到达后 `run/get` 即 `COMPLETED` 且有 usage，`message/list` 多一条助手消息。
  - [ ] 失败路径：`run_failed` 到达后 Run 为 `FAILED` 且无助手消息。
  - [ ] `run_started` 在 `RUNNING` 已提交之后发出（用例可在事件到达后立即查 `run/get` 验证）。
  **完成产物**：可执行的真实 Run。

- [ ] **P2-024 RunEvent 生产：流式增量、序号与心跳**

  **目标**：把事件生产从 P1 的 Mock 迁到 Runtime，并让心跳在真实长等待中真正生效。
  **前置依赖**：P2-023。
  **涉及文件 / Crate**：`crates/agent-runtime/src/event/{mod.rs, producer.rs}`。
  **实施步骤**：
  1. 移植 P1 `Producer` 的公共字段填充与 `seq` 单调规则（从 0 开始连续递增，同时作为 SSE 的 `id`）。
  2. 心跳：等待超过 `stream_heartbeat_ms`（默认 15000）补一条 `heartbeat`（只带公共字段）；实现为「等待增量时按心跳间隔切片睡眠」。
  3. 背压：`mpsc::channel(32)`（与 P1 一致），消费方断开即 `sender.closed()` 返回。
  4. 空增量不发送 `message_delta`（避免空帧）。
  5. 事件与状态一致性：`run_completed` 只在事务提交后发送；`run_failed` 只在失败终态提交后发送。
  **技术要求**：事件字段集合与 P1 完全相同（不得新增字段）；`created_at_ms` 逐条取当前时间。
  **测试要求**：P2-027（心跳）与 P2-031（正常流）覆盖。
  **验收标准**：
  - [ ] `seq` 从 0 连续无空洞，`run_completed` 为最后一条（除 `server_shutdown` 场景）。
  - [ ] 心跳只在空闲超阈值时出现，且不打乱 `seq`。
  - [ ] 无空 `delta` 事件。
  **完成产物**：真实事件流。

- [ ] **P2-025 Cancellation：取消传播、断线与关闭联动**

  **目标**：让「取消 Run」真正中断模型调用，并保证终态只提交一次。
  **前置依赖**：P2-023。
  **涉及文件 / Crate**：`crates/agent-runtime/src/{cancel.rs, executor.rs}`、`crates/lib-web/src/session/api.rs`（`run_cancel` 追加 Runtime 通知）、`crates/lib-bin-web/src/shutdown.rs`（关闭钩子联动）。
  **实施步骤**：
  1. 执行循环用 `tokio::select!` 监听三路：下游增量、`token.cancelled()`、`sender.closed()`。
  2. `api/run/cancel`：先由 `service-core` 做条件更新（数据库是权威），成功后调用 `runtime.cancel(run_id)` 停止流；更新 0 行 → 维持 P1 的 400 语义。
  3. 断线：`sender.closed()` 等价于取消——`UPDATE → CANCELLED`（条件更新，影响 0 行则不动），丢弃已累积内容，不发任何事件。
  4. 服务关闭：`lib-bin-web` 的关闭钩子调用 `runtime.shutdown()` → 广播取消 → 执行器 `UPDATE → CANCELLED` → 发 `server_shutdown`（`status=CANCELLED` + 可读 `error`）。
  5. 终态唯一提交：所有终态写入都是条件更新；竞态时「先提交者获胜，后者不写不改」。
  6. Panic 兜底：执行任务用 `JoinHandle` 结果 / `catch_unwind` 捕获，panic 时尽力把 Run 落 `FAILED`（`error` = 内部错误固定文案），并确保注册表清理。
  **技术要求**：取消必须是「真正中断 HTTP 流」（drop `LlmStream`），不允许只停 SSE；取消后 1 秒内释放信号量与租约。
  **测试要求**：P2-032 三条用例（取消 / 断线 / 关闭）+ P2-018 的 `/mock/stats` 断言连接被关闭。
  **验收标准**：
  - [ ] 取消后 Run 为 `CANCELLED`，`finishedAtMs` 有值，无助手消息。
  - [ ] mock server 侧观察到连接被关闭（不是等流自然结束）。
  - [ ] 取消与完成同时发生时终态唯一，不会被覆盖。
  - [ ] 服务关闭时事件流以 `server_shutdown` 收尾且连接关闭。
  **完成产物**：可取消的执行链路。

- [ ] **P2-026 接入 Server：替换 Mock 生产方与启动装配**

  **目标**：把 Server 的真实执行链路接通，Mock 事件流与 Mock 回复彻底下线。
  **前置依赖**：P2-024、P2-025、P2-013、P2-017。
  **涉及文件 / Crate**：`crates/lib-web/src/session/{api.rs, mod.rs}`（删除 `stream.rs` 生产的 Mock 引用）、`crates/lib-web/src/holder.rs`、`crates/lib-bin-web/src/{server.rs, shutdown.rs}` 与 `Cargo.toml`；删除 `crates/service-core/src/session/stream.rs` 中的 Mock 回复逻辑。
  **实施步骤**：
  1. `lib-web` 的 `run_stream` handler：校验 Run 存在且 `QUEUED`（保持 P1 文案）→ `holder::runtime().subscribe(run)` → 用迁入 `agent-runtime` 的 `RunEventStream` 组装 SSE（`session/stream.rs` 只改类型来源）。
  2. `lib-bin-web::serve` 装配顺序：配置 → `lib-db` → `heal_orphans` → Repository → `SessionService` → `ProviderResolver` → `AgentRuntime` → `holder::install(service, runtime)` → 注册关闭钩子（含 `runtime.shutdown()`）。
  3. `probe_health.runningRunCount` 改为数据库统计（`runs` 中 `QUEUED`/`RUNNING` 数量）。
  4. 删除 P1 的 Mock 回复文案与定时代码路径；`service-core` 不再有 `tokio` 的定时生产任务。
  5. `lib-bin-web` 的 `Cargo.toml` 增加 `agent-runtime`；`lib-web` 增加 `agent-core`、`agent-runtime`。
  **技术要求**：Handler 的 path、入参、出参与错误映射不变；`lib-web` 不承载业务规则；关闭钩子必须在启动前注册（沿用 P1 顺序）。
  **测试要求**：`make test ARGS="session"`、`make test ARGS="web"`、`make test ARGS="probe"` 必须全绿。
  **验收标准**：
  - [ ] 真实调用 Mock LLM 完成一次流式对话，`run_completed` 后 `message/list` 有助手消息。
  - [ ] 仓库内不再有 Mock 回复文案（检索为空）。
  - [ ] `probe/health` 的 `runningRunCount` 反映真实进行中数量。
  - [ ] 服务优雅关闭时进行中的 Run 收尾为 `CANCELLED` 并收到 `server_shutdown`。
  **完成产物**：P2 主链路接通。

---

## Batch G：SSE 与客户端集成（P2-027 ~ P2-030）

> P2-027 / P2-028 / P2-029 都依赖 P2-026；P2-030 无代码依赖。

- [ ] **P2-027 E-3 心跳验证**

  **目标**：把 P1 遗留的 E-3（心跳在长等待时补发）变成可自动验证的用例。
  **前置依赖**：P2-026、P2-018。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/stream.rs`（新增领域）、`crates/bin-tests/src/cases/mod.rs`、`crates/bin-tests/src/server.rs`（env 注入）。
  **实施步骤**：
  1. 用例 `stream.heartbeat`（Isolated）：`AGENT_STREAM_HEARTBEAT_MS=300`，mock 场景 `slow`（首帧前静默 1.5s）。
  2. 断言：首条为 `run_started`；其后至少一条 `heartbeat`；`heartbeat` 只带公共字段（`delta`/`status`/`error` 均为空）；`seq` 连续递增；最后 `run_completed`。
  3. 断言默认值未变：代码中默认 15000ms（静态核对，写入用例的可读提示或报告）。
  **技术要求**：不改变生产默认行为（15s）；心跳间隔只通过环境变量调整（D-P2-020）。
  **测试要求**：`make test ARGS="stream.heartbeat"` 通过，用例耗时 < 10s。
  **验收标准**：
  - [ ] 心跳事件在 `run_started` 与首个 `message_delta` 之间出现。
  - [ ] 心跳不打乱 `seq`，不携带业务字段。
  - [ ] 用例稳定（连续运行 3 次通过）。
  **完成产物**：`stream.heartbeat` 用例 + P1 E-3 关闭。

- [ ] **P2-028 E-4 `run_failed` 验证**

  **目标**：验证真实失败分支：鉴权失败、上游 5xx、空闲超时、流中断都产出 `run_failed` 且 Run 落 `FAILED`。
  **前置依赖**：P2-026、P2-018。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/stream.rs`。
  **实施步骤**：
  1. `stream.failed_auth`：mock 场景 `auth_error` → 事件流出现 `run_failed`，`error` 可读且不含 key；`run/get` 为 `FAILED`；无助手消息。
  2. `stream.failed_upstream`：`server_error` → 同上，`error` 区分「上游错误」。
  3. `stream.failed_timeout`：`AGENT_LLM_IDLE_TIMEOUT_MS=500` + 场景 `stall` → `run_failed`，`error` 为超时文案。
  4. `stream.failed_truncate`：场景 `truncate` → `run_failed`（不挂死，事件流正常结束）。
  5. 断言 `run_failed` 是最后一条事件（除关闭场景），事件流随后结束。
  **技术要求**：错误文案断言到「可读且稳定」粒度，不断言完整句子之外的实现细节；断言 `error` 不包含 base_url、api_key、文件路径。
  **测试要求**：四条用例通过，单个用例耗时 < 15s。
  **验收标准**：
  - [ ] 四类失败各一条用例通过。
  - [ ] 失败 Run 无助手消息、有 `finishedAtMs`。
  - [ ] `error` 中无密钥 / 路径 / 堆栈（检索断言）。
  **完成产物**：`stream.failed_*` 用例 + P1 E-4 / R-3 关闭。

- [ ] **P2-029 客户端失败展示与 W-1 控制台断言**

  **目标**：让三个客户端能显示真实失败与排队状态；补 P1 遗留的浏览器 console error 断言。
  **前置依赖**：P2-026。
  **涉及文件 / Crate**：`crates/lib-bin-cli/src/run.rs`、`crates/lib-bin-tui/src/{state.rs, worker.rs}`、`lingting-agent-ui/src/pages/chat/*`、`lingting-agent-ui/tests/e2e/*.spec.ts`、`crates/bin-tests/src/cases/{web/e2e.rs, cli.rs}`。
  **实施步骤**：
  1. CLI：`run` 收到 `run_failed` → 打印一行可读错误 + 非 0 退出码（`CommandError::runtime`）。
  2. TUI：失败态显示错误文案并可重试；排队状态（收到 `heartbeat` 且尚无 `message_delta`）显示「等待执行…」。**不改 Transport 与 SDK 调用**。
  3. WebUI：失败时显示错误提示条（复用 P1 的 `errors.ts` 与既有提示组件），停止生成后保留已收内容。
  4. W-1：Playwright 用例收集 `page.on("console")` 的 error 级别消息，断言为空（若有已知噪音，白名单必须显式列出并注明原因）。
  5. 用例 `web.console`（转发 Playwright）与 `cli.failed`（CLI 失败退出码与文案）。
  **技术要求**：三端都不允许手写 `api/…` 字面量（沿用 `web.sdk-only`）；不引入新依赖。
  **测试要求**：`make test ARGS="cli"`、`make test ARGS="web"`、`pnpm exec vp test run`、`pnpm exec playwright test` 通过。
  **验收标准**：
  - [ ] CLI 失败路径退出码非 0 且只输出一行错误。
  - [ ] TUI 失败与排队态可读、可恢复。
  - [ ] Playwright 断言 console error 为空（白名单为空）。
  **完成产物**：客户端失败展示 + P1 W-1 关闭。

- [ ] **P2-030 P1 遗留 A-1（生成物过期检测）结案**

  **目标**：给 P1-020 遗留的 A-1 一个明确归属与结论，不再悬空。
  **前置依赖**：无。
  **涉及文件 / Crate**：`.steps/p2/decisions.md`（D-P2-022）。
  **实施步骤**：
  1. 复述事实：生成物不入库（P1 D-006），门禁每次都从源码重建，因此不存在「生成物与定义不一致」的可检测状态；`make gen-check` 的等价判据是「缺生成物 / 生成失败 / 产物不完整 / 生成代码不能编译 → 非 0」。
  2. 结论：A-1 的原始表述 `REJECTED`；等价门禁保留（`make gen-check`）。
  3. 在 `.steps/p2/decisions.md` 记录结案，并在 `.steps/README.md` 的进度表注明（**不修改 `p1/decisions.md` 的原始结论**）。
  **技术要求**：只写结论与理由，不新增门禁步骤（如需新增必须单独立项并说明代价）。
  **测试要求**：无。
  **验收标准**：
  - [ ] 决策记录中 A-1 状态为 `REJECTED` 且理由完整。
  - [ ] 未修改 P1 已完成任务的状态与 P1 决策原文。
  **完成产物**：A-1 结案记录。

---

## Batch H：端到端验收（P2-031 ~ P2-035）

> P2-031 ~ P2-034 依赖 P2-026（及各自的 Mock 场景），可并行；P2-035 必须在全部之后。

- [ ] **P2-031 多轮对话与 Context 生效**

  **目标**：证明历史确实进入模型请求、顺序正确、超预算会裁剪。
  **前置依赖**：P2-026、P2-021、P2-018。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/chat.rs`。
  **实施步骤**：
  1. `chat.multiturn`：同一会话连续 3 轮对话 → 通过 mock 的 `GET /mock/last-request` 断言第 3 次请求包含前两轮的 user + assistant 消息，顺序与 `message/list` 一致，system prompt 在最前。
  2. `chat.truncate`：`AGENT_CONTEXT_MAX_MESSAGES=2` 或极小 `context_window` → 断言请求里历史被裁剪、系统提示仍在、最新用户消息在最后。
  3. `chat.overflow`：最新用户消息超预算 → Run `FAILED`，`error` 为上下文超限文案，且没有发起上游请求（mock 的 `stats` 中请求数为 0）。
  **技术要求**：断言对象是**真实请求体**，不是回复文本；用例不得依赖 Mock 的固定回复内容。
  **测试要求**：三条用例通过。
  **验收标准**：
  - [ ] 历史顺序、system prompt 位置、当前消息位置三项断言通过。
  - [ ] 裁剪生效且系统提示未被裁掉。
  - [ ] 超限时不产生上游请求。
  **完成产物**：`chat.*` 用例。

- [ ] **P2-032 取消 / 断线 / 服务关闭**

  **目标**：验证三条终止路径的终态、事件与资源释放。
  **前置依赖**：P2-026、P2-025、P2-018。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/run.rs`。
  **实施步骤**：
  1. `run.cancel`：`slow` 场景流式中调用 `api/run/cancel` → 返回 `CANCELLED`；事件流停止且无 `run_completed`；`run/get` 为 `CANCELLED`；无助手消息；重复取消返回 400（P1 语义）。
  2. `run.disconnect`：消费前 N 条事件后丢弃流 → 800ms 内 Run 变 `CANCELLED`；mock 的 `stats` 显示连接被关闭。
  3. `run.shutdown`（Isolated）：流式中调用关闭探针 → 末条事件为 `server_shutdown`（`status=CANCELLED` + 可读 `error`），事件流结束；服务端日志无 panic。
  **技术要求**：不得只用「停止读取 SSE」当作取消证据，必须断言服务端状态与 mock 侧连接关闭。
  **测试要求**：三条用例通过；`run.shutdown` 复用 P1 的关闭探针（debug 构建）。
  **验收标准**：
  - [ ] 三条路径终态正确、`finishedAtMs` 有值。
  - [ ] mock 侧连接确实被关闭（取消与断线各一次）。
  - [ ] 关闭后事件流立即结束，不挂死。
  **完成产物**：`run.*` 用例。

- [ ] **P2-033 多 Session 并发与同 Session 串行**

  **目标**：验证隔离、串行与全局并发上限。
  **前置依赖**：P2-026、P2-022。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/run.rs`。
  **实施步骤**：
  1. `run.concurrency`：两个会话各发一条并同时订阅 → 两条流都拿到完整回复；`message/list` 各自只有自己的消息；事件 `sessionId` 归属正确。
  2. `run.session_serial`：同一会话连续发两条并同时订阅 → 第二个 Run 在第一个终态前保持 `QUEUED`（`run/get` 断言），其事件流先收到 `heartbeat` 再收到 `run_started`；最终两条都 `COMPLETED`，消息顺序为 user1/assistant1/user2/assistant2。
  3. `run.queue_limit`（Isolated）：`AGENT_MAX_CONCURRENT_RUNS=1`，三个会话并发 → 全部完成且**同时进行中的 Run 数不超过 1**（通过 `probe/health.runningRunCount` 轮询采样断言）。
  **技术要求**：不得断言全局计数（共享服务端原则）；断言只能落在本用例创建的会话上；`run.queue_limit` 用 Isolated。
  **测试要求**：三条用例通过，总耗时可控（`slow` 场景参数收敛）。
  **验收标准**：
  - [ ] 两会话消息零串扰。
  - [ ] 同会话严格串行且消息顺序正确。
  - [ ] 并发上限生效（采样值 ≤ 配置值）。
  **完成产物**：`run.concurrency` / `run.session_serial` / `run.queue_limit`。

- [ ] **P2-034 重启恢复（含 `RUNNING` 自愈）**

  **目标**：验证真实执行中崩溃/重启后的状态自愈与历史可继续。
  **前置依赖**：P2-026、P2-013、P2-014。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/persist.rs`。
  **实施步骤**：
  1. `persist.running_heal`（Isolated）：`slow` 场景启动 Run 并收到 `run_started` → 直接杀掉服务端进程 → 同 `AGENT_HOME` 重启 → 该 Run 为 `FAILED`（`error` = 「服务重启，执行已中断」）、无助手消息、`finishedAtMs` 有值。
  2. `persist.resume_chat`（Isolated）：重启后在**同一会话**继续发一条消息并完成 → 历史包含重启前的消息，顺序正确。
  3. 断言重启后 `probe/health` 可访问、`session/list` 数据完整。
  **技术要求**：杀进程用 `Server::stop` 的强制终止路径（不得走优雅关闭，否则测的是关闭而非崩溃）；用例必须回收子进程。
  **测试要求**：两条用例通过。
  **验收标准**：
  - [ ] `RUNNING` 残留被自愈为 `FAILED` 且有可读原因。
  - [ ] 重启后同一会话可继续多轮对话，历史顺序正确。
  - [ ] 无残留 `RUNNING` 记录（`runs(status)` 检索为空）。
  **完成产物**：`persist.running_heal` / `persist.resume_chat`。

- [ ] **P2-035 契约一致性、SDK 生成与阶段验收报告**

  **目标**：完成阶段收口：门禁全绿、契约零变更被证明、验收报告成文。
  **前置依赖**：P2-027 ~ P2-034。
  **涉及文件 / Crate**：`docs/reports/p2-035-acceptance.md`（新增）、`.steps/README.md`、`.steps/p2/README.md`（状态）。
  **实施步骤**：
  1. 跑并记录：`make gen`、`make gen-check`、`make check`、`make ui-check`、`make test ARGS="--all"`、`cd lingting-agent-ui && pnpm exec vp test run`、`pnpm exec playwright test`。
  2. 契约一致性：19 条接口的 method/path、23 个 DTO 的字段名/类型/可选性、6 个 `RunEventKind` 取值逐项比对（沿用 P1-020 A-5 的方法），记录零差异结论。
  3. `cargo tree` 复核：无循环、`agent-route` 是唯一含 `rig-core` 的 crate、`agent-core` 无基础设施依赖、未创建被禁止的 Crate。
  4. 记录观察数据（不设阈值）：release binary 体积、冷启动时间、`make ci` 失败步骤与原因。
  5. 按 `acceptance.md` 的 14 类逐条写结论与证据，偏差单列；更新 `.steps/README.md` 进度与 `.steps/p2/README.md` 状态。
  **技术要求**：报告只写结论与证据，不贴大段日志；偏差必须写处置与归属阶段。
  **测试要求**：全部命令通过（`make ci` 若因 Framework `path` 依赖失败，单独记录，不作为阻塞）。
  **验收标准**：
  - [ ] 14 类验收条目全部有结论与证据。
  - [ ] SDK 两侧契约零差异，`make gen-check` 通过。
  - [ ] 无遗留 P0 问题；偏差均有归属。
  **完成产物**：`docs/reports/p2-035-acceptance.md` + 路线图状态更新。
