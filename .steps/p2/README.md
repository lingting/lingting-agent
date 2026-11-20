# P2：基础 Agent Runtime

**状态：进行中（2026-11-20 开工）：Batch A（P2-001 ~ P2-005）已定稿，Batch B（P2-006 ~ P2-009）已完成，Batch C（P2-010 ~ P2-014）已完成。** 上一版方案建立在「Run 由 `api/run/stream` 订阅触发」的前提上（旧 D-P2-006），该前提已被 **P1.1** 移除；本版按 P1.1 的客户端连接与会话运行时模型重写。

> 依据：P1 验收结论（[`docs/reports/p1-020-acceptance.md`](../../docs/reports/p1-020-acceptance.md)）、P1.1 验收结论（[`docs/reports/p1.1-022-acceptance.md`](../../docs/reports/p1.1-022-acceptance.md)）与仓库实际代码。凡与既有文档不一致之处，以代码为准并记入 `decisions.md`。
> 任务见 [`tasks.md`](tasks.md)，判据见 [`acceptance.md`](acceptance.md)，决策见 [`decisions.md`](decisions.md)。

---

## 一、阶段目标

把 P1/P1.1 的 **Mock 执行**换成**真实 LLM 驱动的 Agent Runtime**：可配置 Provider/Model、可多轮对话、可流式、可取消、可持久化、重启可恢复。

**关键前提变化（相对上一版方案）**：Run 的执行触发、同会话串行、事件落盘与下行转发在 P1.1 已经实现并验收通过。P2 **不改这条通路**，只把它里面「产出 Mock 事件」的那一段换成真实 Agent Loop：

```text
CLI / TUI / WebUI
      │ HTTP + 生成 SDK                ▲  api/client/stream（客户端级下行流，P1.1）
      ▼                                │
   lib-web（Handler / 帧合成）──────────┘
      │
      ▼
  service-core ── Session / Message / Run Service + Repository（P2 改造）
      │   │
      │   └──► lib-db ──► SQLite（<AGENT_HOME>/data/agent.db）
      │
      │  RuntimeSession（P1.1：执行队列 + 事件文件 + 转发）
      │        │  执行端口（trait 定义在 service-core，P2 新增）
      │        ▼
      └── agent-runtime（Agent Loop / 事件生产 / 取消）
              ├──► agent-context ──► agent-context-core（Context 组装与预算）
              └──► agent-route ──► rig-core（Adapter）──► LLM Provider（OpenAI 兼容）
```

**逻辑视图，不是 Cargo 依赖图**；依赖以 `docs/workspace.md` 为准。方向要点：`service-core` 定义执行端口但**不依赖** `agent-*`；`agent-runtime` 依赖 `service-core`（实现端口 + 经其 Service 持久化）。

---

## 二、阶段边界

### In Scope

- 真实 LLM 调用（OpenAI 兼容协议），Provider / Model 可配置。
- 自研 Agent Loop（单轮：Context → LLM → 助手消息 → 终态），**不实现 Tool Calling 循环**。
- Context 管理：System Prompt、历史消息、Token 预算、历史裁剪。
- Streaming：真实 token 流经 P1.1 的事件通路（写者先落盘、再转发到 `api/client/stream`），复用 `RunEvent` 契约。
- Run 生命周期与取消：终态唯一提交、取消/断连/关闭三种终止、启动自愈。
- SQLite 持久化：Session / Message / Run；Repository 抽象 + sqlx 实现 + 迁移；重启后历史恢复。
- 全局并发上限与排队超时（同会话串行已由 P1.1 的执行队列保证）。
- P1 遗留项处置：E-4（`run_failed` 真实产出）、W-1（浏览器 console error 断言）、A-1（生成物过期检测结案）；E-3（心跳）已由 P1.1 的客户端心跳与 `: keepalive` 覆盖，本阶段只做结案说明。

### Out of Scope

| 不做的事 | 归属 |
|---|---|
| Tool Calling / Tool Registry / Tool 执行 | P3 |
| MCP | P4 |
| Skills | P5 |
| Memory、RAG、向量检索、高级 Context 压缩 | P6 |
| Multi-Agent、Workflow | P6 |
| Checkpoint / Resume、事件回放与断点续传、部分内容持久化 | P6 |
| Provider 配置的 HTTP CRUD、密钥加密存储 | 未排期（D-P2-025 / D-P2-026） |
| 多用户与权限体系 | 未排期 |
| 新增 Binary（`bin-tui` / `bin-web` / `bin-mock-server`） | 不创建 |
| 微服务、消息队列、分布式调度 | 不引入 |

### 与 P1 / P1.1 的硬约束

1. **接口零变更**：25 条接口（探针 12 条 + 会话/消息/Run 6 条 + P1.1 的客户端 4 条与占用 3 条）的 method / path / 入参 / 出参不变。
2. **事件零新增**：`RunEventKind` 的 7 个取值不变（P1.1 追加的 `run_cancelled` 已存在）；P2 不新增接口、不新增 DTO 字段、不新增枚举值（D-P2-014）。`run_failed` 从「契约存在但无产出方」变为「真实产出」。
3. **执行通路不变**：Run 只在会话被占用时由该会话的执行队列串行执行；事件必须写进运行时会话的生产方（`mpsc::Sender<RunEvent>`），不得绕过事件文件直接写 SSE。
4. **客户端形态不变**：注册 → 建下行流 → 占用 → 发消息 → 消费下行流；Transport 与 SDK 调用方式不重写，只允许增加失败 / 取消 / 排队状态的展示。
5. **生成物仍不入库**，`make gen` 是唯一 SDK 来源。

---

## 三、Crate 变更

### 新增（`docs/workspace.md` 8.2 的 P2 范围）

| Crate | 职责 |
|---|---|
| `agent-core` | Agent 领域基础类型：Message / Role / Run / RunStatus / RunEvent / RunEventKind / AgentError / 状态机校验。**P1 落在 `types-core::session` 的契约类型整体迁入**（类型名、字段名、serde 行为不变）。P1.1 的 `types-core::runtime`（客户端连接与占用）**不迁移**，它不属于 Agent 领域。 |
| `agent-context-core` | Context 抽象：Context / ContextItem / ContextSource / ContextPolicy / 贡献点。不依赖 Rig、Axum、数据库。 |
| `agent-context` | Context 组装实现：System Prompt、会话历史、角色映射、排序、过滤、Token 预算、历史裁剪。 |
| `agent-route` | Provider / Model 解析与 LLM 调用适配：`LlmProvider` 抽象 + Rig Adapter；Rig 类型不得泄漏。 |
| `agent-runtime` | Agent Loop、Run 执行、事件生产、取消、超时、错误处理；**实现 `service-core` 的执行端口**。 |
| `lib-db` | SQLite 基础设施：连接池、迁移、事务、错误；不含业务 SQL。 |
| `manager-agent` | Agent 装配、进程内运行时与跨 Service 编排：启动装配、进程级 holder（`AgentHolder`）、注册表 / 占用 / 事件文件 / 断连链 / 定时任务、Run 执行队列与事件生产、发送消息后唤醒执行队列、关闭入口（P2-013 起新增，P2-035 起接管运行时）。 |

### 扩展

| Crate | 变更 |
|---|---|
| `types-core` | 移出 `session` 模块（保留 `runtime`）；新增 Provider / Model / Agent 配置类型（内部类型，不加 `auto_*` 宏）。 |
| `service-core` | `session` 改为 Service + Repository（SQLite）；删除内存 `store`、Mock 生产方与 `holder`；**进程内 `runtime` 整体迁出到 `manager-agent`**（P2-035），本 Crate 只留业务与数据访问。 |
| `lib-web` | `session` / `runtime` Handler 与帧合成不变；Handler 改调 `manager-agent`（会话入口与运行时状态），不再直接依赖 `service-core`。 |
| `lib-bin-web` | 启动装配：调 `manager-agent::assemble` → `install` → 起服务；关闭钩子联动 Runtime（P1.1 已接）。装配细节（数据库、Repository、Service、运行时状态）下沉到 `manager-agent`。 |
| `lib-bin-cli` / `lib-bin-tui` / `lingting-agent-ui` | 只增加失败 / 取消 / 排队态展示，不改 Transport。 |
| `bin-tests` | 内置 OpenAI 兼容 Mock Server；新增 `llm.*` / `chat.*` / `run.*` / `persist.*` 用例领域。 |

### 依赖边调整（需同步 `docs/workspace.md`，由 P2-005 出清单、落地任务执行后复核）

| 边 | 理由 |
|---|---|
| `service-core -> agent-core` | 契约类型迁入 `agent-core` 后，Repository 与 Service 必须引用该类型。**仅允许这一类型层**，仍禁止依赖 `agent-runtime` / `agent-route` / `agent-context*`。 |
| `agent-runtime -> service-core` | Run / Message 的持久化规则属于 `service-core`，Runtime 通过其 Service 读写；同时在该方向实现 `service-core` 定义的执行端口。方向仍为「上层依赖下层」。 |
| `lib-web -> agent-core`、`lib-web -> agent-runtime` | Handler 的 DTO 来自 `agent-core`；运行时会话的事件来源经 `service-core` 的端口注入，`lib-web` 只在启动装配期接触 `agent-runtime` 的类型（若为空则不必依赖，落地时以 `cargo tree` 为准）。 |
| `manager-agent -> service-core` / `lib-db` | 启动装配与进程级 holder（P2-013 起）；`service-core` 不反向依赖它，运行时状态与会话服务的互相引用在此层解决。 |
| `lib-web -> manager-agent`、`lib-bin-web -> manager-agent` | Handler 的会话业务入口与装配结果；启动方只调装配入口，装配细节（数据库 / Repository / Service / 运行时状态）留在 `manager-agent`。 |

**不再需要的迁移**：P1.1 已删除 `RunEventStream` 与 `lib-web/src/session/stream.rs`，因此不存在「事件流类型迁入 `agent-runtime`」这件事；事件生产方就是传给执行端口的 `mpsc::Sender<RunEvent>`（见 D-P2-028）。

---

## 四、技术选型

| 领域 | 选型 | 状态 |
|---|---|---|
| LLM 接入 | `rig-core` 0.46（`default-features = false, features = ["reqwest","rustls"]`），**不使用 `rig` facade 的 `agent` 特性**，不使用 Rig 的 Agent Loop | DECIDED（细节待 P2-001 勘察 / P2-016 实测） |
| 持久化 | `sqlx` 0.8（`runtime-tokio-rustls` / `sqlite` / `json` / `migrate`，**不启用 `macros`**） | DECIDED（P2-002 复核版本，P2-010 落地） |
| 数据库 | SQLite 单文件 `<AGENT_HOME>/data/agent.db`，WAL + `busy_timeout` | DECIDED |
| 并发与取消 | Tokio 任务 + `CancellationToken`（每 Run 一个）+ 全局 `Semaphore`；**同会话串行由 P1.1 的执行队列保证** | DECIDED |
| 事件 | 复用 `RunEvent` + P1.1 的运行时会话（事件文件 + `api/client/stream`）；生产方从 `service-core` 的 Mock 换成 `agent-runtime` | DECIDED |
| Token 估算 | `TokenEstimator` trait + 启发式实现（ASCII 4 字符 ≈ 1 token，CJK 1 字符 ≈ 1 token），可整体替换 | DECIDED |

### 配置面（P2 定型，后续只追加）

`<应用根目录>/config.toml` 新增段落；环境变量覆盖同名项（**测试通过 `AGENT_HOME` + 环境变量注入**，不需要新增接口）：

```toml
[server]
addr = "127.0.0.1"
port = 20100
log_level = "info"

[runtime.events]
# P1.1 已存在：事件文件会话目录数与总字节数上限
max_sessions = 200
max_bytes = 536870912

[provider]
id = "default"
kind = "openai-compatible"          # P2 只支持 OpenAI 兼容协议
base_url = "https://api.openai.com/v1"
api_key = ""                        # 明文；也可由 AGENT_PROVIDER_API_KEY 提供（D-P2-017）
default_model = "gpt-4o-mini"

[[provider.models]]
id = "gpt-4o-mini"
context_window = 128000
max_output_tokens = 4096

[agent]
max_concurrent_runs = 4
run_queue_timeout_ms = 120000
llm_idle_timeout_ms = 60000
context_max_messages = 40
context_safety_tokens = 256
```

| 环境变量 | 覆盖项 |
|---|---|
| `AGENT_PROVIDER_BASE_URL` / `AGENT_PROVIDER_API_KEY` / `AGENT_PROVIDER_MODEL` | Provider 连接与默认模型 |
| `AGENT_MAX_CONCURRENT_RUNS` / `AGENT_RUN_QUEUE_TIMEOUT_MS` / `AGENT_LLM_IDLE_TIMEOUT_MS` | 并发与超时 |
| `AGENT_CONTEXT_MAX_MESSAGES` / `AGENT_CONTEXT_SAFETY_TOKENS` | Context 预算 |

> 连接保活不新增配置：下行流的 `: keepalive` 间隔与客户端心跳都是 P1.1 的常量（15s，D-P1.1-006）。

---

## 五、批次与任务

任务清单见 [`tasks.md`](tasks.md)，共 **32 个任务、8 个批次**。批次之间强序，批次内可并行（见每批说明）。

| 批次 | 主题 | 任务 | 出口 |
|---|---|---|---|
| A | 架构与技术决策 | P2-001 ~ P2-005（已完成） | 决策定稿、契约清单、`workspace.md` 变更清单 |
| B | Agent 领域模型与类型迁移 | P2-006 ~ P2-009 | `agent-core` 建立、契约类型迁移完成、SDK 语义零差异 |
| C | SQLite 与 Repository | P2-010 ~ P2-014（已完成） | 内存存储换成 SQLite，重启可恢复、残留自愈 |
| D | Provider 与 Rig Adapter | P2-015 ~ P2-018 | 可真实调用 OpenAI 兼容服务，Mock LLM Server 就绪 |
| E | Context 管理 | P2-019 ~ P2-021 | Context 可组装、可裁剪、可替换估算 |
| F | Agent Runtime 接入 | P2-022 ~ P2-026 | 真实执行接到 P1.1 的执行端口，Mock 生产方下线 |
| G | 客户端与遗留项 | P2-027 ~ P2-030 | TUI/WebUI 展示失败与取消、P1 遗留项处置、文档回填 |
| H | 端到端验收 | P2-031 ~ P2-032 | 15 类验收条目全部通过并出报告 |

### 批次依赖

```text
A ──► B ──► C ──┐
       │        ├──► F ──► G ──► H
A ──► D ──┴──► E ┘
```

- **必须串行**：A → B → C → F → G → H；D 必须在 F 之前；E 必须在 F 之前。
- **可并行**：B 与 D（D 只读 `agent-core` 的 Run / Message 类型与 `types-core` 的配置类型）；C 与 E（C 只碰存储，E 只碰 Context）；D 内部的 P2-018（Mock Server）与 P2-015 / P2-016 可并行。
- **P2-030（A-1 结案、文档回填）无代码依赖**，可在 G 内任意时点完成。

### 阶段依赖

- **上游**：P1 的契约、错误码、测试入口；P1.1 的客户端注册与心跳、会话占用、执行队列、事件文件与轮转、下行流、关闭链路全部复用，**不改其语义**。
- **下游**：P3 的 Tool System 依赖 P2 的 Agent Loop 与 `agent-context-core` 贡献点；P4 / P5 依赖 P3；P6 依赖前五阶段的可观察点（`RunEvent`、Run 终态、持久化结构、事件文件）。
- **Framework**：仍为本地 `path` 依赖，P2 不切换 Version Dependency、不改 GitHub Actions（D-P2-027）。

---

## 六、验收方向

验收条目见 [`acceptance.md`](acceptance.md)，共 15 类（Build / Architecture / Domain Model / SQLite / Provider / Context / Agent Loop / Run Integration / Streaming / Cancellation / Concurrency / Recovery / API Compatibility / Security / End-to-End）。

阶段结论判据：

1. 真实 Provider（本阶段用 OpenAI 兼容 Mock Server，不要求付费 API）能完成多轮对话并流式输出。
2. 事件仍经 P1.1 的通路：先落盘 flush，再转发到 `api/client/stream`；两个会话并发互不串扰，同一会话内 Run 串行。
3. 取消、断连、服务关闭三种路径下 Run 终态正确、模型连接被真正关闭、事件文件有终态事件。
4. 服务重启后会话与消息完整恢复，崩溃残留的 Run 被自愈成可读终态；占用不恢复（P1.1 语义），客户端重新占用即可继续。
5. 25 条接口与 `RunEvent` 契约零变更，`make gen` / `make gen-check` 通过。
6. 密钥不出现在日志与任何响应中。
7. `make test ARGS="--all"`、前端 `pnpm exec vp test run`、`pnpm exec playwright test` 全通过；`make ci` 若因未发布 Framework 依赖失败，不作为本阶段阻塞条件（但需记录失败原因与具体步骤）。

---

## 七、已知限制与风险（阶段内明确接受）

1. **无占用不执行**：Run 只在会话被占用时执行（P1.1、D-P1.1-004），未被占用的会话里 Run 永远停在 `QUEUED`；重启后占用不恢复，客户端需重新注册并重新占用（D-P1.1-007）。
2. **事件文件与数据库不做补偿**：事件文件是事件事实源，数据库是消息与状态的事实源；两者之间没有对账机制（属 P6 的 Checkpoint / Resume 范畴）。
3. **流式期间不写库**：助手消息只在终态事务里写入一次；取消 / 断连 / 崩溃会丢弃该轮已生成内容（Run 落 `CANCELLED` / `FAILED`）。
4. **同会话串行**降低单会话吞吐；排队期间客户端只看到 Run 停在 `QUEUED`（P2 不新增排队事件），全局排队超时到点落 `FAILED`。
5. **Token 估算为启发式**：预算裁剪结果可能与真实分词有偏差，仅保证不越界与可替换。
6. **Provider 配置无 HTTP 接口**：只能改 `config.toml` 或环境变量，改后需重启 Server（P2 不做热更新）。
7. **rig-core 依赖面较大**（`schemars` / `uuid` / `rig-http` / `rig-reqwest`），对编译时间与二进制体积的影响只记录、不设阈值（P2-032 记录数据）。
8. **CI**：`framework-*` 仍为本地 `path` 依赖，GitHub Actions 必然失败；本地编译、用例与功能验证仍必须完整执行并记录。
