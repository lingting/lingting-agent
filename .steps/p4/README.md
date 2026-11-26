# P4：MCP Integration

**状态：已完成（2026-11-26；20 个任务 / 5 个批次，23 项决策全部定案）。** 任务与批次见 [tasks.md](tasks.md)，判据见 [acceptance.md](acceptance.md)，决策见 [decisions.md](decisions.md)，阶段验收报告见 [`docs/reports/p4-020-acceptance.md`](../../docs/reports/p4-020-acceptance.md)。

> 依据：P3 的验收结论（[`docs/reports/p3-032-acceptance.md`](../../docs/reports/p3-032-acceptance.md)）、`docs/workspace.md` 的 `agent-mcp` 职责、`docs/develop.md` 的 API 契约规范、`docs/api/readme.md` 的路径动作词表，以及仓库实际代码（`crates/agent-tool`、`crates/agent-runtime`、`crates/manager-agent`、`crates/lib-web`、`crates/types-core`、`crates/lib-bin-cli`、`crates/bin-tests`）。凡文档与代码不一致之处，以代码为准并记入 [decisions.md](decisions.md)。
>
> 本阶段**不改 P2 / P3 的执行语义**：Run 状态机、同会话串行、事件落盘顺序、终态事务边界、工具轮次与审计写入点一律不动。MCP 只是「工具的另一个来源」，接进 P3 已有的注册表与执行器。
>
> **开工前提已核实（2026-11-25）**：`rmcp` 最新可用版本为 **3.11.0**（`cargo add rmcp --dry-run` 可解析），客户端所需特性 `client` / `transport-child-process` / `transport-streamable-http-client-reqwest` 均存在，stdio 与 streamable HTTP 两种 transport 都已实现。风险项 R-1 因此降级为「实现期核对」。

---

## 一、阶段目标

| 目标 | 内容 | 结案证据 |
|---|---|---|
| **G-1** 协议隔离 | MCP 协议类型（`rmcp::*`）只出现在 `agent-mcp`；`agent-runtime` / `agent-route` / `agent-core` / 装配层之外的业务 Crate 不认识 MCP | `cargo tree` 与源码检索（判据 A-1 ~ A-4） |
| **G-2** 双 transport | stdio（子进程，stdin/stdout）与 streamable HTTP（远程）两种 server 都能连接、握手、发现工具 | 用例 `mcp.discover` / `mcp.http`（判据 T-1 ~ T-4） |
| **G-3** 工具发现与适配 | 连接后拉取 `tools/list`，经命名映射与 schema 透传变成内部 `ToolDefinition`，与内置工具同构地进入注册表与执行器；`file_access` 由配置声明（缺省 `None`；精确名 + `*` 通配模式）并据此推导并发 | 用例 `mcp.discover` / `mcp.name` / `mcp.schema` / `mcp.file_access`（判据 S-1 ~ S-7） |
| **G-4** 生命周期 | 子进程启动、握手超时、异常退出检测、退避重连、启用/禁用、服务关闭时终止子进程且无孤儿残留 | 用例 `mcp.crash` / `mcp.reconnect` / `mcp.shutdown`（判据 L-1 ~ L-8） |
| **G-5** 调用语义 | MCP 工具调用走 P3 的执行器：权限、超时、取消、截断、审计、事件全部复用，失败回灌可读文本而不是挂死 | 用例 `mcp.call` / `mcp.denied` / `mcp.confirm` / `mcp.cancel` / `mcp.audit`（判据 C-1 ~ C-9） |
| **G-6** 状态可见 | server 的连接状态、工具数量、最近错误可经只读接口与管理接口观察到；不新增 Run 事件 | 用例 `mcp.api` + `metrics` 快照的 `mcp` 段（判据 I-1 ~ I-6） |
| **G-7** 错误归一 | 传输错误、协议错误、工具执行错误统一归入 P3 的 `ToolError` / `ToolCallStatus`，文案固定、不带凭证与栈 | 用例 `mcp.call_error` / 文案表逐条核对（判据 E-1 ~ E-5） |

关系的说明：G-1 是全部目标的地基（协议不出 crate）；G-2 产出一个可用的连接；G-3 把连接变成工具；G-4 让工具在进程长时间存活期间保持可用；G-5 保证 MCP 工具与内置工具在运行时眼里完全同构；G-6 / G-7 只做可观测与错误收敛，不引入新机制。

---

## 二、阶段边界

### In Scope

- **新增 Crate `agent-mcp`**（`docs/workspace.md` §3.2 已规划）：MCP Client、transport、工具发现、命名映射、`Tool` 适配器、server 生命周期管理、状态机与错误归一化、进程内状态快照。
- `crates/agent-tool`：`Tool` 契约放宽（动态名字与外部 schema）、`ToolRegistry` 运行期可变（注册 / 注销 / 条目默认权限）。
- `crates/types-core`：新增 `mcp.toml` 的配置类型（`McpConfig` / `McpServerConfig` / `McpTransportConfig` / 权限与并发枚举）。
- `crates/lib-core`：应用目录增加 `mcp.toml` 路径（与 `config.toml` 同级）。
- `crates/lib-bin-cli`：`mcp.toml` 的装载与校验（未知 transport、空 command、非法 URL、重复 `id`、非法枚举值、空文件都是启动期硬错误，退出码 2）。
- `crates/manager-agent`：装配期连接 enabled 的 server、把发现的工具注册进注册表、关闭时终止子进程、`mcp` 指标转写、管理接口的业务入口。
- `crates/lib-web`：新增 3 条接口 `GET api/mcp/server/list` / `GET api/mcp/tool/list` / `POST api/mcp/server/update`；`docs/api/mcp.md` 与索引同步。
- `crates/bin-tests`：`mock/mcp` 测试替身（`bin-tests` 的隐藏子命令既是 MCP server 也是替身控制面）、`mcp.*` 用例。
- `docs/workspace.md`：`agent-mcp` 从「阶段：P4」改为落地状态并补依赖事实。

### Out of Scope

| 不做的事 | 说明 |
|---|---|
| 把本项目实现为 MCP **Server** | 排到 P6 之后单独评估（阶段 README 原有 Non-goals 保留） |
| MCP 的 `prompts` / `resources` / `sampling` / `elicitation` | 只接 `tools` 能力；其余留给有明确需求的后续阶段 |
| MCP Server 的安装、包管理、版本升级 | 只负责连接已存在的 server（`command` 由用户配置） |
| OAuth / 动态客户端注册 / 鉴权协商 | HTTP transport 只支持静态请求头（`headers`） |
| 跨机器的 MCP 编排、server 之间的依赖与编排 | 单个 server 独立连接，互不感知 |
| 工具级黑白名单、按 server 过滤工具 | 见 §七 风险 8；不在本阶段提供配置面 |
| MCP 的管理 UI（TUI / WebUI 面板） | 只提供 HTTP 接口与文档；工具调用本身经既有工具事件展示 |
| MCP 调用的细粒度进度（`notifications/progress`） | 忽略该通知；调用仍受 `[tools] timeout_ms` 约束 |
| 新增 Run 事件类型 | 沿用 `tool_call_started` / `tool_call_completed` / `tool_call_confirm_required` |
| 沙箱级隔离（容器 / WASM / 进程隔离） | P6；本阶段的安全边界是「显式 `enabled` + 配置只读 + 权限等级 + 超时」 |

### 与前序阶段的硬约束

1. **协议类型不越界**：`rmcp` 只出现在 `agent-mcp` 的依赖与源码里；`cargo tree -i rmcp` 只允许 `agent-mcp` 与 `bin-tests`。
2. **运行时不认识 MCP**：`crates/agent-runtime` 不引入任何 MCP 概念（判据 A-5 用 `git diff` 与源码检索双重核对）；运行时只按 `ToolRegistry` / `ToolExecutor` 行事。若因 `Tool` 契约或注册表返回类型变化必须做签名适配，只允许最小改动并在验收报告中单列。
3. **Run 语义不动**：触发点（会话被占用 + 消息创建）、同会话串行、事件先落盘再转发、每轮边界落库、终态事务边界一律保持 P1.1 ~ P3 的口径。
4. **工具失败即回灌**：MCP 连接失败、握手超时、server 崩溃、调用报错都必须变成模型可读文本并让 Run 继续或正常收尾，不允许挂死、不允许 panic、不允许把传输层原文交给模型。
5. **启动不被 MCP 拖死**：任一 server 连不上都不能阻止服务启动（MCP 是增强能力，不是启动依赖）。
6. **关闭必须收干净**：服务关闭时先让 Run 落终态、再关 MCP 连接、最后杀子进程；不允许留下孤儿进程。
7. **配置文件只读**：`mcp.toml` 是 MCP server 的唯一持久化来源；管理接口只改进程内状态，不写回配置、不落库（D-P4-008）。
8. **测试只写在 `bin-tests`**：其他 Crate 不得出现 `#[cfg(test)]`（`AGENTS.md` 第 28 条）。
9. **测试数据不从日志来**：新增断言只取接口响应、客户端事件与 `metrics` 快照（`AGENTS.md` 测试数据采集条款）。

---

## 三、现状与缺口（代码事实）

| 编号 | 现状 | 缺口 |
|---|---|---|
| C-1 | `docs/workspace.md` 已规划 `agent-mcp`（依赖 `agent-tool` / `agent-context-core` / `lib-core`，使用 `rmcp`） | 仓库里没有该 Crate；`Cargo.toml` 的 members 与 `workspace.dependencies` 里都没有 |
| C-2 | `agent-tool` 的 `Tool` 契约：`fn name(&self) -> &'static str` / `description() -> &'static str` / `schema() -> &'static ToolSchema` | MCP 工具名与说明来自 server（运行期字符串），`&'static` 只能靠 `Box::leak` 满足，等于按 server 数量永久泄漏内存 |
| C-3 | `ToolSchema` 只有 `of::<A: JsonSchema>()` 一个构造入口，且会剥掉 `$schema` / `title` / `$defs` | 没有「把外部 JSON Schema 原样装进来」的入口；MCP 的 `inputSchema` 必须能透传 |
| C-4 | `ToolRegistry` 是 `BTreeMap<String, Arc<dyn Tool>>`，`register` 只在装配期调用一次，运行期整表只读（`Arc<ToolRegistry>`） | 工具发现是运行期的异步事件（连接成功、`tools/list_changed`、重连），注册表必须能增删 |
| C-5 | `ToolRegistry::lookup` 返回 `Option<&Arc<dyn Tool>>`（借用注册表内部） | 增删发生在另一处时无法安全借用；查找要改为克隆 `Arc` |
| C-6 | 权限只有两档输入：`[tools.permissions]` 的显式值、`ToolRisk` 的缺省等级（`ToolPolicy::permission_of`） | MCP 的 server 级权限（allow / confirm / deny）无处表达：模型可见的工具名在配置装载时还不存在 |
| C-7 | `ToolFileAccess` 由工具自述（`None` / `Read` / `Write` / `ReadWrite`），并据此推导 `ToolConcurrency` 的缺省值 | MCP 工具的行为在 server 里，本进程无从声明；P4 改为「由配置声明（精确名 + `*` 通配模式，缺省 `None`）」，并据此推导并发（D-P4-006） |
| C-8 | `types-core::config` 只有 `[provider]` / `[agent]` / `[tools]`；`lib-bin-cli::config` 的校验只覆盖这三段 | `mcp.toml` 既没有类型也没有装载校验（含 `file_access` 的模式键） |
| C-9 | `manager-agent::build_tools` 是同步函数，只注册五个内置工具；`assemble_with` 之后运行期没有工具变更点 | 没有异步的 MCP 装配点，也没有「连接成功后把工具塞进注册表」的路径 |
| C-10 | `AgentRuntime.tools: Arc<ToolRegistry>`，`ToolExecutor::new` 持有同一份 `Arc` | 注册表内部可变后两者仍共用一个实例；但 `ToolExecutor` 的权限判定只看 `ToolPolicy`，需要接住「条目默认权限」 |
| C-11 | `lib-web` 现有 26 条接口（不含 debug + `metrics` 限定的 `api/probe/metrics`），没有 MCP 路径；`docs/api/` 没有 `mcp.md` | server 状态与工具清单没有查询入口，启用/禁用/重连/探测没有触发入口 |
| C-12 | `metrics` 快照（`manager-agent::runtime::metrics`）有 `chain` / `stop` / `journal` / `limit` / `audit` 等段 | 没有 MCP 段：连接数、工具数、调用失败与重连次数不可观测 |
| C-13 | `bin-tests/src/mock/` 只有 HTTP 型 LLM 替身（`mock/llm`）；`bin-tests` 是唯一的测试入口二进制 | 没有任何会讲 MCP 的子进程，stdio transport 无法被端到端验证 |
| C-14 | `manager-agent::holder::shutdown` 只停客户端与运行时会话 | 没有子进程终止点；MCP 子进程会在服务退出后残留 |
| C-15 | `Cargo.toml` 无 `rmcp` 依赖，`Cargo.lock` 无对应条目 | 需联网拉取（版本与特性已核实：3.11.0，见 §八）；构建成本需实测 |
| C-16 | `agent-tool::ToolCallRecord` 的 `tool_name` 是自由字符串 | 审计表不需要新列（工具名前缀已能定位 server），但前缀规则必须在文档与用例里固定 |

---

## 四、技术方案

### 4.1 Crate 与依赖方向（G-1）

```text
agent-mcp                 （新增，Agent 领域层）
  ├── depends on: agent-tool / lib-core
  ├── depends on: rmcp（client + transport-child-process + transport-streamable-http-client-reqwest）
  ├── depends on: tokio（process / sync / time） / async-trait / futures-util / tokio-util
  ├── depends on: serde / serde_json / tracing
  ├── depends on: framework-proc-auto / framework-proc-core（契约 DTO 的 `auto_type`，collect 特性）
  └── 不依赖: agent-runtime / agent-route / rig-core / service-core / lib-db / types-core / lib-web* / bin-*

agent-tool     ✗→  agent-mcp（工具层不认识 MCP，只认 `Arc<dyn Tool>`）
agent-runtime  ✗→  agent-mcp（运行时不改，判据 A-5）
manager-agent   →  agent-mcp（装配、关闭、指标转写、管理接口入口）
lib-web         →  manager-agent（不直接依赖 `agent-mcp`）
bin-tests       →  agent-mcp（用例直接核对状态机）+ rmcp（`server` 特性，仅测试替身）
```

- `agent-mcp` 不依赖 `types-core`：`mcp.toml` 的配置由装配层读成 `agent-mcp::McpConfig` 再喂进来（与 `agent-tool` 处理 `[tools]` 的口径一致）。理由与备选见 D-P4-001。
- `rmcp` 只在 `agent-mcp` 与 `bin-tests` 出现；`bin-tests` 用 `server` 特性做测试替身，正式构建不含该特性。
- 契约 DTO（`McpServerVO` / `McpToolVO` / `McpServerUpdatePO`）放在 `agent-mcp`，由 `manager-agent` 转发给 `lib-web`（与 P3 的 `ToolCallVO` 同一条路径，D-P3-019）。

### 4.2 配置面（独立文件 `mcp.toml`）

MCP 的配置**独立成文件**：应用根目录下的 `mcp.toml`（与 `config.toml` 同级），不写进 `config.toml`。
理由：它随 server 数量增长、改一次就要重启服务，与主配置的装载节奏不同；客户端命令也不必认识它。
文件内容就是这张表本身（顶层字段 + `[[servers]]`），不再套一层 `[mcp]`。

```toml
# mcp.toml（应用根目录，与 config.toml 同级）
# 单 server 握手超时（毫秒）：连接 + initialize 的总上限，超时即 Failed（不阻塞服务启动）。
connect_timeout_ms = 10000
# 失败后的自动重连：初始退避、2 倍增长、上限；上限写 0 表示关闭自动重连。
reconnect_initial_backoff_ms = 1000
reconnect_max_backoff_ms = 30000
# 未声明写操作的工具的并发缺省值（server 可覆盖）；声明 write / read-write 的工具恒串行，不能被放松。
concurrency = "parallel"

[[servers]]
id = "filesystem"            # ^[a-z][a-z0-9_]{0,31}$，工具名前缀的来源，全局唯一
enabled = false              # 缺省 false：仅写配置不会启动子进程（D-P4-013）
transport = "stdio"          # stdio | http
command = "npx"              # stdio 必填
args = ["-y", "@modelcontextprotocol/server-filesystem", "D:/code"]
env = { LOG_LEVEL = "info" } # 可选，仅追加到子进程环境
cwd = ""                     # 可选，相对应用根目录；空表示不切换
permission = "allow"         # allow | confirm | deny，缺省 allow（D-P4-007）
concurrency = "parallel"     # 可选，覆盖非写声明工具的并发（写声明恒串行，见 file_access）
# 本地文件操作声明：键是 server 报告的工具原名，或含 `*` 的通配模式（`*` 可出现在任意位置，匹配任意长度）；
# 值 none | read | write | read-write。优先级：精确名 > 命中的最具体模式 > 缺省 none（D-P4-006）。
# 缺省（没有任何键命中）= none；精确名优先，其次取命中的最具体模式（非通配字符最多者，同分取字典序最小）。
file_access = { "*" = "read", "write*" = "write", "pre*write" = "read-write" }

[[servers]]
id = "remote"
enabled = true
transport = "http"
url = "https://example.com/mcp"   # 只接受 http / https
headers = { Authorization = "Bearer xxx" }  # 可选，静态请求头（不支持 OAuth）
permission = "confirm"
file_access = { "*" = "none" }   # HTTP server 也可能读写它自己那侧的文件，按实际情况声明
```

装载与校验在 `lib-bin-cli::config`（P2-017 / P3-005 的口径，`parse_mcp` 供用例直接喂文本）。文件的缺省与错误口径：

- `mcp.toml` 缺失：等价于零 server，服务照常启动（MCP 是增强能力，不是启动依赖）；
- 文件存在但内容为空（只有空白）：启动期硬错误——写了空文件说明「本想配置但没写成」，不静默降级。

以下情况一律是启动期硬错误（退出码 2，错误文案给出 server `id` 与字段名）：

- `id` 不匹配 `^[a-z][a-z0-9_]{0,31}$`，或重复；
- `transport` 不是 `stdio` / `http`，或两种 transport 的字段混用（stdio 写了 `url`、http 写了 `command`）；
- `stdio` 的 `command` 为空、`args` 含空串、`env` 的键为空；
- `http` 的 `url` 不是合法 `http` / `https` 地址、`headers` 的键为空；
- `connect_timeout_ms = 0`、`reconnect_initial_backoff_ms = 0`（上限可以写 0 = 关闭自动重连）；
- `permission` / `concurrency` / `file_access` 写了枚举外的值；`file_access` 的键为空串。
- `file_access` 的键只支持「精确名 + `*` 通配」，`*` 可出现在任意位置、匹配任意长度（`write*`、`pre*write` 都是合法模式）；不支持正则、`?`、`[...]`（这些字符按字面处理），连续 `*` 视为一个 `*`。

### 4.3 Client 与 transport（G-2）

```text
                         enable / 启动连接
DISABLED ─────────────────────────────────► CONNECTING ──握手成功──► READY
    ▲                                            │                     │
    │ disable                                    │ 握手失败 / 超时      │ 读循环结束（崩溃 / 断开）
    │                                            ▼                     ▼
    └──────────────────────────────────────── FAILED ◄──────────────────┘
                                                 │
                                                 └── 退避到期 ──► CONNECTING
```

- **两种 transport 一个状态机**：`stdio` 用 `rmcp::transport::child_process::TokioChildProcess`（`tokio::process::Command`，`kill_on_drop(true)`，stdio 全 piped）；`http` 用 `rmcp::transport::streamable_http_client::StreamableHttpClientTransport`（reqwest 后端）。差异只出现在「怎么建 transport」，握手、发现、调用、状态机共用一套代码。
- **握手 = `serve` + initialize**：`().serve(transport)` 即完成 initialize 协商；`connect_timeout_ms` 用 `tokio::time::timeout` 包住整个 `serve`，超时即 `FAILED`（并终止已启动的子进程）。
- **读循环结束时状态必须更新**：`RunningService` 的完成由后台任务驱动，MCP 侧同时 `await` 它的完成信号，据此把状态从 `READY` 改为 `FAILED` 并记录原因。没有这一步就只能在下次调用时才发现断了（会表现为调用超时，不是可读的错误）。
- **客户端标识**：initialize 携带 `clientInfo = { name: "lingting-agent", version: <workspace 版本> }`，便于 server 侧排查。
- **HTTP 参数**：不做 OAuth，不跟随跨域重定向；`headers` 原样附加到每个请求。响应体积上限沿用 `[tools] max_result_chars` 的结果截断（传输层不额外设上限，见 §七 风险 6）。

### 4.4 工具发现与命名映射（G-3）

- **发现时机**：`READY` 之后立即 `tools/list`（自动翻页，直到 `nextCursor` 为空）；收到 `notifications/tools/list_changed` 时重新拉取一次；`reconnect` 成功后同样重新拉取。每次发现都产出一份「该 server 的工具全量快照」，用它与注册表里的旧快照做差集：新增注册、消失注销（D-P4-010）。
- **模型可见名 = `mcp__<server_id>__<tool>`**（D-P4-005）。映射规则（`sanitize`，集中在一个函数里）：
  1. server 段直接用配置的 `id`（装载期已校验）；
  2. tool 段小写化，`[^a-z0-9_]` 逐字符替换为 `_`，连续 `_` 折叠，去掉首尾 `_`；空串退化为 `tool`；
  3. 结果不是 `[a-z]` 开头时补前缀 `t_`；
  4. 总长度超过 64 时保留 `mcp__<server>__` + tool 段前若干个字符 + `_` + 8 位 `blake3` 短哈希（与原名一一对应、可复算）；
  5. 规范化后与已注册名字冲突时（同名 server 的不同工具、或与内置工具撞名），保留先注册者，后来者追加 `_<8 位哈希>` 区分；确定性来自「server 按 `id` 字典序、工具按原名升序」的固定发现顺序。
- **原始名可查**：`McpToolVO` 同时给出 `serverId` / `toolName`（模型可见名）/ `originalName`（server 里的名字）/ `description` / `parameters`（原样透传的 JSON Schema），排查与配置都能对得上。
- **schema 透传**：MCP 的 `inputSchema` 原样装进 `ToolSchema`（只补 `{"type":"object","properties":{}}` 这类缺省骨架，不重写字段），不剥 `$schema`（D-P4-003）。参数校验仍由 server 侧完成：本项目的执行器不做二次校验，参数非法由 server 返回错误，按失败回灌。
- **发现失败**：`tools/list` 报错时状态仍为 `READY`（连接是好的），但保留上一次的工具快照并在 `lastError` 里记一条可读原因；首次发现就失败则工具快照为空（该 server 暂时没有工具）。

### 4.5 适配器与注册表联动（G-3、G-5）

```rust
// agent-mcp/src/tool.rs（示意）
pub struct McpTool {
    server_id: String,           // 配置里的 id
    name: String,                // 模型可见名（mcp__...）
    description: String,
    schema: ToolSchema,          // 由 inputSchema 构造
    risk: ToolRisk,              // readOnlyHint -> ReadOnly，其余 -> Write
    file_access: ToolFileAccess, // 由 mcp.toml 的 servers[].file_access 解析（缺省 None，D-P4-006）
    concurrency: ToolConcurrency,// 写声明恒串行；否则按 server / 全局配置（D-P4-006）
    client: Arc<McpClient>,      // 只持有「调用」能力，不持有 manager
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str { &self.name }              // 契约放宽后（D-P4-003）
    fn description(&self) -> &str { &self.description }
    fn schema(&self) -> &ToolSchema { &self.schema }
    fn risk(&self) -> ToolRisk { self.risk }
    fn file_access(&self) -> ToolFileAccess { self.file_access }
    fn concurrency(&self) -> ToolConcurrency { self.concurrency }
    async fn call(&self, ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        // ctx 只用来写 trace 字段；超时与取消由执行器统一负责
    }
}
```

- **注册表运行期可变**（D-P4-004）：`ToolRegistry` 内部改为 `RwLock<BTreeMap<String, Entry>>`，`Entry { tool: Arc<dyn Tool>, default_permission: Option<ToolPermission> }`；新增 `register_with_default` / `unregister`，`lookup` 返回 `Option<Arc<dyn Tool>>`。写操作只发生在发现快照差异处（低频），读操作（每轮 `model_definitions` + 每次调用 `lookup`）只取读锁。
- **条目默认权限**：`[tools.permissions]` 显式值 > 注册时给出的条目默认 > `ToolRisk` 缺省（D-P4-007）。MCP 的 `permission` 写进条目默认，`ToolPolicy` 保持原样（它只装「显式配置」）。
- **并发语义**：`file_access` 由配置声明（缺省 `None`）。声明 `write` / `read-write` 的工具**恒为 `Exclusive`**（串行，不能被配置放松）；其余工具的并发取 server 级 `concurrency`，未配置时取 `mcp.toml` 顶层的 `concurrency`（缺省 `parallel`）。这样「声明了会写的 server」自动串行，不需要使用者同时维护两份配置（D-P4-006）。
- **不新增执行路径**：MCP 工具经由 P3 的 `ToolExecutor` 执行，校验 → 权限 → 超时 → 取消 → 归一化 → 审计链路一条不少。

### 4.6 权限、审计与事件（G-5、G-6）

- **权限**：server 级 `permission`（缺省 `allow`）作为条目默认；`[tools.permissions]` 里可以按 **模型可见名**（`mcp__filesystem__read_file`）逐工具覆盖，`deny` 的工具不进模型可见清单、执行期二次判定为 `DENIED`（P3 口径不变）。`confirm` 复用 P3 的挂起链路：挂起期间子进程保持在位，客户端确认后重新排队执行。
- **审计**：写入点仍是 `agent-runtime` 的 `record_tool_call`，`tool_calls` 表不加列——`tool_name` 已带 `mcp__<server>__` 前缀，server 可从名字反推（D-P4-015）。`file_access` 记录该工具的声明值（未被配置声明时是 `NONE`），`duration_ms` 含握手外的全部调用耗时。
- **事件**：不新增类型。`tool_call_started` / `tool_call_completed` / `tool_call_confirm_required` 的载荷字段与语义完全不变，`toolName` 就是模型可见名。
- **server 状态不进事件流**：连接/断开是管理面的信息，不是 Run 的信息；只经接口与 `metrics` 暴露（D-P4-019）。

### 4.7 生命周期、错误归一与故障隔离（G-4、G-7）

| 场景 | 处置 | 模型看到的文本 |
|---|---|---|
| `tools/list` 失败（连接正常） | 保留旧快照，`lastError` 记原因，状态仍 `READY` | —— |
| 子进程启动失败（命令不存在 / 权限） | 状态 `FAILED`，不重试到启动成功为止（按退避重连） | `MCP 服务器不可用：<server_id>` |
| 握手超时 | 终止刚启动的子进程，状态 `FAILED` | 同上 |
| 运行期断开 / 子进程崩溃 | 状态 `FAILED`，取消该 server 上的在途调用（返回错误），按退避重连 | `MCP 服务器通信失败：<server_id>` |
| 调用超时 | 执行器统一计时（`[tools] timeout_ms`），`TIMEOUT` | `工具执行超时（超过 <n> ms）` |
| server 返回 `isError = true` | `ToolCallStatus::ERROR`，文本取 server 给的内容（截断 + 脱敏） | server 原文 |
| 协议错误 / 非法响应 | `ToolCallStatus::ERROR` | `MCP 服务器返回了无法解析的响应：<server_id>` |
| 工具已被移除（配置未变但 server 不认） | 状态 `FAILED`（调用期错误），Run 继续 | `MCP 服务器上不存在该工具：<toolName>` |
| 服务关闭 / Run 取消 | 取消 token 命中 → drop 调用 future，`CANCELLED` | —— |

- **文案单一产点**：固定文案集中在 `agent-mcp/src/error.rs`，`Display` 只产出模型可读文本，不含 URL、请求头、环境变量、堆栈、进程命令行（判据 E-2 / E-3）。
- **故障隔离**：单个 server 卡死不影响其他 server，也不拖垮 Run——每次调用都在执行器的超时与取消 `select!` 之内（P3 已有），MCP 侧不额外等待。`McpManager` 的写操作（启停 / 重连 / 刷新）按 server `id` 分段加锁，不持全局锁跨 `await`（P2.1 的锁纪律继续适用）。
- **退避重连**：`FAILED` 后按 `reconnect_initial_backoff_ms` 起、2 倍增长、到 `reconnect_max_backoff_ms` 封顶，无限次；`reconnect_max_backoff_ms = 0` 关闭自动重连（只能手动 `reconnect`）。重连期间 `enable` / `disable` 可以打断（取消在途重连）。

### 4.8 管理接口（G-6）

| 方法 | path | 入参 | 返回 | 说明 |
|---|---|---|---|---|
| GET | `api/mcp/server/list` | 无 | `McpServerVO[]` | 配置里的全部 server（含 `disabled`），按 `id` 升序 |
| GET | `api/mcp/tool/list` | `McpToolListParams`（`serverId?`） | `McpToolVO[]` | 当前已发现的工具快照，按模型可见名升序 |
| POST | `api/mcp/server/update` | `McpServerUpdatePO`（`serverId` + `action`） | `McpServerVO` | `action ∈ ENABLE / DISABLE / RECONNECT / PROBE`，返回**动作完成后**的状态 |

- 动作语义：`ENABLE` 连接并发现（幂等：已 `READY` 时直接返回当前状态）；`DISABLE` 断开并注销该 server 的全部工具（幂等）；`RECONNECT` 先关闭旧连接再按 `ENABLE` 走一遍；`PROBE` 只对当前状态做一次只读探测（`READY` 时真正发一次 `tools/list` 校验连通性，其余状态直接返回状态本身，不建立连接）。
- 状态取值：`DISABLED` / `CONNECTING` / `READY` / `FAILED` / `STOPPED`（`STOPPED` 只在服务关闭后出现，正常请求不会看到）。
- 载荷字段：`McpServerVO { id, transport, enabled, status, toolCount, lastError?, lastConnectedAtMs?, nextRetryAtMs? }`；`McpToolVO { serverId, toolName, originalName, description, parameters }`（`parameters` 是原样 JSON Schema，前端不解析，只展示）。
- 错误：未知 `serverId` → 400 + 固定文案 `MCP 服务器不存在`；对已 `DISABLED` 的 server 发 `PROBE` → 200 + `DISABLED` 状态（不是错误）；内部错误 → 500 固定文案。
- 路径动作词表沿用 `docs/api/readme.md`：`list` / `update`，不新增动作段（D-P4-016）。
- 接口总数 26 → **29**（不含 debug + `metrics` 限定的 `api/probe/metrics`）。

### 4.9 装配、关闭与孤儿进程（G-4）

```text
assemble_with()
  ├── 建内置工具注册表（P3 原样）
  ├── McpManager::new(config, registry.clone())
  ├── manager.connect_enabled_all().await      // 并发连接，逐个 connect_timeout，失败不影响装配
  └── AgentHolder 持有 manager（关闭时用）

shutdown()
  ├── handler::shutdown(runtime)               // 先让 Run 落终态（P2 原样）
  ├── manager.shutdown_all().await             // 置 STOPPED → 取消在途调用 → close() 带超时 → 超时 kill
  └── 子进程回收核对（日志一行：终止 N 个子进程）
```

- **关闭顺序不能反**：先停 Run 再关 MCP。反过来的话，正在执行的工具调用会以「服务器通信失败」收尾，Run 走失败而不是取消收尾，语义就变了。
- **子进程终止**：`TokioChildProcess` 的优雅关闭（关 stdin、等退出、超时 `SIGTERM`、再超时 kill）优先；`kill_on_drop(true)` 作为最后兜底（进程 panic 或 runtime 提前退出时）。Windows 没有 job object，孙进程不保证被回收——写进 §七 风险 5。
- **子进程环境**：`env` 只追加不覆盖（`PATH` 等继续继承），stdout/stderr 全部 piped 并接到 tracing（`debug` 级别，不落事件文件）。

### 4.10 可观测性（G-6）

- `metrics` 快照新增 `mcp` 段（只在 `debug` + `metrics` 特性构建注册，与既有口径一致）：

| 字段 | 含义 |
|---|---|
| `servers` / `ready` / `failed` / `disabled` | 各状态的 server 数（取快照时刻） |
| `tools` | 已注册的 MCP 工具数 |
| `connects` / `connect_failures` | 连接尝试与失败次数（自进程启动累计） |
| `reconnects` | 自动重连次数 |
| `calls` / `call_errors` / `call_timeouts` | MCP 工具调用总数、失败数、超时数 |
| `shutdown_kills` | 关闭时被强制 kill 的子进程数 |

- 不新增事件、不打阈值日志；状态查询走 §4.8 的接口（与 P2.2「测试数据不走日志」的纪律一致）。

---

## 五、批次与任务

批次之间按序；同批次内可并行。每批结束时必须保持可编译、可运行。

| 批次 | 主题 | 任务 | 出口 |
|---|---|---|---|
| A | Crate、契约前提与配置 | P4-001 ~ P4-004 | `agent-mcp` 可编译；`Tool` 契约能承载动态工具；`mcp.toml` 配置可装载校验 |
| B | 连接、传输与适配器 | P4-005 ~ P4-009 | 进程内能连一个 stdio / HTTP server、发现工具并调用成功 |
| C | Manager、装配与生命周期 | P4-010 ~ P4-013 | 服务启动即接入 MCP 工具；关闭后无子进程；状态可见 |
| D | 接口与契约 | P4-014 ~ P4-016 | 3 条接口可用，SDK 重新生成，文档同步 |
| E | 用例与阶段验收 | P4-017 ~ P4-020 | `mcp.*` 用例全绿；阶段报告出稿 |

```text
A ──► B ──► C ──► D ──► E
      │           │
      └──► E-017 ─┘（测试替身只依赖 B 的适配器契约，可与 C / D 并行）
```

- 强序：A → B（适配器依赖契约放宽与注册表可变）→ C（装配依赖 client 与适配器）→ D（接口依赖 manager 的状态快照）→ E（用例覆盖前面全部）。
- 可并行：E-017（测试替身）只依赖 B 的适配器契约，可与 C / D 同时做；E-018 必须等 D 完成。
- 任务清单见 [tasks.md](tasks.md)。

---

## 六、阶段验收方向

1. 配置一个 stdio MCP Server 后，其工具自动出现在可用工具列表中，且能被 Agent 调用成功。
2. 配置一个 HTTP MCP Server 后行为一致。
3. MCP Server 进程崩溃或断开时，系统给出明确状态，Agent 调用得到可读错误而不是挂死。
4. 取消 Run 或关闭 Server 时，MCP 子进程被终止，无孤儿进程残留。
5. MCP 工具调用同样进入审计记录与权限校验。
6. Agent Core 代码中不出现 MCP 协议类型（以依赖方向检查为证据）。

以上六条已展开成可逐条取证的判据，见 [acceptance.md](acceptance.md)（分组 B / A / S / T / L / C / I / E / R）。

---

## 七、已知限制与风险

1. **rmcp 的 API 稳定性（已实测闭环）**：3.11 的 `serve` / `RunningService` / `ClientHandler`（含 `on_tool_list_changed`）已由 P4-005 ~ P4-010 落地并回填 D-P4-002 / D-P4-010；后续升级仍以代码为准并回填决策。
2. **依赖体积与重复 reqwest**：`rmcp` 的 HTTP transport 可能带入与 workspace 不同版本的 `reqwest`（workspace 是 0.13）。P4-001 要核对 `cargo tree -i reqwest`，若出现两份则记录为已知构建代价（不强行统一版本，避免动 P2 的 Provider 链路）。
3. **工具名规范化会失真**：`mcp__<server>__<tool>` 经过小写化、非法字符替换与超长截断后，模型看到的名字可能不等于 server 原名。原名只在 `McpToolVO.originalName` 里保留；这会让「按文档手动调工具」的人需要查一次接口。
4. **文件操作声明是自述，不是校验**：`file_access` 由配置声明（缺省 `None`，精确名 + `*` 通配，前者优先），系统只用它推导并发语义与写审计，不会验证 server 实际动了什么。把会写文件的 server 声明成 `none`（或漏配匹配模式）会让它的工具按并行执行；命中 `write` / `read-write` 才会自动串行。通配只是字符串匹配，不理解语义——`*` 写宽了就会把只读工具也标成写（只会更保守），写窄了则漏标。这不是沙箱，真正的隔离留给 P6。
5. **孙进程不保证回收**：Windows 没有 job object，`kill_on_drop` 只能杀掉直接子进程。若 MCP server 自己再 spawn 子进程（`npx` 就是典型），那些进程可能残留。判据 L-7 只断言「直接子进程已退出」。
6. **HTTP transport 的体积与超时不设独立上限**：响应由 rmcp 读进内存后转成文本，再按 `[tools] max_result_chars` 截断。一个超大响应会在截断前占内存；本阶段不引入独立字节上限（rmcp 自带 `StreamableHttpResponseLimits` 可后续启用）。
7. **取消不通知 server**：Run 取消只是 drop 调用 future 并断开响应等待，不发送 `notifications/cancelled`。server 可能继续把调用执行完（副作用不可撤销）。挂起确认期间也一样：子进程保持在位，确认前不执行。
8. **工具数量会推高 token 成本**：每个 server 的全部工具都进模型可见清单，没有白/黑名单。工具多的 server 会让每轮请求的 `tools` 字段显著变大；过滤能力留到后续阶段。
9. **管理接口无鉴权**：沿用 P1 现状（本地客户端、未启用鉴权）。任何能把请求发到端口的人都能 `enable` 一个配置好的 stdio server（也就等于让服务端启动一个已配置的进程）。
10. **只接 `tools` 能力**：`resources` / `prompts` / `sampling` / `elicitation` / `progress` 不接入；server 若依赖这些能力交互，行为会退化为「只调工具」。
11. **配置改动需要重启**：`mcp.toml` 只读，新增 / 删除 server 必须重启服务；`enable` / `disable` 只影响进程内状态，重启后回到配置值（D-P4-008）。
12. **启动时间随 server 数量增长**：虽然并发连接且各有超时，最坏情况仍是「最慢 server 的握手超时」；配置里写很多不可达 server 会延长启动。
13. **测试执行受授权约束**：所有 `make test` / 单跑用例都需要明确授权。未授权时只推进代码与静态检查，判据保持未勾选。

---

## 八、决策状态

**全部定案，可以开工。** 23 项决策（D-P4-001 ~ D-P4-023）的候选方案、理由与承接任务见 [decisions.md](decisions.md)。

| 编号 | 事项 | 状态 |
|---|---|---|
| D-P4-001 | 新增 `agent-mcp`，MCP 协议类型只在该 Crate | 已定案 |
| D-P4-002 | `rmcp` 3.11.0 + 特性集（client / child-process / streamable-http-client-reqwest；测试替身另加 server 侧特性） | 已定案（实测回填） |
| D-P4-003 | `Tool` 契约放宽为 `&str` + `ToolSchema::from_value`（MCP 动态名字与外部 schema） | 已定案 |
| D-P4-004 | `ToolRegistry` 运行期可变（`RwLock` + 条目默认权限） | 已定案 |
| D-P4-005 | 模型可见工具名 `mcp__<server>__<tool>` + 规范化与冲突规则 | 已定案（需求方确认前缀） |
| D-P4-006 | `file_access` 由配置声明（缺省 `none`；精确名 + `*` 通配，含 `write*` / `pre*write`）；并发按声明推导；`ToolRisk` 由 `readOnlyHint` 推导 | 已定案 |
| D-P4-007 | MCP 工具默认权限：按 server 配置，缺省 `allow`；`[tools.permissions]` 可按可见名覆盖 | 已定案（需求方确认） |
| D-P4-008 | 配置持久化：独立 `mcp.toml` 唯一来源，启停只在内存，不落库、不加迁移 | 已定案（需求方确认） |
| D-P4-009 | 连接时机：启动期并发连接 + 每 server 超时，失败不阻塞启动 | 已定案 |
| D-P4-010 | 工具快照差量刷新（发现 / `list_changed` / 重连） | 已定案 |
| D-P4-011 | 退避重连策略与关闭开关 | 已定案 |
| D-P4-012 | `tools/list` 自动翻页与首屏上限 | 已定案 |
| D-P4-013 | stdio 安全边界：显式 `enabled`，不做可执行路径白名单 | 已定案（需求方确认） |
| D-P4-014 | HTTP transport 只支持静态请求头，不做 OAuth | 已定案 |
| D-P4-015 | 审计与事件：不新增事件、`tool_calls` 不加列 | 已定案 |
| D-P4-016 | 管理接口形态：3 条，动作段沿用 `list` / `update` | 已定案 |
| D-P4-017 | 结果映射：文本拼接、`is_error`、非文本内容降级 | 已定案 |
| D-P4-018 | 超时与取消：复用执行器，不发 `notifications/cancelled` | 已定案 |
| D-P4-019 | 状态可见性：接口 + `metrics` 的 `mcp` 段，不进事件流 | 已定案 |
| D-P4-020 | 关闭顺序与子进程终止 | 已定案 |
| D-P4-021 | 测试替身：`bin-tests` 隐藏子命令扮演 MCP server | 已定案 |
| D-P4-022 | 错误文案单一产点与脱敏边界 | 已定案 |
| D-P4-023 | 本阶段不改 `agent-runtime`（零改动为验收项） | 已定案 |
