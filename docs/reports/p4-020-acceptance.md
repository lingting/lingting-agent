# P4-020 阶段验收与报告（MCP Integration）

- **阶段**：P4 MCP Integration（5 个批次 / 20 个任务）
- **任务**：P4-020（证据覆盖 Batch A ~ E 的已落地实现，含 P4-017 的测试替身、P4-018 的 19 条 `mcp.*` 用例、P4-019 的文档同步）
- **前置**：Batch A ~ D 已落地（P4-001 ~ P4-016）；P4-017 / P4-018 / P4-019 在本轮结案
- **执行日期**：2026-11-26
- **环境**：Windows 11 / Git Bash；CPU 18 核；用例并发度 18（默认 `available_parallelism()`）；`bin-agent` 以 `metrics` 特性构建
- **判据来源**：[`.steps/p4/acceptance.md`](../../.steps/p4/acceptance.md)；任务清单见 [`.steps/p4/tasks.md`](../../.steps/p4/tasks.md)，决策见 [`.steps/p4/decisions.md`](../../.steps/p4/decisions.md)
- **测试授权**：本轮在明确授权下反复执行用例（`make test` 共 9 次，含定向、全量与复跑；另有 2 次 `cargo run -p bin-tests -- <用例>` 的单用例定向复现）。`mcp.call_error` 在复跑中暴露了一处用例自身的断言顺序问题，修复后全绿（见 §四第 5 条）——最终代码：`make test ARGS="mcp"`（19/19，`run-1795682083908-42744`）、`make test ARGS="--all"`（123/123，`run-1795682098490-2964`）、`make test ARGS="tool client"`（30/30，`run-1795682138292-42856`）

---

## 一、结论

1. **九组判据全部成立**（B / A / S / T / L / C / I / E / R），逐条证据见 §二 ~ §三。**没有「未授权」项，也没有「跑一遍看看」式证据**：每条都指向用例名、命令、源码位置或结构化快照。
2. **协议不出 crate**：`rmcp` 只出现在 `agent-mcp`（正式构建）与 `bin-tests`（`server` 特性的测试替身）；`agent-tool` 的 `mcp` 命中全部是注释，`manager-agent` / `lib-web` 零命中；`agent-runtime` / `agent-core` / `agent-tool` / `types-core` / `manager-agent` / `lib-web` 在本轮**零改动**（`git status` 为空）。
3. **同构而不旁路**：MCP 工具就是 `Arc<dyn Tool>`，经 P3 的注册表与执行器执行；`mcp.call` 证明事件、轨迹、审计三处与内置工具完全一致，三个工具事件（`tool_call_started` / `tool_call_completed` / `tool_call_confirm_required`）没有新增类型。
4. **失败即回灌、关得干净**：崩溃、协议错误、超时、取消、`deny`、`confirm` 六条失败路径都变成模型可读文本或终态，Run 不挂死；关闭后替身 PID 不存在，状态全部 `STOPPED`。
5. **用例执行中发现并修复了一个真实缺陷**（§四第 1 条）：`McpClient::fail` 直接写状态、没有唤醒监督任务，导致「自动重连」永不触发（`reconnects` 恒为 0）。修复后 `mcp.reconnect` 证明默认退避下 `FAILED → READY` 自动恢复。
6. **两处口径差异已登记**：`tool/list` 包含 `deny` 的工具（注册在册但模型不可见）与 `docs/api/mcp.md` 的措辞不一致（`KI-P4-001`）；Windows 无 job object，孙进程不保证回收（`KI-P4-002`，沿用阶段 README 风险 5）。见 §六。

---

## 二、Build 与结构（B / A 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| B-1 | workspace 全量可编译 | 成立 | `cargo build --workspace` 退出码 0（45 crates，1m04s） |
| B-2 | 格式一致 | 成立 | `cargo fmt --all --check` 退出码 0 |
| B-3 | Clippy 零告警 | 成立 | `cargo clippy --workspace --all-targets -- -D warnings`：`No issues found` |
| B-4 | SDK 与前端门禁 | 成立 | `make gen`（路由 29 / 接口 29，两份 SDK 内容无变化）、`make gen-check`（只读产物可从零重建）、`make ui-check`（`pnpm exec tsc --noEmit`）退出码均为 0 |
| A-1 | `agent-mcp` 依赖方向正确 | 成立 | `cargo tree -p agent-mcp --depth 1` = `agent-tool` / `lib-core` / `rmcp` / `reqwest` / `blake3` / Tokio / `tokio-util` / `async-trait` / `futures-util` / `serde*` / `specta` / `strum*` / `tracing` / `framework-proc-*`；全树 `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `lib-db` / `lib-web*` / `types-core` / `bin-*` 命中 **0** |
| A-2 | 协议类型只在 `agent-mcp`（+ 测试替身） | 成立 | `cargo tree -i rmcp` 只命中 `agent-mcp` 与 `bin-tests`（后者仅为 `mock-mcp` 追加 `server` 特性，见 D-P4-002 实测回填）；`grep -rn "rmcp" crates --include=*.rs` 除 `crates/agent-mcp/` 与 `crates/bin-tests/` 外无命中 |
| A-3 | 工具层不认识 MCP | 成立（注释命中已说明） | `cargo tree -p agent-tool` 的 `rmcp` 命中 **0**；`grep -rniE "mcp\|rmcp" crates/agent-tool/src` 的 9 处**全部是文档注释**（P4-002 要求写下的「MCP 工具与内置工具同构」边界说明），没有 import、类型或分支 |
| A-4 | 装配层不泄漏协议类型 | 成立 | `grep -rn "rmcp" crates/manager-agent/src crates/lib-web/src` 命中 **0**；两处只用 `agent_mcp` 的业务类型（`McpServerSpec` / `McpManager` / `McpServerVO` 等） |
| A-5 | 运行时零改动 | 成立 | `git status --short crates/agent-runtime crates/agent-core crates/agent-tool crates/types-core crates/manager-agent crates/lib-web` 为空；P4-002 / P4-003 的签名适配已在 Batch A 落地，本轮未再触碰 |
| A-6 | 未创建被禁止的 Crate | 成立 | `crates/` 下不存在 `manager-core` / `types-api` / `bin-tui` / `bin-web` / `bin-mock-server` / `agent-mcp-server`（`lib-bin-tui` / `lib-bin-web` 是既有装配层，不在禁止项内） |
| A-7 | 结构文档同步 | 成立 | `docs/workspace.md` §3.2 `agent-mcp` 段（职责 / 边界 / 依赖，无「阶段：P4」口吻）与 `Cargo.toml` members、`cargo tree` 事实一致；§8.1 成员清单与矩阵一致；本轮把 `bin-tests` 的依赖行补上测试替身所需的 `rmcp`（`server` 特性）/ `axum` / `tokio-util` / `sysinfo` |

---

## 三、契约、传输、生命周期、调用、接口、错误、回归

### 3.1 契约与 Schema（S 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| S-1 | `Tool` 契约支持动态数据 | 成立 | `crates/agent-tool/src/tool.rs` 的三个方法是 `fn name(&self) -> &str` / `fn description(&self) -> &str` / `fn schema(&self) -> &ToolSchema`；全仓无 `&'static str` 的实现（`grep` 只命中五个内置工具的 `-> &'static ToolSchema`，是 `&'static` 到 `&'a` 的子类型转换）；`grep -rn "Box::leak" crates/agent-mcp/src` 命中 **0** |
| S-2 | 外部 schema 原样透传 | 成立 | `mcp.schema`：`tool/list` 的 `parameters` 与剧本 `inputSchema` 逐字段相等（含 `$schema` / `title` / `x-custom` / 嵌套 `anyOf` / `additionalProperties`），请求体的 `function.parameters` 与接口同源 |
| S-3 | 可见名可复算且稳定 | 成立 | `mcp.name`：`Read-File!`→`mcp__naming__read_file`、`read_file`→`mcp__naming__read_file_8ff691b7`（撞名加 8 位哈希）、`UPPER_CASE`→`upper_case`、`***`→`tool`、`9lives`→`t_9lives`、超长名→`mcp__naming__long_tool_name_that_exceeds_the_sixty_four_814104a3`（64 字符封顶）；全部满足 `^[a-z][a-z0-9_]{1,63}$`，重连后重新发现的可见名与第一次完全一致 |
| S-4 | 命名前缀与原名可查 | 成立 | `mcp.discover`：6 个工具的可见名都以 `mcp__mock__` 开头且等于 `mcp__<server>__<清洗后原名>`，`originalName` 与剧本原名集合一致，清单按可见名升序 |
| S-5 | 权限可见性一致 | 成立 | `mcp.denied`：`permission = "deny"` 时请求体的 `tools` 字段不含 `mcp__mock__read_file`；模型凭空调用得到「工具未授权：mcp__mock__read_file」，审计 `DENIED`，替身证据里 `calls` 为空（从未到达 server） |
| S-6 | 契约兼容 | 成立 | `git status` 显示 `agent-core`（`RunEvent` / `ToolDefinition` 所在）与 `agent-tool/src/audit.rs`（`ToolCallVO`）零改动；`make gen` 产物只新增 `api/mcp/{server/list,tool/list,server/update}` 与三个 MCP DTO（`McpServerVO` / `McpToolVO` / `McpServerUpdatePO` + 两个枚举） |
| S-7 | 文件访问声明生效 | 成立 | `mcp.file_access`：`{"*":"read","write*":"write","pre*write":"read-write","write_file":"none"}` 下 `read_file`→`READ`、`write_log`→`WRITE`、`pre_commit_write`→`READ_WRITE`、`write_file`→`NONE`、`delete_all`（只命中 `*`）→`READ`；去掉精确名后 `write_file`→`WRITE`；只写 `read*` 时 `write_file`→`NONE`；同一配置两次启动的映射一致；写声明工具在同一轮里串行（替身证据的调用区间：`write_log 1795682100241~1795682100362`、`pre_commit_write …364~…486`、`write_file …488~…610`，两两不重叠） |

### 3.2 传输与握手（T 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| T-1 | stdio server 能被启动并握手 | 成立 | `mcp.discover`：`GET api/mcp/server/list` 的 `status = READY`，`toolCount = 6`；替身进程由被测服务端以 `mock-mcp --transport stdio` 启动 |
| T-2 | HTTP server 行为一致 | 成立 | `mcp.http`：同一份 `normal` 剧本经 `http://127.0.0.1:<port>/mcp` 连接后 `READY`、6 个工具，一次 `echo` 调用闭环（审计 `OK`），替身证据 `transport = "http"` 且记录了该调用 |
| T-3 | 握手超时受配置约束 | 成立 | `mcp.config`：`connect_timeout_ms = 300` + 只接不回的对端 → `status = FAILED`，`lastError = MCP 服务器连接超时：dead（> 300 毫秒）`，服务照常启动并可读接口 |
| T-4 | 客户端标识 | 成立 | `mcp.discover`：替身证据的 `clientInfo = {"name":"lingting-agent","version":"26.11.0"}`，与 `agent_mcp::CLIENT_NAME`、workspace 版本一致 |

### 3.3 生命周期（L 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| L-1 | 启停幂等 | 成立 | `mcp.api`：连续两次 `ENABLE` 的返回状态与 `toolCount` 一致（`READY` / 6），`tool/list` 总量 12 不变；连续两次 `DISABLE` 均为 `DISABLED` / 0，总量回到 6 |
| L-2 | 启动不阻塞 | 成立 | `mcp.config`：一次启动里 `ok`（stdio）`READY`、`off`（`enabled = false`）`DISABLED`、`dead`（不可达 http）`FAILED`，入口接口全部可用（`server/list` 按 id 升序返回三条） |
| L-3 | 崩溃即状态可见 | 成立 | `mcp.crash`：替身退出后 `status = FAILED`，`lastError = MCP 服务器通信失败：mock`（不含地址与命令行） |
| L-4 | 崩溃后调用不挂死 | 成立 | `mcp.crash`：崩溃后的下一次调用在 20 秒上限内返回「工具执行失败：MCP 服务器通信失败：mock」，审计 `ERROR`，Run `COMPLETED`，替身证据只有 `crash` 一条调用 |
| L-5 | 退避重连 | 成立 | `mcp.reconnect`：`reconnect_max_backoff_ms = 0` 时崩溃后等 1.2s 仍 `FAILED`、`nextRetryAtMs` 为空、`metrics.mcp.reconnects` 不增长；`RECONNECT` 回到 `READY`（2 个工具）并能再调用；默认退避（200ms 起）下崩溃后自动恢复 `READY`，`reconnects 0 → 1` |
| L-6 | 工具清单变更 | 成立 | `mcp.discover`：替身证据的 `toolLists` 第一轮为 `["echo"]`，收到 `notifications/tools/list_changed` 后第二轮含 `late_tool`；`tool/list` 出现 `mcp__mock__late_tool`，下一轮请求体的 `tools` 同步含它与 `echo` |
| L-7 | 关闭无孤儿进程 | 成立 | `mcp.shutdown`：`shutdown_all()` 后状态全部 `STOPPED`、`tool/list` 为空、注册表里查不到该工具、替身 PID 45048 已退出（`kills = 0`，`shutdown_kills = 0` —— 优雅关闭，未触发强杀）；`mcp.config` 另证 `enabled = false` 的 server 从未产生子进程（PID 文件不存在） |
| L-8 | 关闭顺序正确 | 成立 | 源码：`crates/manager-agent/src/holder.rs::shutdown` 先 `handler::shutdown(runtime)` 再 `installed.mcp().shutdown_all()`；行为：`run.shutdown` 在全量运行中仍证明「关闭期间在跑的 Run 以 `server_shutdown`（`cancelled`）收尾，不是 `FAILED`」 |

### 3.4 调用语义（C 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| C-1 | 调用成功闭环 | 成立 | `mcp.call`：模型要求调用 `mcp__mock__echo` → 执行成功 → 第二轮请求的 `messages` 含 `role = tool` 且内容为 `ECHO：{"text":"闭环"}` → 终答「Mock 回复：ECHO：{…}」 |
| C-2 | 结果映射 | 成立 | `mcp.result`：纯文本 `纯文本结果`；文本 + 图片 `前半段文本` + `[图片内容已省略]`（base64 0 命中）；仅结构化 `{"note":"结构化内容","rows":2}`；空内容 `（MCP 工具没有返回内容）`——四者都非空 |
| C-3 | 失败回灌 | 成立 | `mcp.call_error`：`is_error = true` → `磁盘只读，写入被拒绝`；协议级错误 → `工具执行失败：MCP 服务器返回了无法解析的响应：mock`（原文 `mock 协议级错误` 0 命中）；两条审计都是 `ERROR`，Run `COMPLETED` |
| C-4 | 超时 | 成立 | `mcp.timeout`：`timeout_ms = 500` 时审计 `TIMEOUT`、结束事件 `error = 工具执行超时（> 500 毫秒）`、`durationMs = 501`，Run `COMPLETED` 且下一次调用正常 |
| C-5 | 取消 | 成立 | `mcp.cancel`：收到 `tool_call_started` 后取消 → Run `CANCELLED`（11.8ms 内生效）、补一条 `CANCELLED` 的结束事件、审计为空、`run/get` = `CANCELLED`；替身证据显示 `slow` 调用已开始（取消发生在调用之后） |
| C-6 | 权限拒绝 | 成立 | `mcp.denied`：审计 `DENIED` + 固定文案，模型收到同文本，Run `COMPLETED`，替身 0 次调用 |
| C-7 | 确认交互 | 成立 | `mcp.confirm`：`confirm` → `tool_call_confirm_required` → `run/get = WAITING`、`metrics.limit.running_now = 0`、审计 `PENDING` → 批准后替身收到一次 `read_file`、审计转 `OK`；驳回后回灌「用户拒绝调用该工具」、审计 `DENIED`、替身 0 次 `write_log` 调用 |
| C-8 | 审计 | 成立 | `mcp.audit`：记录 `toolName = mcp__mock__write_log`（可见名）、`fileAccess = WRITE`（配置 `write*`）、`status = Ok`、`durationMs = 124`（覆盖替身 120ms 延迟），与 `tool_call_completed` 的同名 / 同状态一致 |
| C-9 | 事件契约不变 | 成立 | `mcp.call`：`seq` 从 0 连续递增；开始 / 结束按 `toolCallId` 配对；遍历本次 Run 的全部事件，wire 值都不含 `mcp`（没有 MCP 专属事件类型） |

### 3.5 接口（I 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| I-1 | 接口清单 | 成立 | `crates/lib-web/src/mcp/api.rs` 只有 3 个 `#[web_api_*]`（`mcp_server_list` / `mcp_tool_list` / `mcp_server_update`）；`make gen` 报「路由 29 条，接口 29 条」（26 + 3） |
| I-2 | `server/list` 语义 | 成立 | `mcp.api`：`["alpha","beta"]` 按 id 升序；`alpha`（`enabled = false`）为 `DISABLED`、`toolCount = 0`、`lastError` / `lastConnectedAtMs` / `nextRetryAtMs` 均为空；`beta` 为 `READY`、`toolCount = 6`、带 `lastConnectedAtMs` |
| I-3 | `tool/list` 语义 | 成立 | `mcp.api`：按可见名升序、`serverId = beta` 过滤生效、`serverId = alpha`（无工具）与未知 id 都返回 `[]` |
| I-4 | `server/update` 语义 | 成立 | `mcp.api`：`ENABLE` / `DISABLE` / `PROBE` / `RECONNECT` 都返回动作完成后的状态；`PROBE` 对 `DISABLED` 返回 200 + 状态本身；未知 `serverId` 返回 400 + `MCP 服务器不存在` |
| I-5 | 文档与代码一致 | 成立 | `docs/api/mcp.md` 的 path / 字段 / 枚举取值与 `agent-mcp/src/vo.rs`、`lib-web/src/mcp/api.rs` 逐项一致；`docs/api/readme.md` 的领域表与「当前接口总数 29 条」一致（mcp 3 条） |
| I-6 | SDK 覆盖新接口 | 成立 | `packages/sdk-rust/src/api.rs` 含 `mcp_server_list` / `mcp_server_update` / `mcp_tool_list` 三条与对应 DTO；`make gen-check` 通过（产物可从零重建且可编译） |

### 3.6 错误与安全（E 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| E-1 | 文案单一产点 | 成立 | 五条传输 / 协议文案只在 `crates/agent-mcp/src/error.rs` 定义（`SERVER_UNAVAILABLE` / `CONNECT_TIMEOUT` / `COMMUNICATION` / `INVALID_RESPONSE` / `TOOL_NOT_FOUND`）；管理动作的 `MCP 服务器不存在` 也只有一个产点（`crates/agent-mcp/src/manager.rs` 的 `Display`），全仓无第二处拼接 |
| E-2 | 不泄漏敏感信息 | 成立 | `mcp.http`：配置带 `Authorization: Bearer mcp-http-secret-token`，在事件、审计、`server/list`、`tool/list` 的 JSON 里检索 `Bearer` / token 值 / 替身 URL / `mock-mcp` / `127.0.0.1` 全部 0 命中 |
| E-3 | 不泄漏内部细节 | 成立 | `crates/agent-mcp/src/error.rs` 的 `Display` 只有固定中文 + `server_id` / 工具名；rmcp 原文只经 `tracing::debug`；`mcp.call_error` 证 `mock 协议级错误` 0 命中 |
| E-4 | stdio 默认不启动 | 成立 | `mcp.config`：`enabled = false` 的 `off` 状态 `DISABLED`、`toolCount = 0`，其 PID 文件**不存在** |
| E-5 | 配置即边界 | 成立 | `mcp.config`：`id` 规则 / 唯一性、transport 字段互斥、URL scheme、`permission` / `concurrency` / `file_access` 枚举等 20 条非法形态逐条命中固定文案；非法 `mcp.toml` 时直接启动 `bin-agent server` 的**退出码为 2** |

### 3.7 回归与收尾（R 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| R-1 | P3 用例全绿 | 成立 | `make test ARGS="tool client"`：30/30 通过（`run-1795682138292-42856`） |
| R-2 | 全量用例全绿 | 成立 | `make test ARGS="--all"`：123/123 通过（`run-1795682098490-2964`），含 P1 ~ P3 的全部用例与 19 条 `mcp.*` |
| R-3 | 既有语义未变 | 成立 | 全量运行里 `run.*` / `persist.*` / `runtime.*` 全部通过；`git status` 显示 `agent-core` 的 `RunEventKind` 零改动（事件类型集合与 P3 相同） |
| R-4 | 阶段收尾 | 成立 | 本报告出稿；`.steps/README.md`、`.steps/p4/README.md`、`.steps/p4/tasks.md`、`.steps/p4/acceptance.md` 的状态行一致；未取证项与遗留风险见 §五 / §六 |

### 3.8 `mcp.*` 用例清单与结果（19 条，全绿）

| 用例 | 耗时 | 覆盖判据 |
|---|---|---|
| `mcp.config` | 4995ms | T-3 / L-2 / E-4 / E-5 + 装载规则 |
| `mcp.name` | 2090ms | S-3 |
| `mcp.discover` | 3771ms | T-1 / T-4 / S-4 / L-6 |
| `mcp.schema` | 2665ms | S-2 |
| `mcp.call` | 3092ms | C-1 / C-9 |
| `mcp.result` | 2763ms | C-2 |
| `mcp.call_error` | 3086ms | C-3 |
| `mcp.timeout` | 3309ms | C-4 |
| `mcp.cancel` | 2880ms | C-5 |
| `mcp.denied` | 3101ms | S-5 / C-6 |
| `mcp.confirm` | 3614ms | C-7 |
| `mcp.audit` | 3247ms | C-8 |
| `mcp.file_access` | 5897ms | S-7 |
| `mcp.http` | 3263ms | T-2 / E-2 |
| `mcp.crash` | 3114ms | L-3 / L-4 |
| `mcp.reconnect` | 4357ms | L-5 |
| `mcp.shutdown` | 2158ms | L-7 / L-8 |
| `mcp.api` | 2855ms | L-1 / I-2 / I-3 / I-4 |
| `mcp.metrics` | 2725ms | P4-013（`metrics.mcp`：`servers=1 ready=1 tools=6 connects=1 calls=1 call_errors=0 timeouts=0 kills=0`） |

失败信息口径：所有断言文本都带 `server` id 或可见工具名，失败时不需要翻日志（例如「替身应只收到过 crash 调用」「重发现的清单应含 late_tool」「`deny` 的工具不应出现在请求体」）。

---

## 四、偏差与修订

1. **修复了 `McpClient::fail` 不唤醒监督任务的缺陷（本轮唯一的业务代码改动）**。原实现直接写状态机（`*self.lock(&self.state) = Failed`），没有走 `set_state`，因此「读循环结束 → FAILED」不会唤醒 manager 的监督任务：`reconnect_max_backoff_ms > 0` 时自动重连**永不触发**（`metrics.mcp.reconnects` 恒为 0），只能手动 `RECONNECT`。`mcp.reconnect` 的进程内段先复现、再修复、再断言 `reconnects 0 → 1`。改动 3 行（`crates/agent-mcp/src/client.rs`），语义只影响「失败也是一次状态迁移」这一条既有约定。
2. **`tool/list` 与 `toolCount` 包含 `deny` 的工具**：`deny` 的条目仍注册在表里（P3 的既有语义要求「执行期能查到并回 `DENIED`」），因此管理面能看到它们；但 `docs/api/mcp.md` 写「`deny` 的工具在注册环节就不进入」。本轮不改实现（改实现会破坏 `DENIED` 的取证路径），改为修正判据口径并登记 `KI-P4-001`：**S-5 的判据是「不进请求体 + 执行期 DENIED」，不是「不进 `tool/list`」**。
3. **测试替身需要 `rmcp` 的 `server` 特性**：正式构建的特性清单不变，`bin-tests` 追加 `server` + `transport-io` + `transport-streamable-http-server`，并引入 workspace 的 `axum` / `tokio-util` / `sysinfo`（后两者用于替身 HTTP 监听与「PID 不存在」断言）。已回填 D-P4-002 的实测结果与 `docs/workspace.md` 的 `bin-tests` 依赖行。
4. **报告渲染补了 MCP 段**：`crates/bin-tests/src/util/observability.rs` 的指标表新增一行 `MCP`（状态现状 + `connects` / `reconnects` / `calls` / `call_errors` / `call_timeouts` / `shutdown_kills`），否则 `metrics` 的 `mcp` 段只有 `mcp.metrics` 用例的接口断言，阶段报告里看不到数字。
5. **用例自身修了一处断言顺序依赖**：`mcp.call_error` 原先按审计数组下标取「工具级错误」与「协议级错误」，而同一轮的两个调用是**并行**执行的，审计落库顺序不保证等于模型给出的顺序——首轮复跑里它偶发失败（`run-1795681959288-13716`）。改为按可见名查找后，`mcp` 定向（19/19）与全量（123/123）各跑一次均稳定通过。这也是「失败信息能定位到具体工具」的一次自证。

---

## 五、未取证项与原因

**无。** 九组判据逐条成立，没有「未授权」项，也没有因为缺少特性 / 环境而降级的项：

- `metrics` 类判据（L-5 的 `reconnects`、`mcp.metrics`、C-7 的 `running_now`）在 `make test` 的 `metrics` 构建下全部取到值；未启用时用例会显式记「未采集：原因」而不是静默通过（`mcp.metrics` 的 `Err` 分支）。
- L-8 的「关闭期间在跑的 Run 落 `CANCELLED`」用既有用例 `run.shutdown` 取证（本轮全量运行仍绿），MCP 侧只举证关闭顺序（源码）与关闭结果（`mcp.shutdown`）。

---

## 六、遗留风险与已知限制

| 编号 | 内容 | 影响 | 后续 |
|---|---|---|---|
| `KI-P4-001` | `docs/api/mcp.md` 的「`deny` 的工具在注册环节就不进入」与实现不一致：`deny` 的条目注册在表里（执行期 `DENIED`），因此 `tool/list` 与 `toolCount` 会包含它们（模型清单与请求体不含） | 管理面看到的工具数大于模型可见数；文档读者会误以为 `deny` 等于「不发现」 | 二选一：把文档措辞改为「注册在册但对模型不可见」，或让 `tool_snapshot` 过滤 `deny` 条目。建议前者（`toolCount` 的语义是「注册在册」）。P5 接入 Skill 目录前处理 |
| `KI-P4-002` | Windows 没有 job object：stdio server 若自己派生孙进程（如 `npx`），关闭时只保证直接子进程退出 | 极端情况下替身的孙进程可能残留；`npx` 类场景需要进程树回收 | 沿用阶段 README §七 风险 5；如后续引入需要孙进程回收的 server，再评估 Windows Job Object 或 wrapper |
| — | 文件访问声明（`file_access`）是**自述**而非强制 | 它只影响并发推导与审计，不是沙箱；MCP server 实际读了什么文件无法拦截 | 属 P4 的既定边界（README §二 Out of Scope），不在本阶段修 |
| — | MCP 工具数量直接抬高每次请求的 token 成本 | server 暴露几十个工具时，上下文与费用上升 | 由配置侧 `deny` / 只启用必要 server 控制，暂无自动裁剪策略（P5 的 Skill 目录可能承担筛选） |

---

## 七、数据来源口径声明

- 本报告的数字与结论只来自四类来源：**命令退出码与输出**（`cargo` / `make` / `cargo tree` / `grep`）、**接口响应**（`api/mcp/*`、`api/tool/audit/list`、`api/probe/metrics`）、**客户端事件与用例自报的度量**、**测试替身写出的结构化证据文件**（`mock-mcp --evidence` 的 `clientInfo` / `toolLists` / `listChangedSent` / `calls`）。
- **没有读取、解析或检索任何 `server.log` 与日志目录**；失败用例的日志路径只在测试输出里给出，供人工排查。
- `metrics` 的数字来自 `GET api/probe/metrics` 的结构化快照（`mcp` 段）；本进程（`bin-tests`）的数字来自 `manager-agent` 的内存快照。未采集时用例显式记「未采集：原因」。
- 用例报告落在 `target/tests/<run-id>/README.md` 与 `mcp.md`，本报告引用的运行目录为 `run-1795682083908-42744`（`mcp` 定向）、`run-1795682098490-2964`（全量）、`run-1795682138292-42856`（`tool` + `client`）。
