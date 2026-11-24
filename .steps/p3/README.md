# P3：Tool System

**状态：规划期（尚未开工；32 个任务 / 8 个批次已细化，26 项决策全部定案）。** 任务与批次见 [tasks.md](tasks.md)，判据见 [acceptance.md](acceptance.md)，决策见 [decisions.md](decisions.md)。

> 依据：P2 与 P2.2 的验收结论（[`docs/reports/p2-032-acceptance.md`](../../docs/reports/p2-032-acceptance.md)、[`docs/reports/p2.2-016-acceptance.md`](../../docs/reports/p2.2-016-acceptance.md)）、`docs/workspace.md` 的 `agent-tool` 职责、`docs/develop.md` 的 API 契约规范，以及仓库实际代码（`crates/agent-runtime`、`crates/agent-context*`、`crates/agent-route`、`crates/service-core`、`crates/lib-db`、`crates/lib-web`、`crates/lib-bin-*`、`crates/bin-tests`）。凡文档与代码不一致之处，以代码为准并记入 [decisions.md](decisions.md)。
>
> 本阶段**不改 P2 的执行语义**（Run 状态机、同会话串行、事件落盘顺序、终态事务边界一律不动），只在既有 Loop 上插入「工具轮次」，并新增一个领域 Crate、一张审计表、一条查询接口与两类事件。

---

## 一、阶段目标

| 目标 | 内容 | 结案证据 |
|---|---|---|
| **G-1** 统一工具抽象 | `agent-tool` 提供 Tool Trait / Definition / Schema / Registry / Executor / Result / Permission / Error 九件套（Tool Trait 带「本地文件操作」声明：只读 / 只写 / 读写 / 不操作）；**MCP 工具与内置工具在 `agent-runtime` 眼里没有区别**（P4 的接口前提） | `cargo tree` 边界核对 + 源码核对：`agent-runtime` 只依赖 `agent-tool` 的 trait，不认识任何具体工具 |
| **G-2** 工具调用循环 | 模型返回工具调用 → 执行 → 结果入本轮 Context → 继续推理，可多轮；参数非法 / 超时 / 执行失败三种情况都回灌可读文本，Agent 不卡死 | 用例 `tool.loop` / `tool.invalid_args` / `tool.timeout` / `tool.failed` |
| **G-3** 并行与取消 | 一轮内多个工具并发执行、结果顺序稳定；取消 Run 中断正在执行的工具，不残留后台任务 | 用例 `tool.parallel` / `tool.cancel` |
| **G-4** 权限与审计 | 工具级权限（允许 / 拒绝 / 需确认），**拒绝的工具既不进模型可见清单也不被执行**；每次调用落审计记录，可按 Run 查询 | 用例 `tool.denied` / `tool.audit`；只读接口 `GET api/tool/audit/list` |
| **G-5** 五个内置工具 | `Calculator` / `CurrentTime` / `FileRead` / `FileSearch` / `HttpRequest`，各自带安全边界（路径白名单、域名白名单、输出截断） | 用例 `tool.builtin.*`；默认禁用清单可核 |
| **G-6** 客户端可观测 | 工具调用的开始 / 完成进 `api/client/stream`，TUI 与 WebUI 都能展示「正在调用工具」与结果状态 | 用例 `tool.events` + 前端类型检查 |
| **G-7** 确认交互 | `confirm` 权限的工具在下发确认事件后**挂起 Run**（回 `QUEUED`、让出并发许可、不阻塞同会话后续 Run），客户端经 `api/client/post` 回传决定后重新排队执行 | 用例 `tool.confirm.*` / `client.post`；挂起期间快照里 `running_now` 不含它 |

关系的说明：G-1 是全部目标的地基；G-2 / G-3 在 G-1 之上实现执行循环；G-4 依附于执行器（每次调用都要过权限与审计）；G-5 只是「按同一份抽象实现五个工具」，不引入任何新机制；G-6 消费 G-2 产出的事件；G-7 把 G-4 的 `confirm` 走完（挂起 / 唤醒 / 恢复），也是 P3 唯一同时改动 Run 生命周期与上行接口的部分。

---

## 二、阶段边界

### In Scope

- **新增 Crate `agent-tool`**（`docs/workspace.md` §3.2 已规划）：Tool Trait、Definition、Schema、Registry、Executor、Result、Permission、Error、Execution Context，以及五个内置工具（`builtin` 子模块）。
- `crates/agent-core`：`MessageRole::Tool` 的落库与事件字段（已有取值，P3 起真正使用）；Run 事件追加工具事件。
- `crates/agent-route`：`LlmRequest` 支持工具定义，`LlmMessage` 支持工具调用与工具结果，`LlmStreamEvent` 支持工具调用增量；Rig Adapter 完成翻译与聚合。
- `crates/agent-context-core` / `crates/agent-context`：`ContextSource::Tool`，本轮工具结果作为 Item 进入下一次请求。
- `crates/agent-runtime`：多轮工具循环、轮次上限、工具执行期间的取消与超时收尾。
- `crates/types-core`：`[agent] max_tool_rounds` 与新增 `[tools]` 配置段（超时、结果上限、权限、文件与网络白名单）。
- `crates/service-core` / `crates/lib-db`：`tool_calls` 表 + `messages` 的工具列 + `runs` 的挂起列（迁移 `0002`）、Repository、审计写入 / 按 Run 查询 / 挂起与唤醒。
- `crates/lib-web`：新增 1 条接口 `GET api/tool/audit/list`；`api/client/update` **重命名为** `api/client/post`（支持心跳与工具确认）；`docs/api/tool.md`、`docs/api/runtime.md` 与索引同步。
- `crates/types-core`：`ClientPostPO` / `ClientPostKind`（原 `ClientUpdate*`）与工具确认相关的 DTO。
- `crates/lib-bin-cli` / `crates/lib-bin-tui`：调用点改名；工具事件与确认提示的展示与上行。
- `crates/bin-tests`：`tool.*` 用例（含确认挂起 / 唤醒）；`client.*` 用例改名；`mock/llm` 支持返回工具调用与记录请求里的 `tools` 字段。
- `docs/workspace.md`：`agent-tool` 从「阶段：P3 规划」改为落地状态并补内置工具位置。

### Out of Scope

| 不做的事 | 说明 |
|---|---|
| MCP 协议接入 | P4。本阶段只保证「换一个 Tool 实现就能接进来」 |
| Skills 加载与 `SKILL.md` | P5 |
| 沙箱级隔离（容器 / WASM / seccomp / 进程隔离） | P6；本阶段的安全边界是白名单 + 超时 + 截断 |
| 工具市场、动态下载、第三方工具加载 | 不引入插件机制 |
| 确认交互的富形态 | 只做「一个确认一次」：不做批量确认、不做确认历史面板、不做「总是允许此工具」；确认超时也不做（D-P3-023） |
| 工具级 token 计费与配额 | P6 |
| 工具结果的向量化 / RAG 压缩 | P6 |
| Provider / Model 配置 CRUD | 沿用 P2：配置文件只读 |
| 新增 Web 框架、TUI 框架、数据库 | 沿用现有栈 |

### 与前序阶段的硬约束

1. **Run 触发点不变**：会话被占用 + 消息创建才执行（P1.1）；工具循环只发生在 `drive` 内部。
2. **事件通路不变**：先落盘 flush，再转发到 `api/client/stream`；工具事件与既有事件同源同序，`seq` 连续。
3. **终态边界收窄一条、其余不变**：最终助手消息与 `COMPLETED` 仍是同一事务提交（D-P2-008 保留）；**新增**「每轮边界落库」——进入下一轮之前，本轮的 assistant `tool_calls` 消息与工具结果消息必须已落库（D-P3-024 修订 D-P2-008 的「流式期间不写库」）。
4. **同会话串行 + 一个例外**：同一会话同一时刻最多一个 `RUNNING` Run；**挂起期间**（Run 回 `QUEUED` 且等待确认）该会话允许执行后续 Run（D-P3-023 明确口径，是 P2「同会话串行」的唯一例外，历史顺序因此可能交错）。
5. **Runtime 不知道具体工具**：`agent-runtime` 只持有 `ToolRegistry` + `ToolExecutor`，不 `match` 工具名。
6. **第三方协议类型不越界**：Rig 的 `ToolDefinition` / `ToolCall` 只在 `agent-route/src/rig/` 内出现。
7. **测试只写在 `bin-tests`**：其他 Crate 不得出现 `#[cfg(test)]`（`AGENTS.md` 第 28 条）。
8. **测试数据不从日志来**：新增断言的证据只来自客户端事件、接口响应与 `metrics` 快照（`AGENTS.md` 测试数据采集条款）。
9. **接口改名必须一次改到底**：`api/client/update` → `api/client/post` 是 P1.1 契约的破坏性变更，服务端、两侧 SDK、CLI、TUI、WebUI 与用例必须在同一个批次内同步，不允许两代并存（D-P3-022）。


---

## 三、现状与缺口（代码事实）

| 编号 | 现状 | 缺口 |
|---|---|---|
| C-1 | `agent-core` 已有 `MessageRole::Tool` 取值（注释写着「P3 起使用」），`RunEventKind` 注释已预留 `tool_call_started` | 取值没有任何生产点；`RunEvent` 没有工具字段 |
| C-2 | `agent-route` 的 `LlmRequest` 只有 `model` / `messages` / `max_output_tokens`；`LlmMessage` 只有 `role` + `content` | 无法把工具定义发给模型，也无法表达「助手请求调用」与「工具结果」两种消息 |
| C-3 | `agent-route/src/rig/stream.rs` 的注释写明「推理、工具调用等部分 P2 不消费，只转发答案文本」 | 工具调用增量被丢弃，`ToolCall` / `Arguments` 事件没有任何映射 |
| C-4 | `agent-runtime/src/loop_.rs` 是单轮：一次请求、一次流消费、一次收尾 | 没有「工具轮次」这一层；`RunInput` 也没有工具相关输入 |
| C-5 | `agent-context/src/builder.rs` 的 `history_item` 显式过滤 `SYSTEM` / `TOOL`（D-P2-013） | 本轮工具结果无法进入下一次请求的 Context |
| C-6 | `docs/workspace.md` 已规划 `agent-tool`，但仓库里没有该 Crate | 九件套全部缺失 |
| C-7 | `lib-db/migrations/0001_init.sql` 只有 `sessions` / `runs` / `messages` | 没有审计表；`messages` 也没有工具字段（按 D-P3-003 不打算加） |
| C-8 | `lib-web` 现有 25 条接口（不含 debug + `metrics` 限定的 `api/probe/metrics`），没有任何工具相关路径 | 审计记录没有查询入口 |
| C-9 | `bin-tests/src/mock/llm/reply.rs` 只产出纯文本（普通 + 流式），`/mock/last-request` 记录完整请求体 | 无法构造「模型要求调用工具」的剧本；也没有断言 `tools` 字段是否发出 |
| C-10 | `lib-bin-tui/src/worker.rs` 对 `RunEventKind` 的分支是穷尽的（含 `Heartbeat` 空实现） | 新增事件类型必须同时改这里与 WebUI 的 `useChat.ts`，否则 Rust 侧编译失败/TS 侧静默忽略 |
| C-11 | `types-core::config::AgentConfig` 是 `[agent]` 段的唯一来源，校验在 `lib-bin-cli::config` | 没有工具相关配置项（轮次、超时、结果上限、权限、白名单） |
| C-12 | `messages` 表只有 `id` / `session_id` / `run_id` / `role` / `content` / `model` / token / `seq` / `created_at_ms`，没有工具列 | 工具轨迹无法落库，挂起后的推理也无可恢复的事实源（D-P3-003 / D-P3-024 要补） |
| C-13 | Context 裁剪是「按条数 + 按预算整条丢弃最旧」（`agent-context/src/trim.rs`，D-P2-013） | 工具轨迹是「assistant tool_calls + N 条 tool 结果」一组，按条丢弃会留下孤儿 tool 结果（上游会 400） |
| C-14 | 上行接口只有 `POST api/client/update`，`ClientUpdateKind` 只有 `PING` / `INQUIRY_ACK` | 没有承载「工具确认」的上行消息，也没有 `toolCallId` / 决定字段（D-P3-022 / D-P3-023 要改） |
| C-15 | `runs` 表没有「等待什么」的列；Run 只能表达 `QUEUED` / `RUNNING` / 终态 | 挂起回 `QUEUED` 后无法区分「正常排队」与「等确认」，重启后也无从恢复（D-P3-023 要补列） |

---

## 四、技术方案

### 4.1 Crate 与依赖方向（G-1）

```text
agent-tool                （新增，Agent 领域层）
  ├── depends on: agent-core / agent-context-core / lib-core
  ├── depends on: serde / serde_json / schemars / tokio / async-trait / futures-util
  ├── depends on: framework-http（reqwest 特性，仅供 HttpRequest）
  └── 不依赖: agent-runtime / agent-route / rig-core / service-core / lib-web* / bin-*

agent-runtime  →  agent-tool（Registry + Executor + trait 对象）
agent-tool     ✗→  agent-runtime（工具不得反向依赖运行时）
```

- 内置工具放在 `agent-tool::builtin`，不新建 `agent-tool-builtin`。理由与备选见 D-P3-001。
- `agent-tool` 不认识 Rig：`ToolDefinition` 是本项目自己的类型，翻译成 `agent-route` 的 `LlmToolDefinition` 由 `agent-runtime` 做（一次映射，不双向耦合）。

### 4.2 Tool 抽象与 Schema（G-1）

```rust
// agent-tool/src/tool.rs
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;                 // 模型可见的稳定标识
    fn description(&self) -> &'static str;
    fn schema(&self) -> &'static ToolSchema;        // JSON Schema，来自 schema::of::<Args>()
    fn risk(&self) -> ToolRisk;                     // ReadOnly / Network / Write
    fn file_access(&self) -> ToolFileAccess;         // None / Read / Write / ReadWrite（本地文件操作行为）
    fn concurrency(&self) -> ToolConcurrency;       // 默认由 file_access 推导，工具可覆盖
    fn retryable(&self, error: &ToolError) -> bool; // 默认 false
    async fn call(&self, ctx: &ToolContext, args: serde_json::Value) -> Result<ToolOutput, ToolError>;
}
```

- **本地文件操作声明**（`ToolFileAccess`）：`None`（不操作本地文件）/ `Read`（只读）/ `Write`（只写）/ `ReadWrite`（读写）。它是工具的自述声明，不改变执行逻辑本身，只有两个用途：
  1. **推导 `ToolConcurrency` 的默认值**：`Write` / `ReadWrite` → `Exclusive`（串行，避免并发写互相踩），`None` / `Read` → `Parallel`；工具可以显式覆盖。
  2. **进 `ToolDefinition` 与审计记录**（`tool_calls.file_access`），事后可判断一次调用对本地文件的影响面。
- **权限判定不看它**：权限只由 `[tools.permissions]` 与 `ToolRisk` 决定（D-P3-005 不变）。声明是「这个工具会怎么动本地文件」的说明，不是授权依据（D-P3-021）。

- **参数类型是唯一来源**：每个工具声明一个 `*Args`（`serde::Deserialize` + `schemars::JsonSchema`），`schema()` 返回 `schema::of::<Args>()` 的结果，`call` 用 `serde_json::from_value::<Args>` 反序列化。schema 与校验因此不会两处维护（D-P3-002）。
- **校验就是反序列化**：不引入 `jsonschema` 校验库；`deny_unknown_fields` + 必填字段 + 类型不匹配都会在同一次反序列化里报错，错误文案由 serde 给出并被加工成模型可读文本。
- `ToolSchema` 是 `serde_json::Value` 的薄包装（`{"type":"object","properties":…,"required":…}`），同时提供契约快照 DTO（`auto_type`）供 SDK 与前端看到「有哪些工具、参数长什么样」。
- 工具名规则：`snake_case`、小写、`^[a-z][a-z0-9_]{1,63}$`；注册期校验，重名直接拒绝注册。

### 4.3 Registry 与权限（G-1、G-4）

```text
配置（[tools]）──► ToolPolicy ──┐
                                ├──► ToolRegistry（注册：内置 5 个）
会话/上下文 ────────────────────┘        │
                                         ├─ model_definitions()  → 发给模型的清单（按权限过滤 + 稳定排序）
                                         └─ lookup(name)         → 执行期查找
```

- `ToolRegistry` 是进程内只读表：装配期一次性注册，运行期不加锁写（`Arc<ToolRegistry>`）。
- **模型可见清单就是权限边界**：`deny` 的工具**不发给模型、不进入上下文**（模型根本不知道它存在），同时在执行期保留二次判定（模型仍可能凭空构造名字）。
- 权限判定顺序固定：`deny`（含未注册）> `confirm`（挂起等确认，见 §4.9）> `allow`（配置显式）> 工具默认（`ToolRisk` 的缺省等级）。

### 4.4 Executor：校验 → 权限 → 超时 → 取消 → 归一化（G-2、G-3、G-4）

```text
一次工具调用
  ① 查找工具（未注册 → DENIED「未知工具」）
  ② 权限判定（deny → DENIED，不执行、不触网；confirm → 交回 Loop 走挂起，见 §4.9）
  ③ 参数反序列化（失败 → INVALID_ARGUMENTS，错误文本回灌模型）
  ④ 执行：tokio::time::timeout(timeout_ms, tool.call(..))，与 cancel token 并列 select
  ⑤ 归一化：成功文本 / 错误文本 / 超时文本，统一截断到 max_result_chars
  ⑥ 产出 ToolCallRecord（审计）与 LLM 侧 tool 消息
```

- **超时**：单工具 `[tools] timeout_ms`（默认 15000）；超时视为失败的一种，不重试。
- **重试**：只有 `retryable(&error)` 为真且 `[tools] max_retries > 0` 时重试，默认 0（D-P3-008）。P3 内置工具里只有 `HttpRequest` 对「连接失败 / 超时」声明可重试。
- **取消**：`select!` 命中取消即 drop 工具 future，工具实现不感知取消（D-P3-019）。超时与取消的区别只体现在结局：超时是一次「工具失败」，Run 继续；取消是 Run 终止。
- **并行**：一轮内的 `Parallel` 调用 `futures_util::future::join_all` 并发执行，结果按**模型给出的调用顺序**回填（不是完成顺序）；`Exclusive` 调用把这一轮切成若干段，段间串行、段内并行（并发语义的默认值由工具的本地文件操作声明推导：`Write` / `ReadWrite` 默认 `Exclusive`，见 D-P3-021）。并发上限 `[tools] max_parallel_calls`（默认 8），超出部分排队。
- **归一化**：工具永远返回 `ToolOutput { text, is_error, truncated }`。错误也是「一段模型可读的文本」——OpenAI 兼容协议的工具结果没有 error 字段，错误只能靠文本表达。

### 4.5 Agent Loop 的多轮工具循环（G-2）

在 `agent-runtime` 把现在的单轮流程改成分轮：

```text
prepare（历史 + tools，含挂起恢复）
  └─ loop round in 1..=max_tool_rounds
       ├─ 请求 Provider（带 tools）
       ├─ 流式消费：文本增量 → message_delta；工具调用增量 → 聚合
       ├─ 无工具调用 → 收尾（落最终助手消息 + COMPLETED，同事务）
       └─ 有工具调用
            ├─ assistant tool_calls 消息落库（每轮边界）
            ├─ 逐个/并发执行 → tool_call_started / tool_call_completed
            │    └─ 命中 confirm 权限 → tool_call_confirm_required → 挂起 Run（回 QUEUED）
            ├─ 结果按条落库为 TOOL 消息（每轮边界）
            └─ 进入下一轮
  轮次用尽 → FAILED「工具调用轮次超过上限」
```

- **Context 每轮重建**：系统提示词 → 历史（含之前轮次已落库的工具轨迹）→ 本次输入 → 本轮新增的工具项。历史裁剪按「工具调用组」成对丢弃（D-P3-025）；任何一轮估算超预算即 `FAILED`「上下文超出模型预算」，不做「丢工具结果硬塞」。
- **每轮边界落库**：进入下一轮之前，本轮的 assistant `tool_calls` 消息与全部 TOOL 结果消息必须已落库（D-P3-024）。这是**修订** D-P2-008 的「流式期间不写库」：流式期间仍不写库，但轮次边界成为新的落库点。最终助手消息与 `COMPLETED` 仍在同一事务提交。
- **事件**：`tool_call_started` 在调用**开始执行前**发出（含序号、工具名、脱敏参数摘要），`tool_call_completed` 在得到结果后发出（含状态、耗时、错误文本）。三个事件都带 `tool_call_id`，客户端据此配对与展示。
- **挂起与恢复**：命中 `confirm` 的工具不执行，改为发 `tool_call_confirm_required` 并把 Run 回 `QUEUED`（写 `runs.pending_tool_call_id`，释放并发许可）；客户端经 `api/client/post` 回传决定后清空待确认列、重新排队。恢复时从 `messages` 的历史里认出「最后一个 tool_call 没有对应 TOOL 结果」，按决定执行工具或回灌拒绝文本（D-P3-023）。
- **终态边界**：`FAILED` / `CANCELLED` 不写最终助手消息；已落库的中间轮次消息保留（它们是已经发生的事实）。

### 4.6 事件与契约扩展（G-6）

`RunEventKind` 追加三个取值（`snake_case`，同时用于 SSE 的 `event:` 行）与 `ToolCallStatus` 枚举：

| 事件 | 载荷字段（`RunEvent` 追加的可选项） |
|---|---|
| `tool_call_started` | `toolCallId` / `toolName` / `toolArguments`（脱敏 + 截断的 JSON 文本）/ `toolRound` |
| `tool_call_completed` | `toolCallId` / `toolName` / `toolStatus` / `toolDurationMs` / `error`（失败时） |
| `tool_call_confirm_required` | `toolCallId` / `toolName` / `toolArguments`（脱敏 + 截断）/ `toolRound`；发出后 Run 回 `QUEUED` 等待确认（D-P3-023） |

`ToolCallStatus` 取值：`OK` / `ERROR` / `DENIED` / `TIMEOUT` / `CANCELLED` / `PENDING`（沿用默认 SCREAMING_SNAKE_CASE；`PENDING` 只出现在挂起等待确认期间，是审计表里的中间态，不会出现在 `tool_call_completed` 事件里）。既有字段语义一律不变（D-P3-010）。

### 4.7 审计落库与查询（G-4）

迁移 `0002_tool_calls.sql`：

```sql
CREATE TABLE IF NOT EXISTS tool_calls (
    id            TEXT    PRIMARY KEY,
    run_id        TEXT    NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    session_id    TEXT    NOT NULL,
    tool_call_id  TEXT    NOT NULL,
    tool_name     TEXT    NOT NULL,
    file_access   TEXT    NOT NULL,   -- None / Read / Write / ReadWrite（本地文件操作声明）
    status        TEXT    NOT NULL,
    arguments     TEXT    NOT NULL,   -- 脱敏 + 截断
    result        TEXT,               -- 截断后的结果 / 错误文本
    duration_ms   INTEGER NOT NULL,
    tool_round    INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    UNIQUE (run_id, tool_call_id)
);
CREATE INDEX IF NOT EXISTS idx_tool_calls_run ON tool_calls (run_id, created_at_ms);
```

- 写入点只有一个：`agent-runtime` 每次调用结束后调 `SessionService::record_tool_call`（失败只记日志，不影响 Run 结论——审计不该把成功 Run 打成失败）。
- 查询接口 `GET api/tool/audit/list?runId=…` → `Vec<ToolCallVO>`，只读、无分页（单 Run 的调用条数受轮次 × 并发限制约束）。
- 脱敏规则：键名命中 `api_key` / `apikey` / `token` / `secret` / `password` / `authorization` / `cookie`（大小写不敏感）的值替换为 `***`；`arguments` 整体上限 2048 字符；`result` 用 `[tools] max_result_chars`。
- 写入不是幂等的例外：`PENDING` 记录先插入（挂起时），确认后由同一条记录收尾（批准 → 执行结果覆盖为 `OK` / `ERROR`；驳回 → `DENIED`），`UNIQUE(run_id, tool_call_id)` + `ON CONFLICT … DO UPDATE` 保证只留一条。

同一迁移里还有两组列（D-P3-003 / D-P3-023）：

```sql
-- 工具轨迹进对话历史：TOOL 角色消息靠 tool_call_id 与助手的 tool_calls 配对
ALTER TABLE messages ADD COLUMN tool_call_id TEXT;
ALTER TABLE messages ADD COLUMN tool_name TEXT;
ALTER TABLE messages ADD COLUMN tool_calls TEXT;   -- 仅 ASSISTANT 消息：本轮的 tool_calls JSON

-- 挂起等待确认：非空表示「这个 Run 在等某个工具调用的确认」，此时 status 为 QUEUED
ALTER TABLE runs ADD COLUMN pending_tool_call_id TEXT;
ALTER TABLE runs ADD COLUMN pending_tool_round INTEGER;
CREATE INDEX IF NOT EXISTS idx_runs_pending ON runs (pending_tool_call_id);
```

- `MessageRole::Tool` 的消息必须带 `tool_call_id`；`ASSISTANT` 消息带 `tool_calls` 时 `content` 可以为空串。
- `pending_tool_call_id` 非空即挂起态：**不占全局并发许可**，也不阻止同会话后续 Run 执行（D-P3-023 的唯一串行例外）。
- 迁移在 `0001` 之后执行，只加列不改既有列；`0001` 的数据（无工具）保持可读。

### 4.8 内置工具与安全边界（G-5）

| 工具 | 参数 | 本地文件操作 | 默认权限 | 安全边界 |
|---|---|---|---|---|
| `calculator` | `expression` | `None` | allow | 自研递归下降，只支持四则、括号、取模与一元负号；不做函数、幂、变量（D-P3-015） |
| `current_time` | `zone?`（`utc` / `local`，默认 `utc`） | `None` | allow | 只读系统时钟；不做时区名解析（不引 tzdata） |
| `file_read` | `path` | `Read` | allow（限定白名单） | `canonicalize` 后必须落在 `[tools.file] allow_dirs` 前缀内；二进制拒绝；单次 ≤ `max_bytes` |
| `file_search` | `name` / `path?` / `max_results?` | `Read` | allow（限定白名单） | 同上白名单；遍历期剪枝固定排除目录（`target` / `node_modules` / `.git` / `.venv` 等）；默认 50 条上限 |
| `http_request` | `url` / `method?`（`GET` / `POST`）/ `body?` | `None` | **deny** | host 必须命中 `[tools.http] allow_hosts`；禁重定向；响应 ≤ `max_bytes`；只 http/https |

### 4.9 工具确认与挂起唤醒（G-7）

`[tools.permissions]` 里配成 `confirm` 的工具，模型请求调用时走一条「服务端挂起 → 客户端确认 → 重新排队」的链路（D-P3-023）：

```text
执行器判定 confirm
  ├─ 发 tool_call_confirm_required（toolCallId / 工具名 / 脱敏参数 / 轮次）
  ├─ tool_calls 插入 status=PENDING（参数已脱敏）
  ├─ runs.pending_tool_call_id = toolCallId；status: RUNNING → QUEUED
  ├─ 释放全局并发许可、结束本次 drive；会话队列可以继续跑后续 Run
  └─ 客户端 api/client/post { kind = TOOL_CONFIRM, toolCallId, decision }
        ├─ APPROVE：清 pending → 重新排队 → 恢复推理 → 执行该工具
        └─ REJECT ：tool_calls 置 DENIED → 清 pending → 重新排队 → 回灌「用户拒绝调用该工具」
```

约定：

- **接口改名**：上行接口从 `POST api/client/update` 改为 `POST api/client/post`，`ClientUpdateKind` → `ClientPostKind`，取值追加 `TOOL_CONFIRM`；请求体追加可空的 `toolCallId` 与 `decision`（`APPROVE` / `REJECT`）。`PING` / `INQUIRY_ACK` 的语义与响应体（`pongAtMs`）不变（D-P3-022）。
- **谁可以确认**：只能是当前占用该会话的客户端；未注册、非占用者、`toolCallId` 与该 Run 的 `pending_tool_call_id` 不一致，一律 400 + 固定文案。
- **不设超时**：确认没有时限（D-P3-023）；挂起的 Run 只能由「客户端确认」「取消 Run」「释放占用 / 客户端断连」「服务关闭」四条路径收尾，后三条按既有取消链路落 `CANCELLED` 并清 `pending_tool_call_id`。
- **重连补发**：客户端重新注册并建立 `api/client/stream` 后，服务端对该客户端占用会话下所有 `pending_tool_call_id` 非空的 Run 各补发一次 `tool_call_confirm_required`（否则确认入口会随断连永久丢失）。
- **不占并发位**：挂起期间该 Run 不持有全局许可，`metrics` 快照的 `running_now` 不包含它；这也是「同会话允许后续 Run 执行」的前提。
- **确认只有一个意义**：批准 = 允许这一次调用；没有「总是允许此工具」的记忆（不在 P3 范围内）。

### 4.10 配置面（新增）

```toml
[agent]
# 一轮 Run 内最多几轮工具调用（含第一次请求之后的所有轮次）。
max_tool_rounds = 8

[tools]
timeout_ms = 15000          # 单次工具调用超时
max_result_chars = 8000     # 单次工具结果回灌给模型的最大字符数
max_parallel_calls = 8      # 一轮内并行执行上限
max_retries = 0             # 可重试错误的重试次数上限（默认不重试）

# 权限：allow / deny / confirm（confirm = 挂起等客户端确认；没配的工具按默认等级）
# 注意：确认没有超时配置（D-P3-023）
[tools.permissions]
http_request = "deny"
file_read = "allow"

[tools.file]
allow_dirs = ["D:/code/lingting/lingting-agent"]   # 空 = 只允许应用根目录
max_bytes = 1048576

[tools.http]
allow_hosts = []            # 空 = 一律拒绝（默认禁用）
max_bytes = 262144
```

装载与校验仍在 `lib-bin-cli::config`（P2-017 的口径）：未知工具名、非法枚举值、负数、空 host 都是启动期硬错误（退出码 2）。

---

## 五、批次与任务

批次之间按序；同批次内可并行。每批结束时必须保持可编译、可运行。

| 批次 | 主题 | 任务 | 出口 |
|---|---|---|---|
| A | 工具抽象、Schema 与注册 | P3-001 ~ P3-004 | `agent-tool` 可注册五个工具并产出模型可见清单（执行器尚未接上） |
| B | 权限、执行器与审计类型 | P3-005 ~ P3-009 | 执行器可独立被驱动：校验、权限、超时、取消、截断、审计记录齐全 |
| C | Provider 与 Context 扩展 | P3-010 ~ P3-012 | 请求能带工具定义，流能产出工具调用，本轮工具结果能进 Context |
| D | Agent Loop、事件与挂起恢复 | P3-013 ~ P3-016 | 端到端多轮工具循环可跑，三类事件可观测，三种失败可回灌，挂起后能从历史恢复 |
| E | 持久化与接口 | P3-017 ~ P3-020 | 审计与工具轨迹落库、按 Run 查询、SDK 重新生成 |
| F | 五个内置工具 | P3-021 ~ P3-025 | 五个工具按安全边界落地，默认禁用清单可核 |
| G | 确认交互（接口改名与挂起唤醒） | P3-026 ~ P3-029 | `api/client/post` 可用；确认后 Run 重新排队并执行；TUI / WebUI 可确认 |
| H | 客户端展示与验收 | P3-030 ~ P3-032 | 工具调用可见；`tool.*` / `client.*` 用例全绿；阶段报告出稿 |

```text
A ──► B ──► C ──► D ──┐
        └──► E ───────┼──► G ──► H
F（内置工具只依赖 A/B 的 trait）──┘
```

- 强序：A → B（执行器依赖注册与权限）→ C（Provider / Context 扩展）→ D（Loop 才能跑通）→ G（确认链路依赖 D 的挂起点与 E 的落库）。
- 可并行：E 的落库部分只依赖「审计记录类型」（B 的产物）；F 只依赖 A / B 的 trait，可与 C / D / E 并行；G 的接口改名部分可与 D 并行，但「唤醒后重新排队」要等 D 与 E。
- H 必须最后：用例要覆盖前面全部批次。
- 任务清单见 [tasks.md](tasks.md)。

---

## 六、阶段验收方向

1. Agent 可自主选择工具完成需要外部信息的任务，并基于结果继续推理。
2. 工具参数非法、执行超时、执行失败三种情况都被正确回灌给模型，Agent 不会卡死。
3. 一轮内多个工具可并行执行，结果顺序稳定且可复现。
4. 取消 Run 能中断正在执行的工具，不残留后台任务。
5. 每次工具调用都有审计记录，可按 Run 查询。
6. 未授权的工具不会被调用（模型请求被拒绝并收到可读错误）。
7. 需确认的工具在客户端确认前不会执行：Run 挂起后回 `QUEUED`、不占并发位、同会话后续 Run 可继续；确认或驳回后重新排队并恢复推理。

以上七条已展开成可逐条取证的判据，见 [acceptance.md](acceptance.md)（分组 B / A / S / L / C / P / T / W / R / F）。

---

## 七、已知限制与风险

1. **确认没有超时，会被遗忘**：挂起的 Run 只能由「确认 / 取消 / 释放占用或断连 / 服务关闭」四条路径收尾（D-P3-023）。用户关掉客户端后，该 Run 会一直停在 `QUEUED` + 非空 `pending_tool_call_id`，直到有人取消它；服务端不会自动清理。
2. **挂起打破了「同会话串行」**：挂起期间会话允许执行后续 Run，因此同一会话的历史里可能出现「后发的 Run 先落消息」。这是 D-P3-023 的有意取舍（不占并发位优先），代价是历史顺序不再等于发起顺序；客户端按 `runId` 分组展示，不能假设全局序号。
3. **接口改名是破坏性变更**：`api/client/update` → `api/client/post` 会让 P1.1 时代的客户端（含已发布的 SDK）直接 404。P3 在同一个批次内改完服务端、SDK、CLI、TUI、WebUI 与用例，不保留旧路径（D-P3-022）；跨版本兼容不在本阶段范围内。
4. **工具轨迹进了历史，`messages` 表因此变宽**：`tool_call_id` / `tool_name` / `tool_calls` 三列只对工具消息有意义，对普通消息恒为 `NULL`。`0001` 的既有数据保持可读，但 `Message` 契约新增了三个可选字段（SDK / 前端同步）。
5. **挂起恢复依赖「历史里最后一个 tool_call 没有配对结果」这一推断**：只要有任何一步把 assistant `tool_calls` 与 TOOL 结果的配对写坏（例如落库失败但事件已发），恢复就会失败并落 `FAILED`。所以每轮边界落库是硬要求（D-P3-024），且落库失败必须让本轮失败而不是继续。
6. **审计表与 Run 生命周期绑定的代价**：`tool_calls` 用 `ON DELETE CASCADE` 挂在 `runs` 上，删除 Run 会一并丢审计。P3 没有「删除 Run」的接口，这是可接受的；一旦将来加删除功能，需要重新决策。
7. **截断是「模型看不到全量」**：`max_result_chars` 之外的内容只在审计里留同样的截断文本，不会留全量。宁可让模型知道「被截断」，也不让它以为读到的是全部（截断提示文本固定）。
8. **FileRead / FileSearch 的边界不是沙箱**：白名单是「应用的善意约束」，不是安全隔离。符号链接、挂载点、Windows 的短路径名、大小写不敏感盘符都可能把路径带出白名单——P3 用 `canonicalize` + 前缀匹配把常见情形挡住，真正的隔离留给 P6。
9. **HttpRequest 的风险面最大**：即使有域名白名单，它仍然是 SSRF 的入口（DNS 重绑定、白名单域名自身的内网跳转）。P3 的处置是「默认 deny + 禁重定向 + 域名白名单 + 超时与体积上限」，并在文档里写明这不是 SSRF 防护。
10. **并行会让 SQLite 写入更密集**：工具审计是每次调用一次 INSERT，一轮 8 个并行调用就是 8 次写。`lib-db` 是单写者，P2.1 的「锁内收尾」修复必须继续遵守：审计写入不得在会话锁内做。
11. **事件数量会显著上升**：一次多轮 Run 可能有几十条事件。下行通道容量（64）与事件文件写入路径在 P2 是足够的，但 `tool.events` 用例必须断言「事件顺序稳定、`seq` 连续」而不是只看条数。
12. **Rig 的工具桥接是新代码**：0.46 的 `StreamEvent::{Start, Arguments, End}` 与 `PartKind::ToolCall` 是 P2 未消费的路径，聚合逻辑（分片拼接、非法 JSON、缺 `End`）是本次最容易出错的地方，必须有一份纯映射的用例（`tool.stream`）兜住。
13. **`max_tool_rounds` 与排队超时的相互作用**：一个带工具的长 Run 会长时间占用全局许可，`stress` 类用例的容量假设不再成立。P3 不调整任何既有参数；带工具的容量评估留到 P6。
14. **文件操作声明是自述，不是校验**：`ToolFileAccess` 由工具作者声明，编译器不会验证「声明只读却写了文件」。它只用来推导并发默认值与记录影响面；想真正约束行为只能靠 P6 的沙箱。新增工具时必须人工核对声明与实现是否一致（判据 T-8）。
15. **测试执行受授权约束**：所有 `make test` / 单跑用例都需要明确授权。未授权时只推进代码与静态检查，判据保持未勾选。

---

## 八、决策状态

**全部定案，可以开工。** 26 项决策（D-P3-001 ~ D-P3-026）的候选方案、理由与承接任务见 [decisions.md](decisions.md)。

| 编号 | 事项 | 状态 |
|---|---|---|
| D-P3-001 | 内置工具与 Tool 抽象同处 `agent-tool`，不新建 crate | 已定案 |
| D-P3-002 | Schema 单一来源：`schemars` 派生 + serde 反序列化校验（不引 `jsonschema`） | 已定案（方案①） |
| D-P3-003 | 工具调用与结果**落 `messages` 表并进对话历史**；Context 裁剪按「工具调用组」成对丢弃 | 已定案（方案②） |
| D-P3-004 | `confirm` 走**完整确认交互**：`api/client/post` + 下发确认事件后挂起 + 客户端回传决定 | 已定案（方案③ + 方案调整） |
| D-P3-005 | 内置工具默认权限：`http_request` 默认 deny，其余 allow 且受白名单约束；**被拒绝的工具不进模型可见清单** | 已定案 |
| D-P3-006 ~ D-P3-021 | 并行与串行语义、截断、重试、轮次上限、事件字段、审计脱敏、查询接口、文件与网络边界、Calculator / CurrentTime 实现口径、取消传递、Context 裁剪口径、本地文件操作声明 | 已定案 |
| D-P3-022 | `api/client/update` 重命名为 `api/client/post`，并承载工具确认 | 已定案（来自 D-P3-004 的方案调整） |
| D-P3-023 | 挂起的落地口径：回 `QUEUED` + 新列、让出并发许可、不阻塞同会话后续 Run、不设超时、重连补发 | 已定案 |
| D-P3-024 | 每轮边界落库（修订 D-P2-008 的「流式期间不写库」） | 已定案（D-P3-003 方案②的必然结果） |
| D-P3-025 | `messages` 里的工具轨迹如何配对与裁剪 | 已定案（D-P3-003 方案②的必然结果） |
| D-P3-026 | 确认交互的客户端形态（TUI 一行 + `y`/`n`；WebUI 卡片 + 两个按钮） | 已定案 |
