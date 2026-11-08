# P2 技术决策记录

每项记录包含：问题 / 背景与证据 / 可选方案 / 决策 / 理由 / 状态 / 承接任务。

状态定义（与 P1 一致）：

| 状态 | 含义 |
|---|---|
| `DECIDED` | 已确定，可直接执行 |
| `DECIDED (待验证)` | 方向已定，细节依赖某个任务实测确认 |
| `OPEN` | 未确定，动手前必须讨论或先做实验 |
| `REJECTED` | 已否决（保留记录，不删除） |
| `DEFERRED` | 本阶段不做，明确归属后续阶段 |

**重要**：本文件区分「已核实的代码事实」与「推断 / 假设」。标注「已核实」的来自当前仓库源码或 `rig-core 0.46` / `sqlx` 元数据阅读；未验证的一律 `OPEN`，不得写成 `DECIDED`。

---

## 决策索引

| ID | 主题 | 状态 |
|---|---|---|
| D-P2-001 | LLM 接入层：`rig-core` 0.46 + 自有 Adapter，不用 Rig 的 Agent Loop | DECIDED (待验证) |
| D-P2-002 | 持久化：`sqlx` 而非 `rusqlite` | DECIDED (待验证) |
| D-P2-003 | `sqlx` feature 组合（不启用 `macros`） | DECIDED |
| D-P2-004 | 数据库位置、连接参数与测试隔离 | DECIDED |
| D-P2-005 | 消息顺序用 `seq` 列；会话消息数用 `COUNT` | DECIDED |
| D-P2-006 | Run 执行由 `api/run/stream` 订阅触发 | DECIDED (已核实) |
| D-P2-007 | 同 Session 串行 + 全局并发上限 + 排队超时 | DECIDED |
| D-P2-008 | 持久化时序：流式不落库、终态与助手消息同事务 | DECIDED |
| D-P2-009 | 终态唯一提交由数据库条件更新裁决 | DECIDED |
| D-P2-010 | Cancellation 归属与传播 | DECIDED |
| D-P2-011 | 启动自愈：`RUNNING → FAILED`、`QUEUED → CANCELLED` | DECIDED |
| D-P2-012 | Context 预算来源与 Token 估算 | DECIDED |
| D-P2-013 | 历史裁剪策略与超限失败 | DECIDED |
| D-P2-014 | P2 不新增接口 / DTO 字段 / 事件枚举值 | DECIDED |
| D-P2-015 | 未知模型返回 400 | DECIDED |
| D-P2-016 | Provider 配置只走文件与环境变量，不新增 API | DECIDED |
| D-P2-017 | API Key 明文存配置文件 + 脱敏规则 | DECIDED |
| D-P2-018 | Crate 新增范围与四条依赖边调整 | DECIDED |
| D-P2-019 | `RunEventStream` 迁入 `agent-runtime`；`holder` 迁入 `lib-web` | DECIDED |
| D-P2-020 | 心跳间隔可配置（`AGENT_STREAM_HEARTBEAT_MS`） | DECIDED |
| D-P2-021 | 测试只在 `bin-tests`；LLM 用内置 TCP Mock Server | DECIDED |
| D-P2-022 | P1 遗留 A-1（生成物过期检测） | REJECTED |
| D-P2-023 | 流式部分内容持久化 | DEFERRED (P6) |
| D-P2-024 | SSE 断点续传与事件回放 | DEFERRED (P6) |
| D-P2-025 | Provider 配置 CRUD 接口 | DEFERRED (未排期) |
| D-P2-026 | API Key 加密存储 / 系统密钥库 | DEFERRED (P6) |
| D-P2-027 | Framework 依赖与 CI 策略（P2 不变） | DECIDED |

---

## D-P2-001 LLM 接入层：`rig-core` 0.46 + 自有 Adapter，不用 Rig 的 Agent Loop

- **问题**：P2 如何接入 Rig，才既不重复实现 Provider 协议，又不让 Rig 侵占自研 Agent Loop 的职责？
- **背景（已核实）**：
  - `rig` 当前最新为 0.46.0；facade crate `rig` 的 `default` 特性包含 `agent`（→ `rig-agent`），而 `rig-agent` 就是 Rig 自己的 Agent 运行时（`Agent` / `AgentBuilder` / `AgentRun` / `AgentRunner` / tool loop）。
  - `rig-core` 0.46.0 的 `default = ["derive", "rustls"]`，`reqwest` 是独立特性；OpenAI 兼容路径为 `providers::openai::{OpenAIConfig, Route, OpenAI}`，配置含 `api_key` / `base_url` / `route` 字段并有 `with_base_url` / `with_route` 构造器。
  - 完成调用的入口是 `driver::Model::{call, stream}`；流式返回 `Streamed<Completion>`（`Stream<Item = Result<Item<StreamEvent>, ProviderError>>`），可用 `finish()` 取 `CompletionResponse { choice, usage, ... }`；错误类型 `ProviderError` 含 `InvalidAuthentication` / `ProviderResponse` / `Http` / `Stalled` / `Truncated` / `Json`。
  - 仓库现状：`Cargo.toml` 无任何 Rig 依赖（P1 未引入）。
- **可选方案**：
  1. 依赖 facade `rig`（含 `agent`）——REJECTED：会把 Rig 的 Agent Loop 拉进依赖树，与「Agent Runtime 自研」直接冲突。
  2. 依赖 `rig-core`（`default-features = false, features = ["reqwest","rustls"]`），在 `agent-route` 内做 Adapter——**采用**。
  3. 自己实现 OpenAI 兼容 HTTP 客户端，完全不用 Rig——REJECTED：与项目既定决策「Rig 承担 Provider / Completion 基础设施」冲突，且重复实现协议。
- **决策**：
  1. 只依赖 `rig-core`，不依赖 `rig` facade；依赖声明固定为 `rig-core = { version = "0.46", default-features = false, features = ["reqwest", "rustls"] }`。
  2. `rig` 的类型只允许出现在 `crates/agent-route/src/rig/` 下；上层只见项目自有 `LlmProvider` / `LlmRequest` / `LlmStreamEvent` / `LlmError`。
  3. **禁止**使用 `rig_agent`、`AgentBuilder`、`AgentRun`、任何 tool loop API；Agent Loop 由 `agent-runtime` 实现（P2 只做单轮）。
  4. Provider 协议范围：P2 只支持 OpenAI 兼容（`Route::Chat`，即 `POST /chat/completions`）。Responses API、其他方言、Anthropic 等留给后续阶段。
- **理由**：职责边界最清晰（Rig = 传输 + 协议编解码；项目 = 循环 + 状态 + 持久化 + 事件），且不会因为 Rig 的运行时抽象而把 Run 生命周期交给第三方。
- **代价与风险**：`rig-core` 依赖面较大（`schemars`、`uuid`、`web-time`、`rig-http`、`rig-reqwest`），编译时间与体积上升；只记录数据不设阈值（P2-035）。
- **状态**：`DECIDED (待验证)`——构造 / 流式 / 错误映射细节由 P2-001 勘察、P2-016 实测确认。
- **承接任务**：P2-001（勘察）、P2-015（抽象）、P2-016（Adapter）、P2-035（记录体积/编译数据）。

---

## D-P2-002 持久化：`sqlx` 而非 `rusqlite`

- **问题**：P2 的 SQLite 驱动选谁？
- **背景（已核实）**：
  - `Cargo.toml` 的 `[workspace.dependencies]` **已声明** `sqlx = { version = "0.8", default-features = false, features = ["any","json","macros","runtime-tokio-rustls","sqlite"] }`。
  - `Cargo.lock` 中**没有** `sqlx`，任何 crate 都未引用它——即「预留未落地」。
  - `sqlx` 最新为 0.9.0（`rust-version = 1.94`）；当前声明为 0.8（最新 0.8.6）。
- **可选方案**：
  1. `sqlx`：原生 async、Tokio 一等公民、内置连接池与迁移（`migrate` 特性）、`sqlite` 特性自带 bundled amalgamation。
  2. `rusqlite`：同步 API，需要 `spawn_blocking` + 自建连接池与线程模型；迁移要自己写或额外用 `rusqlite_migration`。
  3. 两者共存——REJECTED：禁止同时引入两套数据库框架。
- **决策**：采用 `sqlx`；`rusqlite` 记为 `REJECTED`（理由：单进程多 Run 并发下需要手写阻塞线程模型与连接池，与既有 Tokio 运行时和「Repository + 事务」的写法不匹配，收益只有编译期轻量）。
- **理由**：与现有 Tokio + Repository 分层一致；迁移内建；bundled SQLite 免去系统库依赖，符合单 Binary 分发目标。
- **状态**：`DECIDED (待验证)`——版本与 bundled 编译在 P2-010 落地时确认（含 Windows 上是否需要 cc）。
- **承接任务**：P2-002、P2-010。

---

## D-P2-003 `sqlx` feature 组合（不启用 `macros`）

- **问题**：`sqlx` 的 feature 怎么选，才能既够用又不引入编译期数据库依赖？
- **背景（已核实）**：`sqlx` 的 `macros` 特性启用 `query!` / `query_as!` 编译期校验，需要 `DATABASE_URL` 或 `.sqlx` 离线数据；当前声明里带着 `macros`。
- **决策**：
  1. `features = ["runtime-tokio-rustls", "sqlite", "json", "migrate"]`，并去掉 `macros` 与 `any`（除非确实需要 `Any` 驱动）。
  2. 查询一律用运行时 API（`sqlx::query` / `query_as`），SQL 统一写在 `service-core` 的 `repository` 内。
- **理由**：CI 与本地构建不应需要数据库连接或离线缓存文件；Repository 层本来就要手写 SQL 与行映射，编译期校验收益有限。
- **状态**：`DECIDED`。
- **承接任务**：P2-002（写定）、P2-010（落地）。

---

## D-P2-004 数据库位置、连接参数与测试隔离

- **决策**：
  1. 数据库文件：`lib_core::directorys()?.data` 目录下 `agent.db`（release 在用户目录、debug 在可执行文件同级 `runtime`、`AGENT_HOME` 可整体覆盖）。
  2. 连接：`SqlitePoolOptions`，`max_connections` 取 5（单进程写入为主，读多写少）；`PRAGMA journal_mode=WAL`、`busy_timeout=5000`、`foreign_keys=ON`。
  3. 测试隔离沿用 P1：`bin-tests` 用 `AGENT_HOME` 指向 `target/tests/<运行 ID>/...`，不新增测试专用环境变量。
- **理由**：不新增配置面与路径约定，复用 P1 已验证的目录与隔离机制；WAL + busy_timeout 覆盖「同 Session 串行 + 多 Session 并发」的写入争用。
- **状态**：`DECIDED`。
- **承接任务**：P2-002、P2-010、P2-013。

---

## D-P2-005 消息顺序用 `seq` 列；会话消息数用 `COUNT`

- **问题**：消息顺序怎么保证？`SessionSummary.messageCount` 怎么维护？
- **背景（已核实）**：P1 用「按会话分桶的 `Vec<Message>`」保证写入顺序，`message_count` 在存储里累加。换成 SQL 后 `created_at_ms` 可能相同（同一毫秒写入用户消息与助手消息），不能作为排序依据。
- **决策**：
  1. `messages` 增列 `seq INTEGER NOT NULL`，`UNIQUE(session_id, seq)`；`seq = COALESCE(MAX(seq),0)+1` 在同一事务内分配；所有读取按 `seq` 升序。
  2. `SessionSummary.messageCount` 用 `COUNT(*)` 子查询，不维护冗余计数列。
- **理由**：`seq` 是唯一稳定顺序来源，且能顺带发现并发写漏洞（唯一约束会直接报错）；`COUNT` 避免「计数与真实行数漂移」这一类 bug，代价是可接受的小查询成本。
- **状态**：`DECIDED`。
- **承接任务**：P2-002、P2-011、P2-012。

---

## D-P2-006 Run 执行由 `api/run/stream` 订阅触发

- **问题**：P2 的 Run 什么时候开始执行——`message/create` 之后立刻，还是等 SSE 订阅？
- **背景（已核实）**：
  - `crates/service-core/src/session/service.rs::subscribe_run`：订阅时调用 `store.start_run`（带 `QUEUED` 守卫）把 Run 推进到 `RUNNING` 并启动生产任务；`message_create` 只写入 `QUEUED` 的 Run。
  - `crates/bin-tests/src/cases/session.rs::message` 明确断言「未订阅前 Run 应保持 `Queued`」。
  - `docs/api/session.md` 的状态推进一节写的是「订阅即把 Run 从 `QUEUED` 推进到 `RUNNING`」。
- **可选方案**：
  1. **订阅触发执行**（保持 P1 语义）：`run_stream` 是执行的唯一入口。
  2. 创建即执行（`message/create` 落库后立即排队执行）：语义更自然，但会立刻打破 P1 既有用例与已发布契约说明，且需要为「无订阅者时的 Run」定义事件缓冲与重连语义。
- **决策**：采用方案 1。`api/run/stream` 订阅 → 注册表原子占位 → 获得 Session 租约与全局信号量 → `QUEUED → RUNNING` → 启动执行任务 → 返回事件流。
- **理由**：P1 已有实现、用例与文档都以此为契约；P2 的约束是「P1 客户端不需要重写」，方案 1 是唯一无需改契约的选择。
- **代价（明确接受）**：
  1. 没有订阅者的 `QUEUED` Run 永不执行；重启时一律自愈为 `CANCELLED`（D-P2-011）。
  2. 一个 Run 只能订阅一次，断线即取消，不支持续看（D-P2-024 `DEFERRED`）。
- **状态**：`DECIDED`（依据已核实的代码与用例断言）。
- **承接任务**：P2-004、P2-022、P2-026。

---

## D-P2-007 同 Session 串行 + 全局并发上限 + 排队超时

- **问题**：同一次会话能否并发多个 Run？全局要不要限流？超限排队还是拒绝？
- **决策**：
  1. **同 Session 单 Run 串行**：每 Session 一个租约（`tokio::sync::Mutex` 的公平等待排队），第二个 Run 在第一个终态前保持 `QUEUED`，其订阅流只发心跳。
  2. **全局并发上限** `agent.max_concurrent_runs`（默认 4）：`Semaphore` 控制，超限排队（不拒绝请求）。
  3. **排队超时** `agent.run_queue_timeout_ms`（默认 120000）：到点把 Run 落 `FAILED`（`error` = 「执行排队超时」）并发 `run_failed`。
  4. 获取顺序固定「先 Session 租约，后全局信号量」，避免死锁。
- **理由**：同一 Session 的并发 Run 会让 Context 组装读到不稳定的历史（助手消息可能尚未写入），串行是唯一能保证「上下文可预测」的低成本方案；全局上限防止本机被 N 个并发 HTTP 流拖垮。
- **代价**：单会话吞吐受限；排队期间客户端只看到心跳，没有「排队中」事件（P2 不新增事件值，见 D-P2-014）。
- **状态**：`DECIDED`。
- **承接任务**：P2-004、P2-022、P2-033。

---

## D-P2-008 持久化时序：流式不落库、终态与助手消息同事务

- **问题**：Streaming 期间要不要频繁写库？助手消息何时创建？部分内容怎么保存？
- **决策（六条路径）**：
  1. **创建**（`api/message/create`）：单事务 `{INSERT 用户消息(seq), INSERT runs(QUEUED), UPDATE sessions.updated_at}`。
  2. **执行中**：**不写库**。增量只在内存累积并通过事件推送；事件携带的 `messageId` 在 `run_started` 之前生成（与 P1 一致），但该消息在成功收尾时才落库。
  3. **成功**：单事务 `{INSERT 助手消息(seq), UPDATE runs → COMPLETED + usage + finished_at, UPDATE sessions.updated_at}`；**事务提交成功后**才发 `run_completed`。事务失败则转走失败路径（尽力把 Run 落 `FAILED`）。
  4. **失败**：`UPDATE runs → FAILED`（`error` = 脱敏文案 + `finished_at`），**不写助手消息**，然后发 `run_failed`。
  5. **取消 / 断线**：`UPDATE runs → CANCELLED`（条件更新），**不写助手消息**，不发终态事件（P1 语义：调用方已通过 `api/run/cancel` 得到终态）。
  6. **服务关闭**：`UPDATE runs → CANCELLED` → 发 `server_shutdown`（`status=CANCELLED` + 可读 `error`）→ 关闭连接。
- **理由**：逐 delta 落库会产生大量小事务并放大 SQLite 写入争用，收益只有「断线后能看到部分内容」这一 P6 能力；保持 P1 的「不写半成品助手消息」语义则让「成功 Run 必有一条完整助手消息、失败 Run 必无」成为可断言的不变量。
- **状态**：`DECIDED`（部分内容持久化 `DEFERRED`，见 D-P2-023）。
- **承接任务**：P2-004、P2-012、P2-023、P2-025。

---

## D-P2-009 终态唯一提交由数据库条件更新裁决

- **问题**：取消与完成同时发生时谁获胜？如何保证终态只写一次？
- **决策**：
  1. 所有终态写入都是条件更新：`UPDATE runs SET status=?, ... WHERE id=? AND status IN ('QUEUED','RUNNING')`。
  2. 受影响行数为 0 = 已被他人提交终态 → 本次什么都不写、不发终态事件、静默退出。
  3. 成功路径额外要求 `WHERE status='RUNNING'`，保证 `QUEUED` 的 Run 不会被误判为完成。
  4. 内存侧不设「终态锁」；数据库是唯一裁决者（注册表只用于取消信号与单生产方占位）。
- **理由**：内存锁在崩溃、双进程、panic 后不可靠；条件更新是 SQLite 单写者模型下最简洁的正确解，且能顺带满足「终态不可再转」的领域规则（P2-007）。
- **状态**：`DECIDED`。
- **承接任务**：P2-012、P2-023、P2-025。

---

## D-P2-010 Cancellation 归属与传播

- **问题**：`CancellationToken` 谁持有？LLM 流怎么中断？SSE 断开与取消要不要关联？关闭怎么联动？
- **决策**：
  1. 每个 Run 一个 `CancellationToken`：执行任务持有，`RunRegistry` 存 clone；Run 结束（任意终态）时随 RAII guard 移除。
  2. `api/run/cancel` 的处理顺序：先由 `service-core` 做条件更新（数据库权威）→ 成功后 `runtime.cancel(run_id)` 触发 token → 执行器在 `select!` 的取消分支退出。
  3. LLM 流中断方式：`select!` 命中取消分支后 **drop `LlmStream`**，底层 HTTP 连接随之关闭（rig 侧「drop 即停」由 P2-016 实测确认）。
  4. **SSE 断开 = 取消**（保留 P1 D-026 第 5 条）：`sender.closed()` 与取消同权处理，终态 `CANCELLED`，未写的助手消息丢弃。
  5. 服务关闭：`lib-bin-web` 的关闭钩子在既有链路里追加 `runtime.shutdown()`（Runtime 级广播），执行器走「关闭收尾」分支（先写 `CANCELLED`，再发 `server_shutdown`）。
  6. panic：执行任务用 `JoinHandle` 结果 / `catch_unwind` 兜底，panic 时尽力把 Run 落 `FAILED` 并清理注册表。
- **理由**：与 P1 已有的三种终止语义（取消 / 断线 / 关闭）一一对应，客户端无需改动；把「中断模型调用」而不是「停止 SSE」作为取消的判据，避免后台继续烧 token。
- **状态**：`DECIDED`（「drop 是否立刻断连」为 `OPEN`，见 O-1）。
- **承接任务**：P2-004、P2-025、P2-032。

---

## D-P2-011 启动自愈：`RUNNING → FAILED`、`QUEUED → CANCELLED`

- **问题**：Server 崩溃或重启后，数据库里的 `RUNNING` / `QUEUED` 记录怎么办？
- **决策**：服务启动阶段（`migrate` 之后、开始监听之前）执行一次自愈：
  - `RUNNING → FAILED`，`error` = 「服务重启，执行已中断」，写 `finished_at_ms`。
  - `QUEUED → CANCELLED`，`error` = 「服务重启，未开始的执行已取消」，写 `finished_at_ms`。
- **理由**：执行任务随进程消失，不可能续跑（D-P2-024）；让残留状态进入**用户可理解的终态**比留在 `RUNNING` 更安全（否则会话列表永远显示「进行中」，且 `runningRunCount` 失真）。`RUNNING` 用 `FAILED` 而非 `CANCELLED`，因为用户并没有主动取消。
- **状态**：`DECIDED`。
- **承接任务**：P2-004、P2-012（`heal_orphans`）、P2-013、P2-014、P2-034。

---

## D-P2-012 Context 预算来源与 Token 估算

- **问题**：Token 预算从哪里来？怎么估算？
- **决策**：
  1. 预算来源：`[[provider.models]]` 的 `context_window` 与 `max_output_tokens`（配置显式给出，缺省 8192 / 1024）；P2 不引入模型目录或 tokenizer 下载。
  2. 估算：`tokens ≈ ceil(ascii_chars / 4) + non_ascii_chars + 4`（每条消息固定开销 4）。
  3. 预算：`available = context_window - max_output_tokens - safety_tokens(默认 256) - system_prompt_tokens`。
  4. 可替换：`TokenEstimator` trait + `HeuristicEstimator` 默认实现，构造点唯一（`agent-context`），后续可换成真实 tokenizer 而不改调用方。
- **理由**：不引入 tokenizer 依赖（体积与离线要求）；启发式对「中英混排」偏保守（CJK 按 1 字符 1 token 估），宁可提前裁剪也不越界。
- **状态**：`DECIDED`。
- **承接任务**：P2-003、P2-021、P2-031。

---

## D-P2-013 历史裁剪策略与超限失败

- **决策**：
  1. 先按 `agent.context_max_messages`（默认 40，约 20 轮）截断最旧消息。
  2. 再逐条丢弃最旧历史直到估算不超预算。
  3. **系统提示永不被裁**（最优先保留）；**最新一条用户消息必须保留**。
  4. 最新用户消息单条即超预算 → Run 落 `FAILED`，`error` = 「上下文超出模型预算」，**不发起上游请求**。
  5. 被裁掉的消息数记入 `Context` 的 metadata，只进日志。
- **理由**：显式失败比静默截断用户输入更诚实；「整条丢弃 + 保证最新输入」是最容易解释与验证的规则，也让 P3 的 Tool/Memory 贡献点有明确的插入位置。
- **状态**：`DECIDED`。
- **承接任务**：P2-003、P2-021、P2-031。

---

## D-P2-014 P2 不新增接口 / DTO 字段 / 事件枚举值

- **问题**：接入真实运行时后，契约要不要扩展？
- **背景（已核实）**：P1 的 `Message` / `Run` / `RunEvent` 字段已按「P2 需要」预留（`error`、`input_tokens`、`output_tokens`、`finished_at_ms`、`message_id`、`status`、`delta`）；`RunEventKind` 已有 `run_failed` 与 `heartbeat`，只是 Mock 不产出。
- **决策**：P2 **零新增**：19 条接口的 method / path / 入参 / 出参不变；DTO 字段不变；`RunEventKind` 保持 6 个取值。
- **理由**：P1-020 的验收与既有客户端都建立在这份契约上；「够用」的字段已经存在，为内部架构迁移改契约属于无收益风险。P3 需要 `tool_call_*` 时按「只追加枚举值」的方式扩展。
- **状态**：`DECIDED`。
- **承接任务**：P2-005、P2-008、P2-035。

---

## D-P2-015 未知模型返回 400

- **问题**：`SessionCreatePO.model` / 会话当前模型未在配置中出现时怎么办？
- **决策**：返回 400 `模型不可用：<id>`（沿用 `SessionError::Invalid` → 400 的既有映射，不新增错误码）。`model` 缺省时使用 `provider.default_model`。
- **理由**：P1 的 Mock 接受任意字符串（默认 `mock-model`）；P2 若静默回落到默认模型，用户会以为换了模型而实际没有。显式拒绝是最小、可读、可测的行为。
- **状态**：`DECIDED`。
- **承接任务**：P2-005、P2-017、P2-035。

---

## D-P2-016 Provider 配置只走文件与环境变量，不新增 API

- **问题**：Provider / Model 配置怎么管理？要不要暴露 HTTP 接口？
- **决策**：
  1. 配置来源：`<应用根目录>/config.toml` 的 `[provider]` / `[[provider.models]]` / `[agent]`，环境变量覆盖（清单见 `README.md`）。
  2. **不新增任何 Provider / Model 配置接口**；不做热更新（改配置需重启）。
  3. `service-core` 提供只读的 `provider` 服务（装载 + 校验 + 解析），`agent-route` 只消费。
- **理由**：P2 的范围是运行时而不是配置管理界面；一旦开口返回 Provider 配置，就要处理「密钥是否出现在响应里」这类安全设计，属于独立课题（D-P2-025 `DEFERRED`）。文件 + 环境变量足以支撑 P2 的全部验收。
- **状态**：`DECIDED`。
- **承接任务**：P2-005、P2-017。

---

## D-P2-017 API Key 明文存配置文件 + 脱敏规则

- **问题**：API Key 存在哪、怎么防止泄漏？
- **决策**：
  1. 存放：`config.toml` 的 `provider.api_key`（明文）或环境变量 `AGENT_PROVIDER_API_KEY`；环境变量优先。
  2. `api_key` **不进入任何 DTO / 响应**；`Secret` 类型的 `Debug` 与 `Display` 均为 `***`，取值只能通过显式 `expose()`。
  3. 日志：不记录请求头；Provider 相关日志只允许 `provider id` 与 `base_url` 的 host。
  4. 错误：`LlmError::public_message()` 为固定脱敏文案；Provider 原始错误只进 `source`（日志），且日志路径也不打印 key（构造错误时不得拼接 Authorization 头）。
  5. 文档提示：`config.toml` 含密钥，不得提交（`.gitignore` 已忽略本地配置文件；确认覆盖到该文件名）。
- **理由**：P2 单机单用户，系统密钥库（Windows Credential Manager / macOS Keychain / libsecret）会引入平台分支与额外依赖，收益不足；但「不泄漏」的工程措施必须现在就到位，否则后续很难补。
- **状态**：`DECIDED`（加密存储见 D-P2-026 `DEFERRED`）。
- **承接任务**：P2-005、P2-009、P2-017、P2-028、P2-035。

---

## D-P2-018 Crate 新增范围与四条依赖边调整

- **问题**：P2 新增哪些 Crate？哪些既有依赖方向要改？
- **决策**：
  1. 新增（均在 `docs/workspace.md` 8.2 的 P2 范围内）：`agent-core`、`agent-context-core`、`agent-context`、`agent-route`、`agent-runtime`、`lib-db`。
  2. 新增依赖边（属**目标架构**调整，需同步 `docs/workspace.md`）：
     - `service-core -> agent-core`：仅允许依赖这一**类型层**；仍禁止 `service-core` 依赖 `agent-runtime` / `agent-route` / `agent-context*`（原文档写的是「不依赖任何 `agent-*`」，需按此细化）。
     - `agent-runtime -> service-core`：Run / Message 的持久化规则属于 `service-core`，Runtime 通过其 Service 读写；方向仍为上层依赖下层。
     - `lib-web -> agent-core`、`lib-web -> agent-runtime`：DTO 来自 `agent-core`，事件流由 `agent-runtime` 产出。
     - `lib-bin-web -> service-core` / `agent-runtime` / `lib-db`：启动装配（对应 P1 D-025「改为启动时构造并注入」）；仍然不承载业务逻辑、不直接调用 Runtime 的业务方法。
  3. 不创建：`manager-core`、`types-api`、`bin-tui`、`bin-web`、`bin-mock-server`、`agent-tool`、`agent-mcp`、`agent-skill`。
- **理由**：契约类型迁入 `agent-core` 后，`service-core` 必然依赖它；持久化归 Service、执行归 Runtime 也是既有分工。把边界写成「允许 `agent-core`、禁止其余」比原文档的「全部禁止」更准确。
- **状态**：`DECIDED`（`docs/workspace.md` 由 P2-005 出清单、P2-006 / P2-008 / P2-010 / P2-013 / P2-026 分别在落地时同步）。
- **承接任务**：P2-005、P2-006、P2-008、P2-010、P2-013、P2-026。

---

## D-P2-019 `RunEventStream` 迁入 `agent-runtime`；`holder` 迁入 `lib-web`

- **问题**：事件流类型与进程级 holder 归谁？
- **背景（已核实）**：P1 的 `RunEventStream`（`mpsc::Receiver<RunEvent>` 的薄包装）与 `holder::session()` 都在 `service-core`；`holder` 是 `LazyLock<SessionService>` 单例，注释里写着「P2 移除」，但 `web_api` 的 Handler 不接受服务实例参数（只能 `Json<T>` / `Query<T>`），因此进程级访问点无法取消。
- **决策**：
  1. `RunEventStream` 迁入 `agent-runtime`（生产方在 Runtime，类型应与其同源）；`lib-web` 改用它组装 SSE，`session/stream.rs` 只改类型来源。
  2. `service-core::holder` 删除；新增 `lib-web::holder`（`OnceLock<Arc<SessionService>>` + `OnceLock<Arc<AgentRuntime>>` + `install(..)` / `session()` / `try_session()`），由 `lib-bin-web` 启动时注入。
  3. `probe_health` 在未安装时返回 `runningRunCount = 0`，保证 `lib-web` 的 example 服务仍可独立启动。
- **理由**：事件流属于运行时；holder 是 Web 层的进程级访问点（对应 `docs/workspace.md` 4.4「仅在确实需要跨调用共享状态时引入」）。这修订了 P1 D-025 的「holder 随之移除」表述——**改为「保留 holder 形态，但改为显式初始化」**。
- **状态**：`DECIDED`。
- **承接任务**：P2-013、P2-022、P2-026。

---

## D-P2-020 心跳间隔可配置（`AGENT_STREAM_HEARTBEAT_MS`）

- **问题**：P1 的心跳间隔硬编码 15 秒，E-3 无法在合理时间内自动验证。
- **决策**：心跳间隔由 `agent.stream_heartbeat_ms`（默认 15000）决定，环境变量 `AGENT_STREAM_HEARTBEAT_MS` 可覆盖；用例把它压到 300ms。生产默认值不变。
- **理由**：不新增事件、不改契约；把「可测性」做进配置而不是测试后门，且这个参数本身对慢模型场景有实际价值（可在网关超时较短时调大）。
- **状态**：`DECIDED`。
- **承接任务**：P2-024、P2-027。

---

## D-P2-021 测试只在 `bin-tests`；LLM 用内置 TCP Mock Server

- **问题**：P2 的 Provider 行为怎么测，且不依赖付费 API？
- **决策**：
  1. 所有用例只在 `crates/bin-tests`（仓库 `AGENTS.md` 第 28 条），其他 Crate 不写 `#[cfg(test)]`。
  2. LLM 侧用 `bin-tests` 内置的最小 HTTP/1.1 Mock Server（`tokio::net::TcpListener` 手写，不引入 Web 框架），提供 `POST /v1/chat/completions`（JSON 与 SSE）、`GET /v1/models`、`GET /mock/last-request`、`GET /mock/stats`。
  3. **不创建** `bin-mock-server`（P2 范围外）；该 Binary 的目标形态是「独立进程的外部供应商替身」，与本阶段「测试进程内的 Mock」不是同一件事。
- **理由**：手写最小 HTTP 避免让 `bin-tests` 依赖 `lib-web` / `service-core`（依赖约束），也让「非法 JSON、中途断连、静默 stall」这些场景可精确控制。
- **状态**：`DECIDED`。
- **承接任务**：P2-018、P2-027、P2-028、P2-031。

---

## D-P2-022 P1 遗留 A-1（生成物过期检测）

- **问题**：P1-020 遗留「A-1 生成物过期检测没有自动化方案」。
- **背景（已核实）**：生成物不入库（P1 D-006），门禁每次清空 `packages/` 后重建（`scripts/gen-check.sh`），因此不存在「仓库里的生成物与定义不一致」这一可检测状态；P1-020 已实测确认「改定义后不重新生成，`gen-check` 仍通过」。
- **决策**：A-1 的原始表述 **`REJECTED`**；等价门禁保留为「缺生成物 / 生成失败 / 产物不完整 / 生成代码不能编译 → 非 0 退出」，即现有 `make gen-check`。
- **理由**：判据在生成物不入库的前提下不可满足；继续追求会迫使生成物入库，与 D-006 冲突且带来合并冲突风险。
- **状态**：`REJECTED`。
- **承接任务**：P2-030。

---

## D-P2-023 流式部分内容持久化

- **问题**：Run 被取消或进程崩溃时，已生成的部分助手内容要不要落库？
- **决策**：P2 不做。取消 / 断线 / 崩溃一律丢弃已累积内容，Run 落终态且无助手消息。
- **理由**：与 P1 语义一致（不写半成品），且「部分内容」需要额外的消息状态机（draft / finalized）与前端渲染规则，属于 P6 的 Checkpoint / Resume 范畴。
- **状态**：`DEFERRED`（归属 P6）。
- **承接任务**：P2-004（记录限制）、P2-035（写入偏差说明）。

---

## D-P2-024 SSE 断点续传与事件回放

- **问题**：客户端断线后能否重连续看同一 Run？
- **决策**：P2 不支持。一个 Run 只能订阅一次；断线即取消（D-P2-010）。
- **理由**：需要事件缓冲 / 持久化事件日志 / 订阅者代际管理，并与「断线是否取消」重新定义，属 P6 的 Checkpoint / Resume。
- **状态**：`DEFERRED`（归属 P6）。
- **承接任务**：P2-004（记录限制）。

---

## D-P2-025 Provider 配置 CRUD 接口

- **问题**：是否在 P2 提供 Provider / Model 的增删改查接口（含密钥写入）？
- **决策**：不做。Provider 配置只走 `config.toml` + 环境变量（D-P2-016）。
- **理由**：需要同时解决密钥写入、脱敏返回、校验失败提示、热更新等一整套设计；P2 的验收不依赖它。
- **状态**：`DEFERRED`（归属阶段未定，需在 P3 之前评估是否与 Tool 配置一起设计）。
- **承接任务**：P2-005（记录）。

---

## D-P2-026 API Key 加密存储 / 系统密钥库

- **问题**：是否把 API Key 存进系统密钥库或加密后落盘？
- **决策**：P2 不做。明文 + 文件权限提示 + 严格脱敏（D-P2-017）。
- **理由**：跨平台密钥库引入平台分支、额外依赖与打包问题；当前威胁模型是单机单用户本地文件。
- **状态**：`DEFERRED`（归属 P6 的生产就绪）。
- **承接任务**：P2-005（记录）。

---

## D-P2-027 Framework 依赖与 CI 策略（P2 不变）

- **问题**：P2 是否处理 `framework-*` 本地 `path` 依赖导致的 CI 失败？
- **决策**：
  1. 不修改 Framework `path` 依赖，不提前切换 Version Dependency。
  2. 不修改 `.github/workflows/ci.yml` 以绕过问题，不为 CI 失败改无关代码。
  3. 本地必须完整执行 `make gen` / `make gen-check` / `make check` / `make ui-check` / `make test ARGS="--all"` 与前端用例，并把结果记入 P2-035 报告。
  4. CI 失败原因单独记录，明确「与 P2 功能无关，P6 Framework 发布正式版本后统一恢复 CI 验收」。
- **理由**：CI 失败与功能失败是两件事；为让 CI 变绿而动的改动（改 workflow、提前切版本）会把风险推迟到更晚阶段暴露。
- **状态**：`DECIDED`。
- **承接任务**：P2-005、P2-035。

---

## OPEN 待确认项

| ID | 问题 | 现状 | 承接任务 |
|---|---|---|---|
| O-1 | drop `LlmStream` 后底层 HTTP 连接是否立即关闭（Rig 内部是否有后台任务继续读取） | 未实测；影响 N-2 的判据强度 | P2-016 |
| O-2 | `rig-core` 0.46 对编译时间与 release 体积的实际影响 | 未实测；只记录不设阈值 | P2-016、P2-035 |
| O-3 | `sqlx` 走 0.8（现状声明）还是升到 0.9 | 未定；0.9 的 `rust-version = 1.94` 与当前工具链兼容，但升级需独立验证 | P2-002、P2-010 |
| O-4 | `context_window` / `max_output_tokens` 的默认值（8192 / 1024）是否足够覆盖常见本地模型 | 未实测；偏小会触发频繁裁剪 | P2-002/003 定默认值，P2-031 观察 |
| O-5 | 排队中（只收心跳）的 Run 在 TUI / WebUI 上如何提示最合适 | 未定；P2 只做「等待执行…」最小提示 | P2-029 |
| O-6 | `stream_heartbeat_ms` 是否进入正式配置文档（而非仅测试用） | 未定；当前先作为配置项（D-P2-020） | P2-035 |

---

## 与 P1 决策的关系（只做引用，不修改 P1 结论）

| P1 决策 | P2 的处理 |
|---|---|
| D-017（P1 不引入 SQLite） | P2 按 D-P2-002 引入 `lib-db` + `sqlx`；D-017 的「P2 无论如何都要重做 Repository 层」得到执行 |
| D-025（Mock 契约与 P2 替换点） | 替换点全部按计划落地；唯一修订是 holder「移除」改为「显式初始化」（D-P2-019） |
| D-026（Streaming 契约） | 事件与帧格式不变；断线即取消、订阅即执行、`run_completed` 前先落库三条语义继续有效 |
| D-028（优雅关闭） | 关闭链路不变，只在其钩子里追加 `runtime.shutdown()` |
| D-031 / D-032（生成物门禁、验收判据） | 不修改；A-1 由 D-P2-022 结案 |
