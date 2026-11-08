# P2：基础 Agent Runtime

**状态：已细化，未开工，等待用户确认本路线图（`tasks.md` / `acceptance.md` / `decisions.md`）**

> 本阶段在 P1 验收结论（[`docs/reports/p1-020-acceptance.md`](../../docs/reports/p1-020-acceptance.md)）与仓库实际代码之上细化。
> 凡与 P1 文档不一致之处，以代码为准并记入 `decisions.md`。

---

## 一、阶段目标

把 P1 的 Mock 执行换成**真实 LLM 驱动的 Agent Runtime**：可配置 Provider/Model、可多轮对话、可流式、可取消、可持久化、可恢复。

阶段结束时链路如下（**逻辑视图，不是 Cargo 依赖图**，依赖以 `docs/workspace.md` 为准）：

```text
CLI / TUI / WebUI
      │  HTTP + 生成 SDK
      ▼
   lib-web（Handler / SSE 编码，不变）
      │
      ▼
  service-core（Session / Message / Run 业务 + Repository）
      │                    │
      │                    └──► lib-db ──► SQLite（<AGENT_HOME>/data/agent.db）
      ▼
  agent-runtime（Run 调度 / Agent Loop / 事件生产 / 取消）
      ├──► agent-context ──► agent-context-core（Context 组装与预算）
      └──► agent-route ──► rig-core（Rig Adapter）──► LLM Provider（OpenAI 兼容）
```

---

## 二、阶段边界

### In Scope

- 真实 LLM 调用（OpenAI 兼容协议），Provider/Model 可配置。
- 自研 Agent Loop（单轮：Context → LLM → 助手消息 → 终态），**不实现 Tool Calling 循环**。
- Context 管理：System Prompt、历史消息、Token 预算、历史裁剪。
- Streaming：真实 token 流经既有 SSE 通路输出，复用 P1 的 `RunEvent` 契约。
- Run 生命周期、Run Cancellation、同 Session 串行与全局并发上限。
- SQLite 持久化：Session / Message / Run；Repository 抽象 + sqlx 实现 + 迁移。
- Server 重启后历史恢复，崩溃残留状态自愈。
- 把 P1 遗留项 E-3（心跳）、E-4（`run_failed`）、W-1（浏览器 console error 断言）、A-1（生成物过期检测结案）处置完毕。

### Out of Scope

| 不做的事 | 归属 |
|---|---|
| Tool Calling / Tool Registry / Tool 执行 | P3 |
| MCP | P4 |
| Skills | P5 |
| Memory、RAG、向量检索、高级 Context 压缩 | P6 |
| Multi-Agent、Workflow | P6 |
| Checkpoint / Resume、SSE 断点续传与事件回放 | P6 |
| Provider 配置的 HTTP CRUD 接口、密钥加密存储 | 未排期（见 `decisions.md` D-P2-025 / D-P2-026） |
| 多用户与权限体系 | 未排期 |
| 新增 Binary（`bin-tui` / `bin-web` / `bin-mock-server`） | 不创建 |
| 微服务、消息队列、分布式调度 | 不引入 |

### 与 P1 的硬约束

1. P1 的 19 条接口的 method / path / 入参 / 出参**不变**；`RunEvent` 的 6 个事件值与字段语义**不变**；错误契约 `{code, message}` 不变。
2. P2 **不新增接口、不新增 DTO 字段、不新增事件枚举值**（结论与依据见 `decisions.md` D-P2-014）。`run_failed` / `heartbeat` 从「契约存在但无产出方」变为「真实产出」。
3. P1 客户端（CLI / TUI / WebUI）的 API Transport 与 SDK 调用方式不需要重写，只允许增加**失败与排队状态展示**。
4. 生成物仍不入库；`make gen` 是唯一 SDK 来源。

---

## 三、Crate 变更

### 新增（P2 允许范围，见 `docs/workspace.md` 8.2）

| Crate | 职责 |
|---|---|
| `agent-core` | Agent 领域基础类型：Message / Role / Run / RunStatus / RunEvent / RunEventKind / AgentError / 状态机校验。**P1 落在 `types-core::session` 的契约类型整体迁入**（类型名、字段名、serde 行为不变）。 |
| `agent-context-core` | Context 抽象：Context / ContextItem / ContextSource / ContextPolicy / 贡献点。不依赖 Rig、Axum、数据库。 |
| `agent-context` | Context 组装实现：System Prompt、会话历史、角色映射、排序、过滤、Token 预算、历史裁剪。 |
| `agent-route` | Provider / Model 解析与 LLM 调用适配：`LlmProvider` 抽象 + Rig Adapter；Rig 类型不得泄漏。 |
| `agent-runtime` | Agent Loop、Run 调度与执行、事件生产、取消、超时、错误处理。 |
| `lib-db` | SQLite 基础设施：连接池、迁移、事务、错误；不含业务 SQL。 |

### 扩展

| Crate | 变更 |
|---|---|
| `types-core` | 移出 `session` 模块；新增 Provider / Model / Agent 配置类型（内部类型，不加 `auto_*` 宏）。 |
| `service-core` | `session` 改为 Service + Repository（SQLite）；新增 `provider` 配置服务；删除内存 `store`、Mock `stream` 与 `holder`。 |
| `lib-web` | `session` Handler 与 SSE 编码不变；新增进程内装配点 `holder`（`OnceLock`，由启动方注入）。 |
| `lib-bin-web` | 启动装配：配置 → `lib-db` → Repository → Service → Runtime → 注入 → 服务；关闭钩子联动 Runtime。 |
| `lib-bin-cli` / `lib-bin-tui` / `lingting-agent-ui` | 仅增加失败 / 排队 / 取消态展示，不改 Transport。 |
| `bin-tests` | 内置 OpenAI 兼容 Mock Server；新增 `stream.*` / `chat.*` / `run.*` / `persist.*` / `mockllm.*` 用例领域。 |

### 依赖边调整（需同步 `docs/workspace.md`，由 P2-005 落纸、P2-008 / P2-026 执行后复核）

| 边 | 理由 |
|---|---|
| `service-core -> agent-core` | 契约类型迁入 `agent-core` 后，Repository 与 Service 必须引用该类型。**仅允许依赖 `agent-core` 这一类型层**，仍禁止依赖 `agent-runtime` / `agent-route` / `agent-context*`。 |
| `agent-runtime -> service-core` | Run / Message 的持久化规则属于 `service-core`，Runtime 通过其 Service 读写，不自己写 SQL。方向仍为「上层依赖下层」，不构成反向依赖。 |
| `lib-web -> agent-core`、`lib-web -> agent-runtime` | Handler 的 DTO 来自 `agent-core`；SSE 事件流由 `agent-runtime` 产出。 |
| `lib-bin-web -> service-core` / `agent-runtime` / `lib-db` | 启动装配（对应 P1 D-025「改为启动时构造并注入」）；`lib-bin-web` 仍**不承载业务逻辑、不直接调用 Runtime 业务方法**。 |

`RunEventStream` 由 `service-core` 迁入 `agent-runtime`；`holder` 由 `service-core` 迁入 `lib-web`（`decisions.md` D-P2-019）。

---

## 四、技术选型

| 领域 | 选型 | 状态 |
|---|---|---|
| LLM 接入 | `rig-core` 0.46（`default-features = false, features = ["reqwest","rustls"]`），**不使用 `rig` facade 的 `agent` 特性**，不使用 Rig 的 Agent Loop | DECIDED（细节待 P2-001 / P2-016 实测） |
| 持久化 | `sqlx` 0.8（`runtime-tokio-rustls` / `sqlite` / `json` / `migrate`，**不启用 `macros`**） | DECIDED（P2-002 复核版本，P2-010 落地） |
| 数据库 | SQLite 单文件 `<AGENT_HOME>/data/agent.db`，WAL + `busy_timeout` | DECIDED |
| 并发与取消 | Tokio 任务 + `CancellationToken`（每 Run 一个，Registry 持有句柄）+ 全局 `Semaphore` + 每 Session 租约 | DECIDED |
| 事件 | 复用 P1 `RunEvent` + `lib-web-core::sse`；生产方从 `service-core` 换成 `agent-runtime` | DECIDED |
| Token 估算 | `TokenEstimator` trait + 启发式实现（ASCII 4 字符 ≈ 1 token，CJK 1 字符 ≈ 1 token），可整体替换 | DECIDED |

### 配置面（P2 定型，后续只追加）

`<应用根目录>/config.toml` 新增段落；环境变量覆盖同名项（**测试通过 `AGENT_HOME` + 环境变量注入**，不需要新增接口）：

```toml
[server]
addr = "127.0.0.1"
port = 20100
log_level = "info"

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
stream_heartbeat_ms = 15000
context_max_messages = 40
context_safety_tokens = 256
```

| 环境变量 | 覆盖项 |
|---|---|
| `AGENT_PROVIDER_BASE_URL` / `AGENT_PROVIDER_API_KEY` / `AGENT_PROVIDER_MODEL` | Provider 连接与默认模型 |
| `AGENT_MAX_CONCURRENT_RUNS` / `AGENT_RUN_QUEUE_TIMEOUT_MS` / `AGENT_LLM_IDLE_TIMEOUT_MS` | 并发与超时 |
| `AGENT_STREAM_HEARTBEAT_MS` | 事件流心跳间隔（默认 15000；用例靠它把 E-3 压到秒级） |
| `AGENT_CONTEXT_MAX_MESSAGES` / `AGENT_CONTEXT_SAFETY_TOKENS` | Context 预算 |

---

## 五、批次与任务

任务清单见 [`tasks.md`](tasks.md)，共 **35 个任务、8 个批次**。批次之间强序，批次内可并行（见每批说明）。

| 批次 | 主题 | 任务 | 出口 |
|---|---|---|---|
| A | 架构与技术决策 | P2-001 ~ P2-005 | 决策定稿、契约清单、workspace.md 变更清单 |
| B | Agent 领域模型与类型迁移 | P2-006 ~ P2-009 | `agent-core` 建立、契约类型迁移完成、SDK 语义零差异 |
| C | SQLite 与 Repository | P2-010 ~ P2-014 | 内存存储换成 SQLite，重启可恢复 |
| D | Provider 与 Rig Adapter | P2-015 ~ P2-018 | 可真实调用 OpenAI 兼容服务，Mock Server 就绪 |
| E | Context 管理 | P2-019 ~ P2-021 | Context 可组装、可裁剪、可替换估算 |
| F | Agent Runtime | P2-022 ~ P2-026 | 真实 Run 执行接入 Server，Mock 生产方下线 |
| G | SSE 与客户端集成 | P2-027 ~ P2-030 | E-3 / E-4 / W-1 / A-1 处置完毕 |
| H | 端到端验收 | P2-031 ~ P2-035 | 14 类验收条目全部通过并出报告 |

### 批次依赖

```text
A ──► B ──► C ──┐
        │       ├──► F ──► G ──► H
A ──► D ──┴──► E ┘
```

- **必须串行**：A → B → C → F → G → H；D 必须在 F 之前；E 必须在 F 之前。
- **可并行**：B 与 D（D 只读 `agent-core` 的 Run / Message 类型与 `types-core` 的配置类型）；C 与 E（C 只碰存储，E 只碰 Context）；D 内部的 P2-018（Mock Server）与 P2-015/016 可并行。
- **P2-030（A-1 结案）无代码依赖**，可在 G 之前任意时点完成。

### 阶段依赖

- **上游**：P1 的契约、错误码、SSE 通路、优雅关闭、测试入口全部复用，不改动。
- **下游**：P3 的 Tool System 依赖 P2 的 Agent Loop 与 `agent-context-core` 贡献点；P4 / P5 依赖 P3；P6 依赖前五阶段的可观察点（P2 的 `RunEvent`、Run 终态、持久化结构）。
- **Framework**：仍为本地 `path` 依赖，P2 不切换 Version Dependency、不改 GitHub Actions（见 `decisions.md` D-P2-027）。

---

## 六、验收方向

验收条目见 [`acceptance.md`](acceptance.md)，共 14 类（Build / Architecture / Domain Model / SQLite / Provider / Context / Agent Loop / Streaming / Cancellation / Concurrency / Recovery / API Compatibility / Security / End-to-End）。

阶段结论判据：

1. 真实 Provider（本阶段用 OpenAI 兼容 Mock Server，不要求付费 API）能完成多轮对话并流式输出。
2. 两个会话并发互不串扰；同一会话内 Run 串行。
3. 取消、断线、服务关闭三种路径下 Run 终态正确、且模型连接被真正关闭。
4. 服务重启后会话与消息完整恢复，崩溃残留的 Run 状态被自愈成可读终态。
5. P1 的 19 条接口与 `RunEvent` 契约零变更，`make gen` / `make gen-check` 通过。
6. 密钥不出现在日志与任何响应中。
7. `make test ARGS="--all"`、前端 `pnpm exec vp test run`、`pnpm exec playwright test` 全通过；`make ci` 若因未发布 Framework 依赖失败，不作为本阶段阻塞条件（但需记录失败原因与具体步骤）。

---

## 七、已知限制与风险（阶段内明确接受）

1. **Run 由订阅启动**：`api/run/stream` 订阅才触发执行（保留 P1 语义与既有用例断言，见 D-P2-006）。因此没有订阅者的 `QUEUED` Run 永不执行；重启后 `QUEUED` 一律落 `CANCELLED`。
2. **一个 Run 只能订阅一次**：断线即取消，不支持重连续看；断线前的已生成内容不落库。事件回放与续传归 P6。
3. **流式期间不写库**：助手消息只在终态事务里写入一次，进程崩溃会丢失该轮已生成内容（Run 落 `FAILED`）。
4. **同 Session 串行**会降低单会话吞吐；排队中的订阅在 UI 上只有「等待中」提示（心跳保活），P2 不引入排队事件。
5. **Token 估算为启发式**：预算裁剪结果可能与真实分词有偏差，仅保证不越界与可替换。
6. **Provider 配置无 HTTP 接口**：只能改 `config.toml` 或环境变量，改后需重启 Server（P2 不做热更新）。
7. **rig-core 依赖面较大**（`schemars` / `uuid` / `rig-http` / `rig-reqwest`），对编译时间与二进制体积的影响只记录、不设阈值（P2-035 记录数据）。
8. **CI**：`framework-*` 仍为本地 `path` 依赖，GitHub Actions 必然失败；本地编译、用例与功能验证仍必须完整执行并记录。
