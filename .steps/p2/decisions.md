# P2 技术决策记录

**状态：Batch A 已定稿（2026-11-20），后续批次按需增补。** 本文件记录 P2 的阶段级决策。编号 `D-P2-xxx`；被否决与被修订的方案保留记录，不删除。

> 上一版记录（35 个任务时期）中，凡以「Run 由 `api/run/stream` 订阅触发」为前提的结论，已在 P1.1 后作废，并在正文以 **P1.1 修订** 标注；本版新增 D-P2-028 / D-P2-029。

任务分解见 [tasks.md](tasks.md)，判据见 [acceptance.md](acceptance.md)，阶段边界见 [README.md](README.md)。

每项记录包含：问题 / 背景与证据 / 可选方案 / 决策 / 理由 / 状态 / 承接任务。

状态定义（与 P1 一致）：

| 状态 | 含义 |
|------|------|
| `DECIDED` | 已确定，可直接执行 |
| `DECIDED (待验证)` | 方向已定，但依赖某个验证任务确认细节 |
| `OPEN` | 未确定，动手前必须讨论，或对应任务中必须先做实验再定 |
| `REJECTED` | 已否决（保留记录，不删除） |
| `DEFERRED` | 明确推迟到后续阶段 |

---

## 决策索引

| ID | 主题 | 状态 |
|---|---|---|
| D-P2-001 | LLM 接入层：`rig-core` 0.46 + 自有 Adapter，不用 Rig 的 Agent Loop | DECIDED |
| D-P2-002 | 持久化：`sqlx` 而非 `rusqlite` | DECIDED |
| D-P2-003 | `sqlx` feature 组合（不启用 `macros`） | DECIDED |
| D-P2-004 | 数据库位置、连接参数与测试隔离 | DECIDED |
| D-P2-005 | 消息顺序用 `seq` 列；会话消息数用 `COUNT` | DECIDED |
| D-P2-006 | Run 执行由 `api/run/stream` 订阅触发 | **REJECTED**（P1.1 移除该端点，见 D-P2-028） |
| D-P2-007 | 同 Session 串行 + 全局并发上限 + 排队超时 | DECIDED（P1.1 修订：串行由执行队列保证） |
| D-P2-008 | 持久化时序：流式不落库、终态与助手消息同事务 | DECIDED（P1.1 修订：终态事件由执行层发） |
| D-P2-009 | 终态唯一提交由数据库条件更新裁决 | DECIDED |
| D-P2-010 | Cancellation 归属与传播 | DECIDED（P1.1 修订：三种终止来源） |
| D-P2-011 | 启动自愈：`RUNNING → FAILED`、`QUEUED → CANCELLED` | DECIDED |
| D-P2-012 | Context 预算来源与 Token 估算 | DECIDED |
| D-P2-013 | 历史裁剪策略与超限失败 | DECIDED |
| D-P2-014 | P2 不新增接口 / DTO 字段 / 事件枚举值 | DECIDED |
| D-P2-015 | 未知模型返回 400 | DECIDED |
| D-P2-016 | Provider 配置只走文件与环境变量，不新增 API | DECIDED |
| D-P2-017 | API Key 明文存配置文件 + 脱敏规则 | DECIDED |
| D-P2-018 | Crate 新增范围与四条依赖边调整 | DECIDED（P1.1 修订：`RunEventStream` 不再迁移） |
| D-P2-019 | 进程级访问点：`holder` 改显式注入 | DECIDED（P1.1 修订） |
| D-P2-020 | 心跳间隔可配置（`AGENT_STREAM_HEARTBEAT_MS`） | **REJECTED**（P1.1 用固定常量覆盖） |
| D-P2-021 | 测试只在 `bin-tests`；LLM 用内置 TCP Mock Server | DECIDED |
| D-P2-022 | P1 遗留 A-1（生成物过期检测） | REJECTED |
| D-P2-023 | 流式部分内容持久化 | DEFERRED (P6) |
| D-P2-024 | 事件回放与断点续传 | DEFERRED (P6) |
| D-P2-025 | Provider 配置 CRUD 接口 | DEFERRED (未排期) |
| D-P2-026 | API Key 加密存储 / 系统密钥库 | DEFERRED (P6) |
| D-P2-027 | Framework 依赖与 CI 策略（P2 不变） | DECIDED |
| D-P2-028 | Run 触发点继承 P1.1；执行端口由 `agent-runtime` 实现并注入 | DECIDED |
| D-P2-029 | 终态事件归属与「事件文件 / 数据库」事实源分工 | DECIDED |
| D-P2-030 | SQLite 表结构草案与迁移执行方式（不用 `sqlx::migrate!`） | DECIDED |

---

## D-P2-001 LLM 接入层：`rig-core` 0.46 + 自有 Adapter，不用 Rig 的 Agent Loop

- **问题**：P2 如何接入 Rig，才既不重复实现 Provider 协议，又不让 Rig 侵占自研 Agent Loop 的职责？
- **背景（已核实，源码：本地 registry `rig-core-0.46.0`）**：
  - `rig` facade crate 的 `default` 含 `agent`（`dep:rig-agent`），而 `rig-agent` 就是 Rig 自己的 Agent 运行时（`Agent` / `AgentBuilder` / `AgentRun` / tool loop）；`rig-core` 的 `default = ["derive", "rustls"]`，`reqwest` 是独立特性。
  - 构造（`client/openai.rs:25-70`、`providers/openai/wire.rs:580-800`）：`OpenAIConfig::new(api_key)`（或 `with_key(&dialect, key)`）→ `with_base_url(..)` / `with_route(Route)` → `.client()`（共享 reqwest 客户端）或 `.connect(http)` → `OpenAI`；`OpenAI::chat(model)` 固定走 Chat Completions wire（`POST /chat/completions`），`OpenAI::completion(model)` 走配置的 `completion_route`（可能落到 `/responses`）。
  - 调用（`driver.rs:400-436`）：`Model::{call, stream}`；`call(impl Into<Request<W>>)` → `Result<Response<W>, ProviderError>`，`stream(..)` → `Result<Streamed<W::Op>, ProviderError>`；`OpenAI::chat(..)` 得到的正是 `driver::Model<Chat>`。
  - 流式（`streaming/mod.rs:92-300`）：`Streamed<Op>` 实现 `Stream<Item = Result<Item<Op::Event>, ProviderError>>`（即 `Streamed<Completion>` = `CompletionStream`）；文本增量是 `StreamEvent::Text { part, text }`；错误是流的最后一项，此后流结束；`finish()` 折叠出 `CompletionResponse`（`choice` + `Usage { input_tokens, output_tokens, total_tokens }`，三个计数都是 `Option`）；`partial()` 给出已到达内容。
  - 空闲超时（`driver.rs:63-97, 500-590`）：`Model::with_idle_timeout(Option<Duration>)`，默认 `STREAM_IDLE_TIMEOUT = 300s`，只对 `Mode::Streaming` 计时（首帧前与帧间），超时产出 `ProviderError::Stalled(Duration)`。
  - 消息类型（`completion/message.rs:14-54`、`completion/request.rs:1203`）：`Message` 是**枚举**（`System { content }` / `User { content: Vec<UserContent> }` / `Assistant(AssistantMessage)`），不是 `{ role, content }` 结构体；`CompletionRequest { chat_history: Vec<Arc<Message>>, max_tokens: Option<u64>, tools, documents, … }`。
  - 错误（`error.rs:313-369`）：`ProviderError::{Http, Json, Url, Request, Response, Provider, ProviderResponse, InvalidAuthentication, Truncated, Stalled(Duration), Relayed, UnsupportedOption, CacheExpired, MismatchedDimensions}`。
  - 仓库现状：`Cargo.toml` 仍无 Rig 依赖；本地 registry 已有 `rig-core-0.46.0`，但 `rig-http` / `rig-reqwest` 尚未落地，传输层（`send_streaming` 的字节流实现）本轮无法逐行引用。
- **可选方案**：
  1. 依赖 facade `rig`（含 `agent`）——REJECTED：会把 Rig 的 Agent Loop 拉进依赖树，与「Agent Runtime 自研」直接冲突。
  2. 依赖 `rig-core`（`default-features = false, features = ["reqwest","rustls"]`），在 `agent-route` 内做 Adapter——**采用**。
  3. 自己实现 OpenAI 兼容 HTTP 客户端——REJECTED：与「Rig 承担 Provider / Completion 基础设施」的既定决策冲突，且重复实现协议。
- **决策**：
  1. 只依赖 `rig-core`，不依赖 `rig` facade；声明固定为 `rig-core = { version = "0.46", default-features = false, features = ["reqwest", "rustls"] }`。
  2. `rig` 的类型只允许出现在 `crates/agent-route/src/rig/` 下；上层只见项目自有 `LlmProvider` / `LlmRequest` / `LlmStreamEvent` / `LlmError`。
  3. **禁止**使用 `rig_agent`、`AgentBuilder`、`AgentRun`、任何 tool loop API；Agent Loop 由 `agent-runtime` 实现（P2 只做单轮）。
  4. Provider 协议范围：P2 只支持 OpenAI 兼容（`Route::Chat`，即 `POST /chat/completions`）。
  5. 完成模型只用 `OpenAI::chat(model)` 构造，不用 `OpenAI::completion(model)` 的自动路由（避免 provider 默认路由漂移到 `/responses`）。
  6. 流式空闲超时用 `Model::with_idle_timeout(Some(Duration::from_millis(agent.llm_idle_timeout_ms)))`，不自研计时器；`ProviderError::Stalled` 映射 `LlmError::IdleTimeout`。
  7. Rig 的 `Message` 枚举构造收敛在 `agent-route/src/rig/`：项目自有 `LlmMessage { role, content }` 在 Adapter 内翻译成 `Message::System` / `User` / `Assistant`，上层不出现 Rig 类型。
- **理由**：职责边界最清晰（Rig = 传输 + 协议编解码；项目 = 循环 + 状态 + 持久化 + 事件），且不会因为 Rig 的运行时抽象而把 Run 生命周期交给第三方。
- **代价与风险**：`rig-core` 依赖面较大（`schemars`、`uuid`、`web-time`、`rig-http`、`rig-reqwest`），编译时间与体积上升；只记录数据不设阈值（P2-032）。
- **O-1 结论（源码级，未实测）**：`Streamed` 只持有 `shared`（`Arc<Mutex<Shared<Op>>>`）与 `reading`（`driver::read` 返回的 `async_stream`），没有后台任务持有它们，`select!` 命中取消分支后 drop `Streamed` 即 drop `reading` 与底层 body 流。`rig-http` / `rig-reqwest` 未落地，因此「reqwest body 释放即关闭 TCP 连接」只能作为**推断**。**影响范围**：`run.cancel` / `run.disconnect` / `run.shutdown` 只断言「终态 + 事件文件有终态 + Mock LLM 不再收到后续分块」，不断言连接立即 FIN；P2-016 实测若发现连接不随之关闭，只在此处补记录，不改用例结构。
- **状态**：`DECIDED`（P2-001 勘察完成：构造 / 流式 / 错误三段均有上方源码出处；O-1 见本条结论）。
- **承接任务**：P2-001、P2-015、P2-016、P2-032。

---

## D-P2-002 持久化：`sqlx` 而非 `rusqlite`

- **问题**：P2 的 SQLite 驱动选谁？
- **背景（已核实）**：
  - `Cargo.toml` 的 `[workspace.dependencies]` **已声明** `sqlx = { version = "0.8", default-features = false, features = ["any","json","macros","runtime-tokio-rustls","sqlite"] }`。
  - `Cargo.lock` 中**没有** `sqlx`，任何 crate 都未引用它——即「预留未落地」。
  - `sqlx` 0.9 的 `rust-version` 更高；当前声明为 0.8。
- **可选方案**：
  1. `sqlx`：原生 async、Tokio 一等公民、内置连接池与迁移（`migrate` 特性）、`sqlite` 特性自带 bundled amalgamation。
  2. `rusqlite`：同步 API，需要 `spawn_blocking` + 自建连接池与线程模型；迁移要自己写或额外用 `rusqlite_migration`。
  3. 两者共存——REJECTED：禁止同时引入两套数据库框架。
- **决策**：采用 `sqlx`；`rusqlite` 记为 `REJECTED`（单进程多会话并发下需要手写阻塞线程模型与连接池，与既有 Tokio 运行时和「Repository + 事务」的写法不匹配）。
- **O-3 结案（版本定格）**：本地 registry 有 `sqlx-0.8.6` 与 `sqlx-0.9.0`。0.9.0 的 `rust-version = 1.94.0`（工作区 1.98.0 满足），但 feature 名与语义已变：`runtime-tokio-rustls` 不存在（拆为 `runtime-tokio` + `tls-*`），`sqlite` 展开成 `sqlite-bundled` + `sqlite-deserialize` + `sqlite-load-extension` + `sqlite-unlock-notify` 四项，`macros` 额外带上 `offline`。P2 只用 SQLite 单文件，升级收益为零而验证面变大，**定格 0.8.6**；0.9 的评估留给后续阶段独立验证。
- **理由**：与现有 Tokio + Repository 分层一致；迁移内建；bundled SQLite 免去系统库依赖，符合单 Binary 分发目标。
- **状态**：`DECIDED`（O-3 结案见上，定格 0.8.6）；bundled SQLite 的编译在 P2-010 落地时确认（Windows 上走 cc）。
- **承接任务**：P2-002、P2-010。

---

## D-P2-003 `sqlx` feature 组合（不启用 `macros`）

- **问题**：`sqlx` 的 feature 怎么选，才能既够用又不引入编译期数据库依赖？
- **背景（已核实）**：`macros` 特性启用 `query!` / `query_as!` 编译期校验，需要 `DATABASE_URL` 或 `.sqlx` 离线数据；当前声明里带着 `macros`。
- **决策**：
  1. `sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "tls-none", "sqlite"] }`——去掉已声明的 `macros` / `any` / `json` / `migrate`：P2 只有 SQLite 单文件、没有网络数据库（不需要 TLS）、不存 JSON 列、不使用 `sqlx::migrate!`（迁移见 D-P2-030）。
  2. 查询一律用运行时 API（`sqlx::query` / `query_as`），SQL 统一写在 `service-core` 的 `repository` 内；行映射手写（`Row::try_get`），不启用 `derive` / `FromRow`。
  3. `sqlx::migrate!` 在 0.8 里由 `macros` 特性提供（`sqlx/src/lib.rs` 的 `pub extern crate sqlx_macros` 与 `#[cfg(feature = "macros")] mod macros`），与「不启用 `macros`」直接冲突；迁移改由 `lib-db` 自建 runner 执行，见 D-P2-030。
- **理由**：CI 与本地构建不应需要数据库连接或离线缓存文件；Repository 层本来就要手写 SQL 与行映射。
- **状态**：`DECIDED`。
- **承接任务**：P2-002、P2-010。

---

## D-P2-004 数据库位置、连接参数与测试隔离

- **决策**：
  1. 数据库文件：`lib_core::directorys()?.data` 目录下 `agent.db`（release 在用户目录、debug 在可执行文件同级 `runtime`、`AGENT_HOME` 可整体覆盖）。事件文件仍在同级的 `data/events/`（P1.1，D-P1.1-003）。
  2. 连接：`SqlitePoolOptions`，`max_connections = 5`；`PRAGMA journal_mode=WAL`、`busy_timeout=5000`、`foreign_keys=ON`。
  3. 测试隔离沿用 P1 / P1.1：`bin-tests` 用 `AGENT_HOME` 指向 `target/tests/<运行 ID>/...`，不新增测试专用环境变量。
- **理由**：不新增配置面与路径约定，复用已验证的目录与隔离机制；WAL + `busy_timeout` 覆盖「多会话并发 + 单进程写入」的争用。
- **状态**：`DECIDED`。
- **承接任务**：P2-002、P2-010、P2-013。

---

## D-P2-005 消息顺序用 `seq` 列；会话消息数用 `COUNT`

- **问题**：消息顺序怎么保证？`SessionSummary.messageCount` 怎么维护？
- **背景（已核实）**：P1 的 `types-core::session` 用「按会话分桶的 `Vec<Message>`」保证写入顺序，`message_count` 在存储里累加（P2 迁入 `agent-core`，契约字段不变）。换成 SQL 后 `created_at_ms` 可能相同（同一毫秒写入用户消息与助手消息），不能作为排序依据。
- **决策**：
  1. `messages` 增列 `seq INTEGER NOT NULL`，`UNIQUE(session_id, seq)`；`seq = COALESCE(MAX(seq),0)+1` 在同一事务内分配；所有读取按 `seq` 升序。
  2. `SessionSummary.messageCount` 用 `COUNT(*)` 子查询，不维护冗余计数列。
- **理由**：`seq` 是唯一稳定顺序来源，且能顺带发现并发写漏洞（唯一约束会直接报错）；`COUNT` 避免「计数与真实行数漂移」。
- **状态**：`DECIDED`。
- **承接任务**：P2-002、P2-011、P2-012。

---

## D-P2-006 Run 执行由 `api/run/stream` 订阅触发

- **问题（已作废）**：P2 的 Run 什么时候开始执行——`message/create` 之后立刻，还是等 SSE 订阅？
- **原决策**：订阅 `api/run/stream` → 注册表占位 → 获得 Session 租约与全局信号量 → `QUEUED → RUNNING` → 启动执行任务 → 返回事件流。
- **原理由**：沿用 P1 的语义与用例断言，客户端不需要重写。
- **P1.1 修订（2026-11-20）**：本项**已作废**。P1.1 移除了 `api/run/stream`（D-P1.1-004），「订阅即执行」的前提不复存在：Run 现在只有在**所属会话被客户端占用**时，才由该会话的运行时会话串行执行（`occupancy/create` 执行存量 `QUEUED`，`message/create` 追加队尾）。断线语义也从「per-run 订阅者断开即取消」变成「客户端下行流断开 → 断连处理链停止该会话的运行时会话」。新的接入方式见 **D-P2-028**。
- **对本文件其他项的影响**：凡引用「订阅触发」「per-run 事件流」的表述（D-P2-007 的租约与排队、D-P2-008 的流式时序、D-P2-010 的取消传播、D-P2-024 的回放口径）均已按新触发点修订或标注。
- **状态**：`REJECTED`（原结论保留，被 D-P2-028 取代）。
- **承接任务**：P2-004（修订记录）。

---

## D-P2-007 同 Session 串行 + 全局并发上限 + 排队超时

- **问题**：同一次会话能否并发多个 Run？全局要不要限流？超限排队还是拒绝？
- **决策**：
  1. **同 Session 单 Run 串行**：同一会话同一时刻最多一个 `RUNNING` Run，第二个 Run 在第一个终态前保持 `QUEUED`。
  2. **全局并发上限** `agent.max_concurrent_runs`（默认 4）：`Semaphore` 控制，超限排队（不拒绝请求）。
  3. **排队超时** `agent.run_queue_timeout_ms`（默认 120000）：到点把 Run 落 `FAILED`（`error` = 「执行排队超时」）并发 `run_failed`。
  4. 获取顺序固定「先进入会话执行队列（已串行），再取全局信号量」，避免死锁。
- **理由**：同一会话的并发 Run 会让 Context 组装读到不稳定的历史（助手消息可能尚未写入），串行是唯一能保证「上下文可预测」的低成本方案；全局上限防止本机被 N 个并发模型连接拖垮。
- **P1.1 修订（2026-11-20）**：第 1 条**已由 P1.1 实现**——`RuntimeSession` 内的 `RunExecutor`（`service-core/src/session/run_trigger.rs`）就是一条 `mpsc` 串行任务，`QUEUED → RUNNING` 在存储锁内推进；P2 **不再引入 Session 租约 `Mutex`**，只保留第 2~4 条的全局信号量与排队超时。排队期间客户端看到的是 Run 停在 `QUEUED`（P1.1 没有排队事件，P2 也不新增）。
- **代价**：单会话吞吐受限；排队期间客户端看不到「排队中」事件。
- **状态**：`DECIDED`（P1.1 修订版为实施依据）。
- **承接任务**：P2-004、P2-025、P2-026。

---

## D-P2-008 持久化时序：流式不落库、终态与助手消息同事务

- **问题**：Streaming 期间要不要频繁写库？助手消息何时创建？部分内容怎么保存？
- **决策（六条路径）**：
  1. **创建**（`api/message/create`）：单事务 `{INSERT 用户消息(seq), INSERT runs(QUEUED), UPDATE sessions.updated_at}`。
  2. **执行中**：**不写库**。增量只在内存累积并通过事件推送（事件由 P1.1 的写者落盘到事件文件，属事件事实源，不是消息数据）；`messageId` 在 `run_started` 之前生成，该消息在成功收尾时才落库。
  3. **成功**：单事务 `{INSERT 助手消息(seq), UPDATE runs → COMPLETED + usage + finished_at, UPDATE sessions.updated_at}`；**事务提交成功后**才发 `run_completed`。
  4. **失败**：`UPDATE runs → FAILED`（`error` = 脱敏文案 + `finished_at`），**不写助手消息**，然后发 `run_failed`。
  5. **取消 / 断连 / 关闭**：条件更新 `UPDATE runs → CANCELLED`，**不写助手消息**，然后由执行层发 `run_cancelled` / `server_shutdown`（D-P2-029）。
  6. **进程重启自愈**：`RUNNING → FAILED`、`QUEUED → CANCELLED`（D-P2-011）。
- **理由**：逐 delta 落库会产生大量小事务并放大 SQLite 写入争用，收益只有「断线后能看到部分内容」这一 P6 能力；保持「不写半成品助手消息」语义则让「成功 Run 必有一条完整助手消息、失败/取消 Run 必无」成为可断言的不变量。
- **P1.1 修订（2026-11-20）**：原第 5 条写「不发终态事件」，与 P1.1 的事件文件要求冲突——事件文件里每条 Run 都需要终态事件才有可读的结束。现改为**由执行层发终态事件**（`run_cancelled` 的 `error` 取固定原因文本），P1.1 写者的「停止时兜底补写」只在最后一条事件不是终态时触发，作为安全网保留。详见 D-P2-029。
- **状态**：`DECIDED`（P1.1 修订版为实施依据；部分内容持久化 `DEFERRED`，见 D-P2-023）。
- **承接任务**：P2-004、P2-024、P2-026。

---

## D-P2-009 终态唯一提交由数据库条件更新裁决

- **问题**：取消与完成同时发生时谁获胜？如何保证终态只写一次？
- **决策**：
  1. 所有终态写入都是条件更新：`UPDATE runs SET status=?, ... WHERE id=? AND status IN ('QUEUED','RUNNING')`。
  2. 受影响行数为 0 = 已被他人提交终态 → 本次什么都不写、不发终态事件、静默退出。
  3. 成功路径额外要求 `WHERE status='RUNNING'`，保证 `QUEUED` 的 Run 不会被误判为完成。

  4. 内存侧不设「终态锁」；数据库是唯一裁决者（注册表只用于取消信号与单生产方占位）。
- **状态转换表（P2-004 定稿，P2-012 / P2-022 / P2-024 实施依据）**：

| 起点 | 终点 | 触发方 | 守卫 |
|---|---|---|---|
| — | `QUEUED` | `api/message/create` | 单事务写用户消息 + Run |
| `QUEUED` | `RUNNING` | 会话执行队列（仅在占用存在时） | `WHERE status='QUEUED'` |
| `RUNNING` | `COMPLETED` | 执行器成功收尾 | 事务内条件更新 + 插入助手消息 |
| `RUNNING` | `FAILED` | 执行器失败收尾 | 条件更新 + 脱敏 `error` |
| `QUEUED` / `RUNNING` | `CANCELLED` | `api/run/cancel`、客户端断连、服务关闭、启动自愈 | 条件更新，影响 0 行即放弃 |
| 任意终态 | 任意 | — | 禁止；条件更新影响 0 行视为「已终态」，不报错 |
- **P1.1 补充**：`QUEUED → RUNNING` 的推进仍走 `Store::start_next_run` 的状态 CAS（P1.1 已实现），与终态条件更新同属一条规则：任何状态推进都必须由条件更新裁决。
- **理由**：内存锁在崩溃、panic 后不可靠；条件更新是 SQLite 单写者模型下最简洁的正确解。
- **状态**：`DECIDED`。
- **承接任务**：P2-012、P2-024、P2-026。

---

## D-P2-010 Cancellation 归属与传播

- **问题**：`CancellationToken` 谁持有？LLM 流怎么中断？断连与关闭怎么联动？
- **决策**：
  1. 每个 Run 一个 `CancellationToken`：执行任务持有，`RunRegistry` 存 clone；Run 结束（任意终态）时移除。
  2. `api/run/cancel` 的处理顺序：先由 `service-core` 做条件更新（数据库权威）→ 成功后触发 token → 执行器在 `select!` 的取消分支退出并发 `run_cancelled`。
  3. LLM 流中断方式：`select!` 命中取消分支后 **drop `LlmStream`**，底层 HTTP 连接随之关闭（「drop 即停」由 P2-016 按 O-1 实测确认）。
  4. **客户端断连 = 停止会话**（P1.1 语义）：下行流断开 → 断连处理链停止该会话的运行时会话（`SessionStop::ClientDisconnected`）→ 在跑的 Run 落 `CANCELLED`。
  5. 服务关闭：P1.1 的关闭链路（`handler::shutdown`）会给全部客户端打 `ServerShutdown` 并停运行时会话；执行器走「关闭收尾」分支，事件为 `server_shutdown`。
  6. panic：执行任务用 `JoinHandle` 结果 / `catch_unwind` 兜底，panic 时尽力把 Run 落 `FAILED` 并清理注册表。
- **理由**：与既有的三种终止语义（取消 / 断连 / 关闭）一一对应，客户端无需改动；把「中断模型调用」而不是「停止推送」作为取消的判据，避免后台继续烧 token。
- **P1.1 修订（2026-11-20）**：原第 4 条基于 per-run SSE 的 `sender.closed()`，现已由 P1.1 的断连处理链承担；P2 只需在 `RunExecutor::stop` 的既有链路上响应取消信号，不再自行观察通道关闭。
- **状态**：`DECIDED`（「drop 是否立刻断连」为 `OPEN`，见 O-1）。
- **承接任务**：P2-024、P2-026。

---

## D-P2-011 启动自愈：`RUNNING → FAILED`、`QUEUED → CANCELLED`

- **问题**：Server 崩溃或重启后，数据库里的 `RUNNING` / `QUEUED` 记录怎么办？
- **决策**：服务启动阶段（`migrate` 之后、开始监听之前）执行一次自愈：
  - `RUNNING → FAILED`，`error` = 「服务重启，执行已中断」，写 `finished_at_ms`。
  - `QUEUED → CANCELLED`，`error` = 「服务重启，未开始的执行已取消」，写 `finished_at_ms`。
- **理由**：执行任务随进程消失，不可能续跑（D-P2-024）；让残留状态进入用户可理解的终态比留在 `RUNNING` 更安全（否则会话列表永远显示「进行中」，`runningRunCount` 失真）。`RUNNING` 用 `FAILED` 而非 `CANCELLED`，因为用户并没有主动取消。
- **P1.1 补充**：占用与客户端注册**不恢复**（D-P1.1-007），因此自愈只处理数据库里的 Run 状态，不动占用表与事件文件；客户端重新注册并重新占用后即可继续对话。
- **状态**：`DECIDED`。
- **承接任务**：P2-012、P2-014、P2-026。

---

## D-P2-012 Context 预算来源与 Token 估算

- **问题**：Token 预算从哪里来？怎么估算？
- **决策**：
  1. 预算来源：`[[provider.models]]` 的 `context_window` 与 `max_output_tokens`（显式给出，缺省 8192 / 1024）；P2 不引入模型目录或 tokenizer 下载。
  2. 估算：`tokens ≈ ceil(ascii_chars / 4) + non_ascii_chars + 4`（每条消息固定开销 4）。
  3. 预算：`available = context_window - max_output_tokens - safety_tokens(默认 256) - system_prompt_tokens`。
  4. 可替换：`TokenEstimator` trait + `HeuristicEstimator` 默认实现，构造点唯一（`agent-context`）。
- **O-4 结案**：`context_window` 8192 / `max_output_tokens` 1024 只是缺省兜底——模型必须出现在 `[[provider.models]]`（D-P2-015 拒绝未知模型），实际预算由配置显式给出，因此不构成「本地模型被频繁裁剪」的风险。
- **理由**：不引入 tokenizer 依赖（体积与离线要求）；启发式对「中英混排」偏保守（CJK 按 1 字符 1 token 估），宁可提前裁剪也不越界。
- **状态**：`DECIDED`。
- **承接任务**：P2-003、P2-020、P2-021。

---

## D-P2-013 历史裁剪策略与超限失败

- **决策**：
  1. 先按 `agent.context_max_messages`（默认 40，约 20 轮）截断最旧消息。
  2. 再逐条丢弃最旧历史直到估算不超预算。
  3. **系统提示永不被裁**；**最新一条用户消息必须保留**。
  4. 最新用户消息单条即超预算 → Run 落 `FAILED`，`error` = 「上下文超出模型预算」，**不发起上游请求**。
  5. 被裁掉的消息数记入 `Context` 的 metadata，只进日志。
- **理由**：显式失败比静默截断用户输入更诚实；「整条丢弃 + 保证最新输入」最容易解释与验证，也让 P3 的 Tool/Memory 贡献点有明确插入位置。
- **状态**：`DECIDED`。
- **承接任务**：P2-003、P2-020、P2-021。

---

## D-P2-014 P2 不新增接口 / DTO 字段 / 事件枚举值

- **问题**：接入真实运行时后，契约要不要扩展？
- **背景（已核实）**：`Message` / `Run` / `RunEvent` 的字段已按「P2 需要」预留（`error`、`input_tokens`、`output_tokens`、`finished_at_ms`、`message_id`、`status`、`delta`）；`RunEventKind` 已有 `run_failed`，P1.1 又追加了 `run_cancelled`，`heartbeat` 保留但不产出。
- **决策**：P2 **零新增**：25 条接口的 method / path / 入参 / 出参不变；DTO 字段不变；`RunEventKind` 保持 7 个取值。
- **理由**：既有客户端与验收都建立在这份契约上；「够用」的字段已经存在，为内部架构迁移改契约属于无收益风险。P3 需要 `tool_call_*` 时按「只追加枚举值」的方式扩展。
- **状态**：`DECIDED`。
- **承接任务**：P2-005、P2-008、P2-030、P2-031。

---

## D-P2-015 未知模型返回 400

- **问题**：`SessionCreatePO.model` / 会话当前模型未在配置中出现时怎么办？
- **决策**：返回 400 `模型不可用：<id>`（沿用 `SessionError::Invalid` → 400 的既有映射，不新增错误码）。`model` 缺省时使用 `provider.default_model`。
- **理由**：P1 的 Mock 接受任意字符串（默认 `mock-model`）；P2 若静默回落到默认模型，用户会以为换了模型而实际没有。显式拒绝是最小、可读、可测的行为。
- **状态**：`DECIDED`。
- **承接任务**：P2-005、P2-015、P2-017。

---

## D-P2-016 Provider 配置只走文件与环境变量，不新增 API

- **决策**：
  1. 配置来源：`<应用根目录>/config.toml` 的 `[provider]` / `[[provider.models]]` / `[agent]`，环境变量覆盖（清单见 `README.md`）。
  2. **不新增任何 Provider / Model 配置接口**；不做热更新（改配置需重启）。
  3. `service-core` 提供只读的 `provider` 服务（装载 + 校验 + 解析），`agent-route` 只消费。
- **理由**：P2 的范围是运行时而不是配置管理界面；一旦开口返回 Provider 配置，就要处理「密钥是否出现在响应里」这类安全设计（D-P2-025 `DEFERRED`）。文件 + 环境变量足以支撑 P2 的全部验收。
- **状态**：`DECIDED`。
- **承接任务**：P2-005、P2-017。

---

## D-P2-017 API Key 明文存配置文件 + 脱敏规则

- **决策**：
  1. 存放：`config.toml` 的 `provider.api_key`（明文）或环境变量 `AGENT_PROVIDER_API_KEY`；环境变量优先。
  2. `api_key` **不进入任何 DTO / 响应**；`Secret` 的 `Debug` 与 `Display` 均为 `***`，取值只能 `expose()`。
  3. 日志：不记录请求头；Provider 相关日志只允许 `provider id` 与 `base_url` 的 host。
  4. 错误：`LlmError::public_message()` 为固定脱敏文案；Provider 原始错误只进 `source`（日志），且日志路径也不打印 key。
  5. 文档提示：`config.toml` 含密钥，不得提交（`.gitignore` 已忽略本地配置文件）。
- **理由**：P2 单机单用户，系统密钥库会引入平台分支与额外依赖，收益不足；但「不泄漏」的工程措施必须现在就到位。
- **状态**：`DECIDED`（加密存储见 D-P2-026 `DEFERRED`）。
- **承接任务**：P2-005、P2-009、P2-017、P2-028。

---

## D-P2-018 Crate 新增范围与依赖边调整

- **决策**：
  1. 新增（均在 `docs/workspace.md` 8.2 的 P2 范围内）：`agent-core`、`agent-context-core`、`agent-context`、`agent-route`、`agent-runtime`、`lib-db`。
  2. 新增依赖边（属**目标架构**调整，需同步 `docs/workspace.md`）：
     - `service-core -> agent-core`：仅允许依赖这一**类型层**；仍禁止 `service-core` 依赖 `agent-runtime` / `agent-route` / `agent-context*`。
     - `agent-runtime -> service-core`：Run / Message 的持久化规则属于 `service-core`，Runtime 通过其 Service 读写，并实现其执行端口；方向仍为上层依赖下层。
     - `lib-web -> agent-core`：Handler 的 DTO 类型来源。是否新增 `lib-web -> agent-runtime` 取决于启动装配是否需要接触 Runtime 类型，落地时以 `cargo tree` 为准（执行端口经 `service-core` 注入，正常不需要）。
     - `lib-bin-web -> service-core` / `agent-runtime` / `lib-db`：启动装配；仍不承载业务逻辑、不直接调用 Runtime 的业务方法。
  3. 不创建：`manager-core`、`types-api`、`bin-tui`、`bin-web`、`bin-mock-server`、`agent-tool`、`agent-mcp`、`agent-skill`。
- **P1.1 修订（2026-11-20）**：P1.1 已删除 `RunEventStream` 与 `lib-web/src/session/stream.rs`，因此「`RunEventStream` 迁入 `agent-runtime`」不再成立——事件生产方就是传给执行端口的 `mpsc::Sender<RunEvent>`。
- **理由**：契约类型迁入 `agent-core` 后 `service-core` 必然依赖它；持久化归 Service、执行归 Runtime 是既有分工。把边界写成「允许 `agent-core`、禁止其余」比原文档的「全部禁止」更准确。
- **状态**：`DECIDED`（`docs/workspace.md` 由 P2-005 出清单、P2-030 回填）。
- **承接任务**：P2-005、P2-006、P2-010、P2-013、P2-022、P2-030。

---

## D-P2-019 进程级访问点：`holder` 改显式注入

- **问题**：Handler 不接受服务实例参数（`web_api` 只支持 `Json<T>` / `Query<T>`），进程级访问点怎么处理？
- **背景（已核实）**：P1 的 `service-core::holder` 是 `LazyLock<SessionService>` 单例；P1.1 新增的 `RuntimeState` 走的是 `service_core::runtime::install(state)`（`OnceLock`，显式注入），两者并存。
- **决策**：
  1. 删除 `service-core::holder`；`SessionService` 改为构造注入：`RuntimeState::new(session)` 持有它（执行队列与占用校验需要）。
  2. `lib-web` 新增显式 holder：`OnceLock<AppState { session: Arc<SessionService>, runtime: RuntimeState }>` + `install(...)` + 访问器；Handler 改用它。
  3. `probe_health` 在未安装时返回 `runningRunCount = 0`，保证 `lib-web` 的 example 服务仍可独立启动。
  4. `service-core::runtime::install` 的形态保留（P1.1 已定的注入点），P2 不改。
- **P1.1 修订（2026-11-20）**：原决策里「`RunEventStream` 迁入 `agent-runtime`」一条作废（该类型已不存在）；`holder` 的归属从「迁入 `lib-web`」细化为「`lib-web` 持 `SessionService`、`service-core` 内由 `RuntimeState` 持有」。
- **理由**：holder 是 Web 层的进程级访问点（对应 `docs/workspace.md` 4.4「仅在确实需要跨调用共享状态时引入」）；`service-core` 内部的执行队列需要 `SessionService`，用构造注入比全局单例更明确。
- **状态**：`DECIDED`。
- **承接任务**：P2-013、P2-023。

---

## D-P2-020 心跳间隔可配置（`AGENT_STREAM_HEARTBEAT_MS`）

- **问题（已作废）**：P1 的心跳间隔硬编码 15 秒，E-3 无法在合理时间内自动验证。
- **原决策**：心跳间隔由 `agent.stream_heartbeat_ms`（默认 15000）决定，环境变量 `AGENT_STREAM_HEARTBEAT_MS` 可覆盖。
- **P1.1 修订（2026-11-20）**：本项**作废**。P1.1 把连接保活拆成两件事——客户端每 15s 上行 `api/client/update {PING}`（服务端 45s 无心跳即判超时）与下行流的 `: keepalive` 注释帧；两者都是常量（D-P1.1-006），且 `RunEventKind::Heartbeat` 不再产出。因此不需要新增 `stream_heartbeat_ms` 配置，E-3 的结论改为「契约保留值，无需验证」（P2-029）。
- **状态**：`REJECTED`。
- **承接任务**：P2-029（结案说明）。

---

## D-P2-021 测试只在 `bin-tests`；LLM 用内置 TCP Mock Server

- **决策**：
  1. 所有用例只在 `crates/bin-tests`（仓库 `AGENTS.md` 第 28 条），其他 Crate 不写 `#[cfg(test)]`。
  2. LLM 侧用 `bin-tests` 内置的最小 HTTP/1.1 Mock Server（`tokio::net::TcpListener` 手写，不引入 Web 框架），提供 `POST /v1/chat/completions`（JSON 与 SSE）、`GET /v1/models`、`GET /mock/last-request`、`GET /mock/stats`。
  3. **不创建** `bin-mock-server`（P2 范围外）。
- **P1.1 补充**：`bin-tests` 已在 P1.1 依赖 `service-core` / `types-core`（进程内驱动运行时内核）；该依赖只允许用于「驱动运行时内核与事件文件」，Mock LLM 仍必须手写、不引入 `lib-web` / `lib-bin-web`。
- **理由**：手写最小 HTTP 让「非法 JSON、中途断连、静默 stall」这些场景可精确控制，也不把 Web 框架带进测试入口。
- **状态**：`DECIDED`。
- **承接任务**：P2-018、P2-026、P2-031。

---

## D-P2-022 P1 遗留 A-1（生成物过期检测）

- **问题**：P1-020 遗留「A-1 生成物过期检测没有自动化方案」。
- **背景（已核实）**：生成物不入库（P1 D-006），门禁每次清空 `packages/` 后重建（`scripts/gen-check.sh`），因此不存在「仓库里的生成物与定义不一致」这一可检测状态；P1-020 已实测确认「改定义后不重新生成，`gen-check` 仍通过」。
- **决策**：A-1 的原始表述 **`REJECTED`**；等价门禁保留为「缺生成物 / 生成失败 / 产物不完整 / 生成代码不能编译 → 非 0 退出」，即现有 `make gen-check`。
- **理由**：判据在生成物不入库的前提下不可满足；继续追求会迫使生成物入库，与 D-006 冲突。
- **状态**：`REJECTED`。
- **承接任务**：P2-029、P2-030。

---

## D-P2-023 流式部分内容持久化

- **决策**：P2 不做。取消 / 断连 / 崩溃一律丢弃已累积内容，Run 落终态且无助手消息；事件文件里仍有该轮已产出的 `message_delta`（事件事实源），但不会还原成消息。
- **理由**：与既有语义一致（不写半成品），且「部分内容」需要额外的消息状态机（draft / finalized）与前端渲染规则，属 P6 的 Checkpoint / Resume 范畴。
- **P1.1 补充**：P1.1 的事件文件让「那轮到底产出了什么」可查，但**不是**可恢复的消息草稿，两者不可混淆。
- **状态**：`DEFERRED`（归属 P6）。
- **承接任务**：P2-004（记录限制）、P2-032（写入偏差说明）。

---

## D-P2-024 事件回放与断点续传

- **问题**：客户端断线后能否重连续看同一 Run？
- **决策**：P2 不支持。P1.1 的下行流没有 `Last-Event-ID` 续传，断线即触发断连处理链（停会话、取消在跑 Run），客户端要重新注册 + 重新占用才能继续。
- **理由**：需要订阅者代际管理、事件索引与「断线是否取消」的重新定义；P1.1 已明确不续传，变更它属 P6 的 Checkpoint / Resume 范围。
- **P1.1 补充**：事件文件（`data/events/<sessionId>/<runtimeSessionId>.jsonl`）已经是完整的事件事实源，为将来做回放提供了原材料；但「按事件文件回放给客户端」不属 P2。
- **状态**：`DEFERRED`（归属 P6）。
- **承接任务**：P2-004（记录限制）。

---

## D-P2-025 Provider 配置 CRUD 接口

- **决策**：不做。Provider 配置只走 `config.toml` + 环境变量（D-P2-016）。
- **理由**：需要同时解决密钥写入、脱敏返回、校验失败提示、热更新等一整套设计；P2 的验收不依赖它。
- **状态**：`DEFERRED`（归属阶段未定，需在 P3 之前评估是否与 Tool 配置一起设计）。
- **承接任务**：P2-005（记录）。

---

## D-P2-026 API Key 加密存储 / 系统密钥库

- **决策**：P2 不做。明文 + 文件权限提示 + 严格脱敏（D-P2-017）。
- **理由**：跨平台密钥库引入平台分支、额外依赖与打包问题；当前威胁模型是单机单用户本地文件。
- **状态**：`DEFERRED`（归属 P6）。
- **承接任务**：P2-005（记录）。

---

## D-P2-027 Framework 依赖与 CI 策略（P2 不变）

- **决策**：
  1. 不修改 Framework `path` 依赖，不提前切换 Version Dependency。
  2. 不修改 `.github/workflows/ci.yml` 以绕过问题，不为 CI 失败改无关代码。
  3. 本地必须完整执行 `make gen` / `make gen-check` / `make check` / `make ui-check` / `make test ARGS="--all"` 与前端用例，并把结果记入 P2-032 报告。
  4. CI 失败原因单独记录，明确「与 P2 功能无关，P6 Framework 发布正式版本后统一恢复 CI 验收」。
- **理由**：CI 失败与功能失败是两件事；为让 CI 变绿而动的改动会把风险推迟到更晚阶段暴露。
- **状态**：`DECIDED`。
- **承接任务**：P2-005、P2-032。

---

## D-P2-028 Run 触发点继承 P1.1；执行端口由 `agent-runtime` 实现并注入

- **问题**：P2 的真实执行放在哪一层？`service-core` 的执行队列如何调用到 `agent-runtime`，且不产生反向依赖？
- **背景（已核实）**：
  - P1.1 的 `RuntimeSession`（`service-core/src/runtime/session.rs`）内含 Run 执行队列（`session/run_trigger.rs` 的 `RunExecutor`），`drain` → `run_one` 当前调用 `stream::produce`（Mock）。
  - 触发点唯一：`occupancy/create` 执行存量 `QUEUED`，`message/create` 通过 `service_core::runtime::notify_run` 唤醒队列；未被占用的会话不排队、不执行。
  - `service-core` 不得依赖 `agent-*`（`AGENTS.md` 第 6 条 / `docs/workspace.md`）；`agent-runtime -> service-core` 是允许方向。
- **可选方案**：
  1. 把执行队列迁入 `agent-runtime`，由它持有多会话队列 —— 会让 `service-core` 反向依赖 Runtime，或把 `RuntimeSession` 一并迁走，改动面过大。
  2. `service-core` 定义执行端口 trait，`agent-runtime` 实现，`lib-bin-web` 装配时注册 —— **采用**。
  3. `service-core` 直接依赖 `agent-runtime` —— REJECTED：违反依赖方向。
- **决策**：
  1. 端口定义在 `service-core`：`RunDriver::drive(input)`，`RunInput { run, prompt, events: mpsc::Sender<RunEvent>, cancel: CancellationToken }`，输出 `RunOutcome`。
  2. `agent-runtime` 实现端口：组装 Context → 调 `agent-route` → 逐段写 `Sender<RunEvent>` → 经 `service-core` 的 Service 落库 → 发终态事件。
  3. 注册点在 `lib-bin-web` 启动装配；未注册时（`api_probe_server`、部分进程内用例）退化为「Run 落 `FAILED`，`error` = 「执行器未装配」」并记日志。
  4. 触发点、串行队列、事件落盘与转发**全部沿用 P1.1**，P2 只替换 `run_one` 里的执行实现。
- **端口定义（P2-022 实施依据，`service-core/src/session/driver.rs`）**：

```rust
pub struct RunInput {
    pub run: Run,
    pub prompt: String,
    pub events: mpsc::Sender<RunEvent>,
    pub cancel: CancellationToken,
}

pub enum RunOutcome {
    Completed,               // 助手消息 + COMPLETED 已在同一事务提交
    Failed { public_error: String },
    Cancelled { reason: String },
    SessionGone,             // 运行时会话已停止，未提交任何终态
}

#[async_trait]
pub trait RunDriver: Send + Sync {
    async fn drive(&self, input: RunInput) -> RunOutcome;
}
```

  `RunOutcome` 只描述结局，不承载「谁来写库」：`agent-runtime` 经 `service-core` 的 Service 完成终态提交（`COMPLETED` 与助手消息同事务），条件更新影响 0 行即视为「已终态」并跳过终态事件（D-P2-009）。
- **理由**：不新增依赖边、不改触发语义、不动事件通路，改动面最小且边界最清晰；端口只在 `service-core` 内出现一次调用点，便于将来插入 Tool Loop（P3）。
- **状态**：`DECIDED`。
- **承接任务**：P2-004、P2-022、P2-023。

---

## D-P2-029 终态事件归属与「事件文件 / 数据库」事实源分工

- **问题**：每条 Run 的终态事件由谁发？事件文件与数据库谁说了算？
- **背景（已核实）**：
  - P1.1 的写者在会话停止时补一条终态事件，条件是「最后一条已写事件不是终态」；`api/run/cancel` 在 P1.1 里不产出终态事件（D-P1.1-011）。
  - P1.1 的取消原因只有三个固定文本：`会话占用已释放` / `客户端连接已断开` / `服务正在关闭`。
  - 事件文件是事件事实源，数据库是消息与 Run 状态的事实源。
- **决策**：
  1. **执行层负责每条 Run 的终态事件**：成功 → `run_completed`（事务提交后发）、失败 → `run_failed`、取消 → `run_cancelled`、关闭 → `server_shutdown`。
  2. `api/run/cancel` 新增一条固定原因文本 `用户取消执行`（`error` 字段是自由文本，不构成枚举扩展）；断连与关闭沿用 P1.1 的三个固定文本。
  3. P1.1 写者的「停止时兜底」保留：仅当最后一条事件不是终态时补写，正常情况下不触发（执行层已发）。
  4. 两者**不做对账**：数据库成功但事件文件写失败时，以「事件文件是排障依据、数据库是业务事实」为准，只记日志；反向同理。补偿机制属 P6。
  5. 顺序保证：数据库事务先提交，再发终态事件；写者收到事件后先落盘 flush 再转发，因此客户端看到 `run_completed` 时 `run/get` 已是终态。
- **理由**：让「每条 Run 在事件文件里都有终态」成为不变量，同时保持 P1「先落库再发事件」的可见性语义；把兜底留在写者侧，避免 P2 引入真实执行后出现「取消后文件无终态」的空洞。
- **状态**：`DECIDED`。
- **承接任务**：P2-004、P2-024、P2-026。

---

## D-P2-030 SQLite 表结构草案与迁移执行方式

- **问题**：`sessions` / `messages` / `runs` 的列、约束与索引怎么定？`sqlx::migrate!` 需要 `macros` 特性，而 D-P2-003 不启用它，迁移由谁执行？
- **决策**：
  1. 表结构（`crates/lib-db/migrations/0001_init.sql`，P2-011 落地）：

```sql
CREATE TABLE IF NOT EXISTS sessions (
    id            TEXT    PRIMARY KEY,
    title         TEXT    NOT NULL,
    model         TEXT    NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
    id             TEXT    PRIMARY KEY,
    session_id     TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    status         TEXT    NOT NULL,
    model          TEXT    NOT NULL,
    error          TEXT,
    input_tokens   INTEGER,
    output_tokens  INTEGER,
    created_at_ms  INTEGER NOT NULL,
    updated_at_ms  INTEGER NOT NULL,
    finished_at_ms INTEGER
);
CREATE INDEX IF NOT EXISTS idx_runs_session_created ON runs (session_id, created_at_ms);
CREATE INDEX IF NOT EXISTS idx_runs_status ON runs (status);

CREATE TABLE IF NOT EXISTS messages (
    id            TEXT    PRIMARY KEY,
    session_id    TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    run_id        TEXT             REFERENCES runs(id) ON DELETE SET NULL,
    role          TEXT    NOT NULL,
    content       TEXT    NOT NULL,
    model         TEXT,
    input_tokens  INTEGER,
    output_tokens INTEGER,
    seq           INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    UNIQUE (session_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_messages_session_seq ON messages (session_id, seq);
```

  2. 列与契约类型一一对应：`SessionSummary.message_count` 用 `COUNT(*)`（D-P2-005），不落列；`Run` / `Message` 的 `*_at_ms` 用 `INTEGER`，`*_tokens` 用可空 `INTEGER`，`error` / `model` / `run_id` 可空；枚举列（`role` / `status`）存 serde wire 值（`auto_enum` 注入 `#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`，即 `USER` / `QUEUED` 等），**不加 CHECK**——P3 追加枚举值时不需要迁移。
  3. 索引只建三类：`(session_id, seq)`（消息按序读）、`(session_id, created_at_ms)`（Run 按创建时间取队头）、`(status)`（启动自愈与 `runningRunCount`）。
  4. 终态写入语句（P2-012 的唯一形态，与 D-P2-009 一致，全部以受影响行数判断「已终态」、0 行不报错）：
     - 成功：`UPDATE runs SET status='COMPLETED', input_tokens=?, output_tokens=?, updated_at_ms=?, finished_at_ms=? WHERE id=? AND status='RUNNING'`
     - 失败：`UPDATE runs SET status='FAILED', error=?, updated_at_ms=?, finished_at_ms=? WHERE id=? AND status IN ('QUEUED','RUNNING')`
     - 取消：`UPDATE runs SET status='CANCELLED', error=?, updated_at_ms=?, finished_at_ms=? WHERE id=? AND status IN ('QUEUED','RUNNING')`
     - 启动自愈：`UPDATE runs SET status='FAILED', error=?, finished_at_ms=? WHERE status='RUNNING'`；`UPDATE runs SET status='CANCELLED', error=?, finished_at_ms=? WHERE status='QUEUED'`
  5. 迁移执行方式：`lib-db` 自建 runner，不用 `sqlx::migrate!`（该宏要求 `macros` 特性）。
     - 迁移账本表 `schema_migrations(version INTEGER PRIMARY KEY, applied_at_ms INTEGER NOT NULL)` 是唯一事实源（不使用 `PRAGMA user_version`）。
     - 迁移脚本以 `include_str!` 编进二进制，按 `version` 升序执行；每个脚本包在一个事务里，成功后写 `schema_migrations`，失败即回滚并返回 `DbError`。
     - 幂等：版本已存在则跳过，连续启动不重复执行。
- **理由**：外键与 `UNIQUE(session_id, seq)` 把 D-P2-005 的顺序保证交给数据库；枚举列存 wire 值让「DTO 序列化」与「落库」共用一套映射，不维护第二份字典；自建 runner 只多几十行代码，换来零 `macros` 依赖，且脚本编进二进制、不依赖运行时文件树（单 Binary 分发）。
- **状态**：`DECIDED`。
- **承接任务**：P2-002、P2-010、P2-011、P2-012。

---

## 附：API 契约核对（P2-005 产出）

25 条接口逐条核对 `method` / `path` / 入参 / 出参，**P2 全部不变**（D-P2-014）。入参 / 出参列给的是 P1.1 落地后的类型名。

| # | 方法 | path | 入参 | 出参 | P2 结论 |
|---|---|---|---|---|---|
| 1 | GET | `api/probe/void` | 无 | 空（204） | 不变 |
| 2 | POST | `api/probe/echo` | `Json<ProbeEchoPO>` | `ProbeEchoVO` | 不变 |
| 3 | GET | `api/probe/query` | `Query<ProbeQueryPO>` | `ProbeQueryVO` | 不变 |
| 4 | GET | `api/probe/typed` | `Query<ProbeQueryPO>` | `ProbeQueryVO`（匿名） | 不变 |
| 5 | GET | `api/probe/fail` | `Query<ProbeQueryPO>` | 错误响应 | 不变 |
| 6 | GET | `api/probe/response` | 无 | `WebResponse`（Blob） | 不变 |
| 7 | GET | `api/probe/response_of` | 无 | `WebResponseOf<ProbeQueryVO>` | 不变 |
| 8 | GET | `api/probe/internal` | 无 | 固定文案错误 | 不变 |
| 9 | GET | `api/probe/panic` | 无 | 固定文案错误 | 不变 |
| 10 | GET | `api/probe/health` | 无 | `ProbeHealthVO`（匿名） | 不变（未装配时 `runningRunCount = 0`） |
| 11 | GET | `api/probe/version` | 无 | `ProbeVersionVO`（匿名） | 不变 |
| 12 | POST | `api/probe/shutdown` | 无 | 空 | 不变（仅 debug 构建注册） |
| 13 | POST | `api/client/create` | `Json<ClientCreatePO>` | `ClientCreateVO` | 不变 |
| 14 | GET | `api/client/stream` | `Query<ClientStreamParams>` | SSE（Blob） | 不变（事件内容从 Mock 变为真实执行） |
| 15 | POST | `api/client/update` | `Json<ClientUpdatePO>` | `ClientUpdateVO` | 不变 |
| 16 | POST | `api/client/delete` | `Json<ClientDeletePO>` | 空 | 不变 |
| 17 | POST | `api/occupancy/create` | `Json<OccupancyCreatePO>` | `OccupancyVO` | 不变 |
| 18 | POST | `api/occupancy/delete` | `Json<OccupancyDeletePO>` | 空 | 不变 |
| 19 | GET | `api/occupancy/get` | `Query<OccupancyGetParams>` | `OccupancyVO` | 不变 |
| 20 | POST | `api/session/create` | `Json<SessionCreatePO>` | `SessionSummary` | 不变（新增拒绝路径，见 D-P2-015） |
| 21 | GET | `api/session/list` | 无 | `Vec<SessionSummary>` | 不变 |
| 22 | GET | `api/message/list` | `Query<MessageListParams>` | `Vec<Message>` | 不变 |
| 23 | POST | `api/message/create` | `Json<MessageSendPO>` | `MessageSendVO` | 不变（改为真实执行，仍返回 `QUEUED` Run） |
| 24 | GET | `api/run/get` | `Query<RunGetParams>` | `Run` | 不变 |
| 25 | POST | `api/run/cancel` | `Json<RunCancelPO>` | `Run` | 不变（新增 `用户取消执行` 原因文本，见 D-P2-029） |

`RunEventKind` 仍是 7 个取值（`run_started` / `message_delta` / `run_completed` / `run_failed` / `run_cancelled` / `server_shutdown` / `heartbeat`）；P2 只让 `run_failed` 从「无产出方」变为真实产出，`heartbeat` 保留不产出。

---

## 附：`docs/workspace.md` 变更清单（P2-005 产出，P2-030 回填）

| 位置 | 现状 | P2 落地后 |
|---|---|---|
| 3.1 `types-core` | 含 `session` 模块（契约类型） | 删除 `session`，保留 `runtime` / `probe`；新增 `config`（Provider / Model / Agent 配置，内部类型） |
| 3.1 `service-core` | 内存 `store` + Mock 生产方 + `holder` | Repository（SQLite）+ Service；`RunDriver` 执行端口 trait；`holder` 删除；`RuntimeState` 持 `SessionService` |
| 3.1 `lib-db` | 未创建 | 新增：连接池（WAL / `busy_timeout` / `foreign_keys`）、自有迁移 runner、`DbError` |
| 3.2 `agent-core` | 计划 | 新增：契约类型唯一来源、`RunStatus` 状态机、`AgentError` |
| 3.2 `agent-context-core` / `agent-context` | 计划 | 新增（P2 只做 System / History / User 三类来源与预算裁剪） |
| 3.2 `agent-route` | 计划；依赖列写着 `service-core` | 新增；**删除 `service-core` 依赖**（只消费 `types-core` 的配置类型） |
| 3.2 `agent-runtime` | 计划 | 新增：单轮 Agent Loop + `RunDriver` 实现（无 Tool Loop） |
| 4.4 holder 规则 | 泛述 | 补具体归属：`lib-web` 持 `OnceLock<AppState>`，`service-core` 内由 `RuntimeState` 持 `SessionService` |
| 5.1 依赖清单 | 已写目标边，带「P2 起」括号 | 去掉「P2 起」括号，改为既成事实；`bin-tests` 增 `agent-core`（契约类型迁移后） |
| 5.2 依赖图 | 缺 `service-core --> agent-core` 等新边 | 补 `servicecore --> agentcore`、`agentroute --> typescore`、`agentruntime --> servicecore`、`agentruntime --> agentroute`、`agentruntime --> agentcontext` |
| 5.3 / 5.4 约束 | 已含反向禁止 | 补：`agent-route` 不得依赖 `service-core`；`service-core` 对 `agent-*` 只允许 `agent-core` 类型层 |
| 6.2 / 6.3 类型归属 | 「P2 建 `agent-core` 后按 6.2 整体迁移」 | 改为既成事实 + 迁移时间点，并写明 Provider 配置类型留在 `types-core` |
| 8.1 / 8.1.1 | 「P2 计划、尚未实现」；8.1.1 仍写 `RunEventStream` 迁入 `agent-runtime` | 改为已实现；**删除 `RunEventStream` 条目**（P1.1 已移除该类型，事件生产方就是 `mpsc::Sender<RunEvent>`）；`holder` 一句改为「`lib-web` 新增、`service-core::holder` 删除」 |
| 8.2 | P2 行仍列出 6 个新 crate | 移除 P2 行，保留 P3–P6 |
| `docs/develop.md` 目录约定 | 只写 `data` / `config.toml` / `logs` | 补 `data/agent.db`（与 `data/events/` 并列） |

---

## OPEN 待确认项

| ID | 问题 | 现状 | 承接任务 |
|---|---|---|---|
| O-1 | drop `LlmStream` 后底层 HTTP 连接是否立即关闭（Rig 内部是否有后台任务继续读取） | **结案（源码级）**：`Streamed` 无后台任务持有读取流，drop 即释放 body 流；reqwest 侧「释放即关闭连接」为推断，未实测（影响面见 D-P2-001 的 O-1 结论） | P2-001、P2-016 |
| O-2 | `rig-core` 0.46 对编译时间与 release 体积的实际影响 | 未实测；只记录不设阈值 | P2-016、P2-032 |
| O-3 | `sqlx` 走 0.8（现状声明）还是升到 0.9 | **结案**：定格 0.8.6（0.9 的 feature 改名与展开见 D-P2-002） | P2-002、P2-010 |
| O-4 | `context_window` / `max_output_tokens` 的默认值（8192 / 1024）是否覆盖常见本地模型 | **结案**：8192 / 1024 只作缺省兜底；模型必须出现在 `[[provider.models]]`（D-P2-015 拒绝未知模型），因此实际预算由配置显式给出 | P2-002、P2-021 |
| O-5 | 排队中（Run 停在 `QUEUED`）在 TUI / WebUI 上如何提示最合适 | 未定；P2 只做「等待执行…」最小提示 | P2-027、P2-028 |

---

## 与 P1 / P1.1 决策的关系（只做引用，不修改其结论）

| 既有决策 | P2 的处理 |
|---|---|
| P1 D-017（P1 不引入 SQLite） | P2 按 D-P2-002 引入 `lib-db` + `sqlx`；D-017 的「P2 无论如何都要重做 Repository 层」得到执行 |
| P1 D-025（Mock 契约与 P2 替换点） | 替换点按计划落地；`holder` 由 D-P2-019 细化为「显式注入 + `RuntimeState` 持有 `SessionService`」 |
| P1 D-026（Streaming 契约） | `RunEvent` 建模与帧格式不变；「订阅即执行」「断线即取消」两条已由 P1.1 取代（D-P1.1-004） |
| P1 D-028（优雅关闭） | 关闭链路不变，P1.1 已把运行时会话挂进同一条链（D-P1.1-010） |
| P1 D-031 / D-032（生成物门禁、验收判据） | 不修改；A-1 由 D-P2-022 结案 |
| P1.1 D-P1.1-003（事件文件与轮转） | 不变；P2 只让事件内容变成真实执行产出 |
| P1.1 D-P1.1-004（Run 触发点与 `run/stream` 移除） | 不变；P2 的 D-P2-028 是其延续 |
| P1.1 D-P1.1-006（心跳/问询常量） | 不变；D-P2-020 作废 |
| P1.1 D-P1.1-007（重启不恢复占用） | 不变；P2 的自愈只处理数据库里的 Run 状态（D-P2-011） |
| P1.1 D-P1.1-010 / D-P1.1-011（关闭、取消收尾） | P2 在其上补「执行层发终态事件」（D-P2-029） |
