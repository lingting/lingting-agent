# P4 验收清单

**状态：待开工（判据已定稿，尚无取证）。** 每条都要能实际验证；**任一条不通过，P4 不视为完成**。

验证方式只有四类：

- **命令**：可直接执行的命令与期望退出码 / 输出特征。
- **用例**：`make test ARGS="mcp"` / `ARGS="--all"` 下的 `crates/bin-tests` 用例（用例名即证据）。
- **静态核对**：`cargo tree`、源码检索、`git diff` 限定到本次改动文件、逐字段比对表。
- **人工**：只能人工判定的项，必须写明判定动作。

除「用例」类外，不得以「跑一遍看看」作为证据；**未获明确授权不新增单元测试**（`AGENTS.md` 第 28 条只允许 `bin-tests`），**未获明确授权也不执行任何用例**——未授权的条目在报告中标注「未授权」，不得留空、不得算通过。

证据只允许来自接口响应、客户端事件与 `metrics` 快照；不得读取、解析或检索日志文本（`AGENTS.md` 测试数据采集条款）。

---

## 1. Build（B-1 ~ B-4）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| B-1 | workspace 全量可编译 | `cargo build --workspace` 退出码 0，无 error |
| B-2 | 格式一致 | `cargo fmt --all --check` 退出码 0 |
| B-3 | Clippy 零告警 | `cargo clippy --workspace --all-targets -- -D warnings` 退出码 0 |
| B-4 | SDK 与前端门禁 | `make gen`、`make gen-check`、`make ui-check` 均退出码 0 |

## 2. Architecture（A-1 ~ A-7）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| A-1 | `agent-mcp` 依赖方向正确 | `cargo tree -p agent-mcp` 只有 `agent-tool` / `lib-core` / `rmcp` / 通用库与框架 crate；**没有** `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `lib-db` / `types-core` / `lib-web*` / `bin-*` |
| A-2 | 协议类型只在 `agent-mcp` | `cargo tree -i rmcp` 只命中 `agent-mcp` 与 `bin-tests`；`grep -rn "rmcp" crates --include=*.rs` 除 `crates/agent-mcp/` 与 `crates/bin-tests/` 外无命中 |
| A-3 | 工具层不认识 MCP | `grep -rniE "mcp|rmcp" crates/agent-tool/src` 为空 |
| A-4 | 装配层不泄漏协议类型 | `grep -rn "rmcp" crates/manager-agent/src`、`crates/lib-web/src` 为空；两处只出现 `agent_mcp` 的业务类型 |
| A-5 | 运行时零改动 | `git diff --stat` 显示 `crates/agent-runtime/` 无改动（P4-002 / P4-003 若必须调整 `loop_/tool.rs` 的借用方式，需在报告中单独列出并说明「仅签名适配」） |
| A-6 | 未创建被禁止的 Crate | 仓库中不存在 `manager-core` / `types-api` / `bin-tui` / `bin-web` / `bin-mock-server` / `agent-mcp-server` |
| A-7 | 结构文档同步 | `docs/workspace.md` 的 `agent-mcp` 段（职责、依赖、边界）与 `Cargo.toml` 的 members、`cargo tree` 的事实逐条一致 |

## 3. Contract & Schema（S-1 ~ S-7）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| S-1 | `Tool` 契约支持动态数据 | 静态核对：`fn name(&self) -> &str`、`fn description(&self) -> &str`、`fn schema(&self) -> &ToolSchema`；全仓无 `&'static str` 的实现；`Box::leak` 在 `crates/agent-mcp/src` 中 0 命中 |
| S-2 | 外部 schema 原样透传 | 用例 `mcp.schema`：`GET api/mcp/tool/list` 的 `parameters` 与 `mock-mcp` 剧本里的 `inputSchema` 逐字段相等（不剥键、不重写） |
| S-3 | 可见名可复算且稳定 | 用例 `mcp.name`：非法字符 / 大写 / 超长 / 空名 / 与内置工具重名五类输入，两次执行的可见名完全一致，且都满足 `^[a-z][a-z0-9_]{1,63}$` |
| S-4 | 命名前缀与原名可查 | 用例 `mcp.discover`：可见名以 `mcp__<serverId>__` 开头；`tool/list` 的 `originalName` 等于剧本原名 |
| S-5 | 权限可见性一致 | 用例 `mcp.denied`：`deny` 的工具不出现在请求体的 `tools` 字段里；执行期模型凭空构造该名字得到 `DENIED` |
| S-6 | 契约兼容 | 静态核对：`ToolDefinition` / `ToolCallVO` / `RunEvent` 三处契约字段未变（`git diff` 只含新增的 MCP DTO）；`make gen` 产物里只有新增的 `api/mcp/*` 与 DTO |
| S-7 | 文件访问声明生效 | 用例 `mcp.file_access`：配置 `{"*":"read","write*":"write","pre*write":"read-write","write_file":"none"}` 时 `read_file` → `READ`、`write_log` → `WRITE`、`pre_commit_write` → `READ_WRITE`、`write_file` → `NONE`；去掉精确名后 `write_file` 命中 `write*`；两级模式同分时结果确定（同配置两次启动一致）；无命中 → `NONE`；命中 `write` 的工具在同一轮并行调用里被串行（`tool_call_started` 时间不重叠） |

## 4. Transport & Handshake（T-1 ~ T-4）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| T-1 | stdio server 能被启动并握手 | 用例 `mcp.discover`：`bin-tests` 的 `mock-mcp` 子进程被启动，`GET api/mcp/server/list` 的 `status` 为 `READY` |
| T-2 | HTTP server 行为一致 | 用例 `mcp.http`：同一份剧本经 HTTP transport 连接后 `status` 为 `READY`，且能完成一次调用 |
| T-3 | 握手超时受配置约束 | 用例 `mcp.config`（`connect_timeout_ms` 极小 + 不响应的替身端口）：`status` 为 `FAILED`、`lastError` 是可读文案，服务仍正常启动 |
| T-4 | 客户端标识 | 用例 `mcp.discover`：替身记录到的 `clientInfo.name` 为 `lingting-agent`、`version` 等于 workspace 版本 |

## 5. Lifecycle（L-1 ~ L-8）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| L-1 | 启停幂等 | 用例 `mcp.api`：连续两次 `ENABLE` / `DISABLE` 的返回状态一致，`tool/list` 的数量在第二次后不变 |
| L-2 | 启动不阻塞 | 用例 `mcp.config`：一个不可达 server + 一个正常 server，服务能启动且入口接口可用；`server/list` 分别为 `FAILED` / `READY` |
| L-3 | 崩溃即状态可见 | 用例 `mcp.crash`：替身退出后 `status` 变为 `FAILED`，`lastError` 非空 |
| L-4 | 崩溃后调用不挂死 | 用例 `mcp.crash`：崩溃后的工具调用在 `[tools] timeout_ms` 内返回可读错误，`tool_call_completed` 的 `toolStatus` 不是 `OK`，Run 正常收尾 |
| L-5 | 退避重连 | 用例 `mcp.reconnect`：`RECONNECT` 能把 `FAILED` 拉回 `READY`；`reconnect_max_backoff_ms = 0` 时不再自动尝试（`metrics.mcp.reconnects` 不增长） |
| L-6 | 工具清单变更 | 用例 `mcp.discover`（替身触发 `notifications/tools/list_changed` 后新增一个工具）：`tool/list` 出现新工具，且模型的 `tools` 字段同步 |
| L-7 | 关闭无孤儿进程 | 用例 `mcp.shutdown`：关闭服务后替身的直接子进程 PID 不存在（`kill_on_drop` 兜底也算通过）；`metrics.mcp.shutdown_kills` 有对应计数 |
| L-8 | 关闭顺序正确 | 静态核对 + 用例：`holder::shutdown` 中 `handler::shutdown` 先于 `manager.shutdown_all()`；关闭期间在跑的 Run 落 `CANCELLED`（不是 `FAILED`） |

## 6. Call Semantics（C-1 ~ C-9）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| C-1 | 调用成功闭环 | 用例 `mcp.call`：模型要求调用 → 执行 → 结果进下一轮请求 → 最终助手回复包含基于结果的答案 |
| C-2 | 结果映射 | 用例 `mcp.result`：纯文本 / 文本 + 图片段 / 仅 `structuredContent` / 空内容四种形态都有确定文本，且总文本非空 |
| C-3 | 失败回灌 | 用例 `mcp.call_error`：`is_error = true` 与协议级错误都变成模型可读文本，Run 不 `FAILED`、不挂起 |
| C-4 | 超时 | 用例 `mcp.timeout`：超过 `[tools] timeout_ms` 时审计 `status = TIMEOUT`，Run 继续下一轮 |
| C-5 | 取消 | 用例 `mcp.cancel`：调用中取消 Run → 审计/事件为 `CANCELLED`，Run 落 `CANCELLED`，无残留后台任务（进程 `Drop` 后不再有该调用的完成事件） |
| C-6 | 权限拒绝 | 用例 `mcp.denied`：`deny` 的调用产出 `DENIED` 审计记录，模型收到可读文本 |
| C-7 | 确认交互 | 用例 `mcp.confirm`：`confirm` 的工具在下发 `tool_call_confirm_required` 后 Run 进 `WAITING`、`metrics.limit.running_now` 不含它；批准后执行、驳回后回灌 `用户拒绝调用该工具` |
| C-8 | 审计 | 用例 `mcp.audit`：`GET api/tool/audit/list` 的记录 `toolName` 是可见名、`fileAccess` 等于配置里的声明值、`status` 与事件一致 |
| C-9 | 事件契约不变 | 用例 `mcp.call`：`seq` 连续递增，`event:` 行与 JSON 同值，只有既有的 `tool_call_*` 类型（新增事件 0 个） |

## 7. Interface（I-1 ~ I-6）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| I-1 | 接口清单 | 静态核对：`crates/lib-web/src/mcp/api.rs` 只有 3 个 `#[web_api_*]`；接口总数 29（26 + 3） |
| I-2 | `server/list` 语义 | 用例 `mcp.api`：含 `disabled` 的 server；按 `id` 升序；字段与 `docs/api/mcp.md` 的表一致 |
| I-3 | `tool/list` 语义 | 用例 `mcp.api`：按可见名升序；`serverId` 过滤生效；未知 `serverId` 返回空列表 |
| I-4 | `server/update` 语义 | 用例 `mcp.api`：四个 `action` 都返回动作完成后的状态；未知 `serverId` 返回 400 + `MCP 服务器不存在` |
| I-5 | 文档与代码一致 | 静态核对：`docs/api/mcp.md` 的 path / 字段 / 枚举取值与代码逐项一致；`docs/api/readme.md` 的领域表与总数已更新 |
| I-6 | SDK 覆盖新接口 | `make gen` 产物里出现三条 path 与 DTO（`grep` 生成目录）；`make gen-check` 通过 |

## 8. Error & Safety（E-1 ~ E-5）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| E-1 | 文案单一产点 | 静态核对：固定文案集中在 `agent-mcp/src/error.rs`；全仓没有第二处拼接同类文案 |
| E-2 | 不泄漏敏感信息 | 用例 `mcp.http`（配置带 `Authorization` header）：事件、审计、接口响应里检索 `Bearer` / header 值 / 命令行 / URL 全部 0 命中 |
| E-3 | 不泄漏内部细节 | 静态核对：`McpError` 的 `Display` 不含 `rmcp` / `ServiceError` 的原文；原文只进 `tracing::debug` |
| E-4 | stdio 默认不启动 | 用例 `mcp.config`：`enabled = false` 的 server 不产生子进程（`server/list` 为 `DISABLED`，PID 文件不存在） |
| E-5 | 配置即边界 | 静态核对：配置装载期对 `id` / transport 字段 / URL / 枚举的校验逐条存在；非法配置退出码 2（用例 `mcp.config`） |

## 9. Regression & Closure（R-1 ~ R-4）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| R-1 | P3 用例全绿 | `make test ARGS="tool"` 与 `ARGS="client"` 通过（`tool.*` / `client.*` 行为未变） |
| R-2 | 全量用例全绿 | `make test ARGS="--all"` 通过（需授权） |
| R-3 | 既有语义未变 | 用例 `run.*` / `persist.*` / `runtime.*` 通过；`api/client/stream` 的事件类型集合与 P3 相同（静态核对 `RunEventKind` 的取值清单） |
| R-4 | 阶段收尾 | `docs/reports/p4-020-acceptance.md` 出稿；`.steps/README.md` 与 `.steps/p4/README.md` 的状态行一致；未取证项显式标注原因 |

---

## 口径说明

1. **「用例」类判据的取证前提是执行授权**。未授权时报告里写「未授权」，不得写「通过」。
2. **`metrics` 类判据**只在 `debug` + `metrics` 特性构建下成立；未启用时按「未采集：原因」显式报告。
3. **L-7 只断言直接子进程**：`npx` 之类会再派生孙进程，Windows 无 job object，孙进程残留属于 §七 风险 5 的已知限制。
4. **A-5 的口径**：`agent-runtime` 的「零改动」指不引入 MCP 概念、不新增依赖；若因 `ToolRegistry` 的返回类型调整需要改一行借用写法，需在报告中单列并说明理由，判据仍算成立。
