# P3-032 阶段验收与报告（Tool System）

- **阶段**：P3 Tool System（8 个批次 / 32 个任务）
- **任务**：P3-032（证据覆盖 Batch A ~ H 的已落地实现，含 P3-031 的文档同步与 P3-030 的用例）
- **前置**：Batch A ~ G 全部落地；P3-030（27 个 `tool.*` + `client.post`）与 P3-031（文档同步）在本轮结案
- **执行日期**：2026-11-25
- **环境**：Windows 11 / Git Bash；CPU 18 核；用例并发度 18（默认 `available_parallelism()`）
- **判据来源**：[`.steps/p3/acceptance.md`](../../.steps/p3/acceptance.md)；任务清单见 [`.steps/p3/tasks.md`](../../.steps/p3/tasks.md)
- **测试授权**：本轮 `make test ARGS="--all"` 一次经明确授权后执行（103 个用例全部通过，运行目录 `target/tests/run-1795605450775-43892/`）；P3-030 的同一目标另有一次 103 用例全绿运行（`run-1795599811697-44180`）

---

## 一、结论

1. **七条验收方向全部成立**：自主选择工具并继续推理、三种失败回灌且不卡死、并行且顺序稳定、取消可中断且无残留、每次调用都有审计且可按 Run 查询、未授权工具不进清单也不执行、需确认的工具在确认前不执行且可挂起唤醒。证据见 §三 的 L / C / P / F / T 组。
2. **五条结构性边界成立**：`agent-tool` 依赖方向、Rig 类型不出适配层、Runtime 不认识具体工具、被测 Crate 不含 `#[cfg(test)]`、接口改动收敛为「1 改名 + 1 新增」。
3. **门禁全绿**：`cargo build --workspace`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`make gen-check`、`make ui-check`、`cargo build --release -p bin-agent` 退出码均为 0；全量回归 103/103 通过。
4. **与规划不符的两处口径已修订**（§四）：R-1 的「请求体无 `tools` 键」在 P3 不成立（新增 D-P3-028）；A-2 的反向依赖清单需把 `manager-agent` 计入（装配层直接持有 Registry 与 Executor）。
5. **两处判据只做到静态取证**：C-3（确认点截断同轮后续调用）缺「确认点在中间的批量调用」用例；T-8 的「声明与实现人工核对」按人工结论记录。二项均见 §八。

---

## 二、Build 与结构（B / A 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| B-1 | workspace 全量可编译 | 成立 | `cargo build --workspace` 退出码 0 |
| B-2 | 格式一致 | 成立 | `cargo fmt --all --check` 退出码 0 |
| B-3 | Clippy 零告警 | 成立 | `cargo clippy --workspace --all-targets -- -D warnings` 退出码 0，输出无 warning / error |
| B-4 | SDK 与前端门禁 | 成立 | `make gen-check`、`make ui-check` 退出码 0；`make gen` 作为 `make test` 前置执行，生成物无 diff |
| A-1 | `agent-tool` 依赖方向正确 | 成立 | `cargo tree -p agent-tool --depth 1` = `agent-core` / `agent-context-core` / `lib-core` + `schemars` / `serde` / `serde_json` / `serde_with` / `strum` / `specta` / `tokio` / `tokio-util` / `async-trait` / `futures-util` / `url` / `chrono` / `anyhow` / `framework-http` / `framework-proc-*`；全树无 `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `lib-web*` / `bin-*` |
| A-2 | 工具不反向依赖运行时 | 成立（清单修订） | `cargo tree -i agent-tool` 的反向边为 `agent-runtime` / `manager-agent` / `service-core` / `bin-tests`（`manager-agent` 是本阶段装配层的直接依赖，方向仍在上层，见 §四）；`grep -rn "agent_runtime" crates/agent-tool/src` 为空 |
| A-3 | Runtime 不认识具体工具 | 成立 | `grep -rnE "calculator\|current_time\|file_read\|file_search\|http_request" crates/agent-runtime/src` 为空；执行只经 `ToolRegistry` / `ToolExecutor`（`agent-runtime/src/loop_/tool.rs`） |
| A-4 | Rig 类型不出 `agent-route/src/rig/` | 成立 | `grep -rn "rig_core" crates --include=*.rs` 只命中 `crates/agent-route/src/rig/` |
| A-5 | 未创建被禁止的 Crate | 成立 | `crates/` 下不存在 `manager-core` / `types-api` / `bin-tui` / `bin-web` / `bin-mock-server` / `agent-tool-builtin`（`lib-bin-tui` / `lib-bin-web` 是既有装配层，不在禁止项内） |
| A-6 | 结构文档同步 | 成立 | `docs/workspace.md` 的 `agent-tool` 段、`service-core → agent-tool` 依赖与 `Cargo.toml` members 逐条对照一致；本轮补齐 `service-core` 的审计职责、`lib-db` 的 `0002` 迁移说明，并修正「`service-core` 不依赖任何 `agent-*` Crate」这一过期表述 |

---

## 三、契约、循环、裁剪、并发、权限、确认、内置工具、客户端、回归

### 3.1 契约与 Schema（S 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| S-1 | 参数 schema 单一来源 | 成立 | 五个内置工具的 schema 都由 `schema::of::<*Args>()` 给出（`builtin/{calculator,current_time,file_read,file_search,http_request}.rs` 的 `SCHEMA`）；全仓检索 `"type":"object"` 只命中 `agent-tool/src/schema.rs` 的一处文档注释，无手写参数 schema |
| S-2 | 校验与 schema 同源 | 成立 | `tool.schema`：必填 / 可选 / 枚举 / 数字四类字段逐类对照通过（必填 `["name"]`；`max_results` 描述含 `integer=true`；`zone` 枚举；未知字段 `additionalProperties=false`）；serde 侧同一份 `*Args` 报 `missing field \`expression\`` |
| S-3 | 模型可见清单稳定 | 成立 | `tool.schema` 两次执行清单同为 `["calculator","current_time","file_read","file_search"]`；`tool.denied` 证明 deny 的 `http_request` 不在清单、`tool.schema` 的另一配置证明 `confirm` 的在清单 |
| S-4 | 事件契约兼容 | 成立 | `tool.loop` 断言的 `seq` 从 0 连续递增、`event:` 行与 JSON `event` 同值；`tool.events` 的下行流顺序为 `[RunStarted, ToolCallStarted, ToolCallConfirmRequired, RunStarted, ToolCallCompleted, MessageDelta×3, RunCompleted]`；P2 五个事件的字段在 `RunEvent` 里只追加了 6 个可选字段（`toolCallId` / `toolName` / `toolArguments` / `toolStatus` / `toolDurationMs` / `toolRound`），既有字段语义未变 |
| S-5 | 审计载荷字段完整 | 成立 | `tool.audit`：`toolCallId` / `toolName` / `toolStatus` / `toolDurationMs` / `fileAccess` 均有值；`durationMs` 与 `createdAtMs` 在 wire 上是字符串 |
| S-6 | 工具调用流聚合正确 | 成立 | `tool.stream`：三段 `Arguments` 拼成 `{"expression":"6*7"}`；缺 `End` 落 `FAILED`「调用模型失败：响应不完整」，不发出半截参数；纯文本流无工具事件、事件序列与 P2 一致 |
| S-7 | 工具轨迹进历史 | 成立 | `tool.loop`：`message/list` 4 条，中间轮 assistant 消息带 `toolCalls`、TOOL 消息带 `toolCallId`，`seq` 升序 |
| S-8 | `client/post` 契约完整 | 成立 | `client.post`：`PING` / `INQUIRY_ACK` 各返回 `pongAtMs`；四种 400 文案逐条命中（`工具确认缺少 toolCallId` / `工具确认缺少 decision` / `心跳与问询应答不得携带工具确认字段` / 框架 `请求参数转换失败`）；未注册客户端返回 `客户端未注册，请重新连接` |

### 3.2 工具循环（L 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| L-1 | 自主选择工具并继续推理 | 成立 | `tool.loop`：第一轮要求调用 `calculator`，第二轮给出终答「Mock 回复：20」，最终助手消息来自第二轮 |
| L-2 | 参数非法被回灌 | 成立 | `tool.invalid_args`：Run `COMPLETED`，回灌文本「参数不合法：unknown field `expr`, expected `expression`」 |
| L-3 | 执行超时被回灌 | 成立 | `tool.timeout`：Run `COMPLETED`，回灌「工具执行超时（> 300 毫秒）」，实测耗时 300ms |
| L-4 | 执行失败被回灌 | 成立 | `tool.failed`：Run `COMPLETED`，回灌「工具执行失败：除数不能为零」，不含内部路径 |
| L-5 | 轮次上限生效 | 成立 | `tool.rounds`：`max_tool_rounds = 1` 时第二轮被拦，Run `FAILED`「工具调用轮次超过上限」，不写最终助手消息 |
| L-6 | 多轮请求携带完整轨迹 | 成立 | `tool.loop`：第二轮请求体含 assistant `tool_calls` 与配对的 tool 结果消息 |
| L-7 | 每轮边界落库 | 成立 | `tool.loop`：Mock 在收到第二轮请求时反查 `message/list`，本轮的 assistant `toolCalls` 与 TOOL 结果已在库中 |
| L-8 | Context 超预算即失败 | 成立 | `tool.context`：把预算压到最小后 Run `FAILED`「上下文超出模型预算」，不静默丢工具结果 |

### 3.3 Context 裁剪（X 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| X-1 | 组完整性 | 成立 | `tool.context`：最后一次请求 6 条消息，历史留 1 组、本 Run 1 组，不存在「assistant `tool_calls` 缺结果」或孤儿 TOOL |
| X-2 | 整组丢弃 | 成立 | `tool.context`：预算只够最近一组时旧组整体消失（历史 1 组 = 4 条结果），新组完整（本 Run 1 组 = 6 条结果） |
| X-3 | 无工具时行为不变 | 成立 | `chat.context`：系统提示词单条在前、末条为本次输入、历史按 `seq` 升序居中；`chat.truncate` 仍按条数裁剪；`git diff` 未触及 `agent-context/src/trim.rs` 的 P2 算法 |

### 3.4 并发与取消（C 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| C-1 | 一轮内并行执行 | 成立 | `tool.parallel`：两个调用在同一轮发出，两个 `tool_call_started` 都在任一 `tool_call_completed` 之前到达 |
| C-2 | 结果顺序稳定可复现 | 成立 | `tool.parallel`：默认并发与 `max_parallel_calls = 1` 两种配置下完成顺序都是 `["call_mock_0","call_mock_1"]`（等于模型给出的调用顺序） |
| C-3 | 确认点截断后续调用 | **部分取证** | 代码侧成立：`agent-tool/src/batch.rs` 的 `execute_batch` 在 `confirm_index` 处 `calls.truncate(index + 1)`，同轮后续调用不进入执行；用例侧只有「一轮一个 confirm 调用」（`tool.confirm.approve`：审计 1 条）与「一轮两个 allow 调用」（`tool.parallel`），缺「确认点在中间的批量调用」用例。见 §八 遗留 1 |
| C-4 | 取消中断工具执行 | 成立 | `tool.cancel`：取消后 `run_cancelled`、工具的 `tool_call_completed` 为 `CANCELLED`、慢响应服务端连接被关闭、无残留审计 |
| C-5 | 无孤儿任务 | 成立 | `tool.cancel`：取消后 1s 内无新事件、无新增审计；`run/get` 为 `CANCELLED`；服务端快照「Run 执行：读取瞬间 0 个在跑」 |

### 3.5 权限与审计（P 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| P-1 | 默认拒绝的工具不进上下文 | 成立 | `tool.denied`：默认配置下 `last-request` 的工具清单为 `["calculator","current_time","file_read","file_search"]`，不含 `http_request` |
| P-2 | 未授权调用被拒绝且可读 | 成立 | `tool.denied`：模型凭空调用 `http_request`，回灌「工具未授权：http_request」，Run `COMPLETED` |
| P-3 | 拒绝不触发执行 | 成立 | 同用例的审计状态为 `DENIED`；`tool.builtin.http` 另证「白名单外无连接」（假服务端命中次数不增长） |
| P-4 | 每次调用都有审计记录 | 成立 | `tool.audit`：审计条数与 `tool_call_started` 条数一致、`toolCallId` 一一对应；`tool.confirm.approve` 证挂起期间以 `PENDING` 存在 |
| P-5 | 可按 Run 查询 | 成立 | `tool.audit`：`GET api/tool/audit/list?runId=…` 返回 2 条、按时间升序、状态 `[Ok, InvalidArguments]`；未知 `runId` 返回 `[]`（同用例断言） |
| P-6 | 参数脱敏 | 成立 | `tool.audit`：`api_key` 的值在审计 `arguments` 里为 `***`；脱敏在写出侧完成（`agent-tool/src/audit.rs`），查询接口与事件都只见到打码后的文本 |
| P-7 | 配置校验 | 成立 | `tool.config`：`timeout_ms = 0` / `max_result_chars = 0` / `max_parallel_calls = 0` / 未知工具名 / 相对路径白名单 / 带协议的 host 六类非法配置各自报出点名字段的文案；`[tools]` 里不存在「确认超时」字段（静态核对 `types-core/src/config/tools.rs`） |

### 3.6 确认交互（F 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| F-1 | 确认前不执行 | 成立 | `tool.confirm.approve`：挂起时审计为 `PENDING`（`call_mock_0`）、工具未被调用；假服务端在批准前命中 0 次 |
| F-2 | 挂起回 `WAITING` 且不占位 | 成立 | 同用例：`run/get` 为 `WAITING`，快照 `running_now = 0`，`drive` 已返回无后台任务（服务端快照「Run 执行：读取瞬间 0 个在跑」） |
| F-3 | 不阻塞同会话后续 Run | 成立 | 同用例：挂起期间同一会话的第二个 Run 执行到 `COMPLETED` |
| F-4 | 批准后重新排队并执行 | 成立 | 同用例：`client/post { TOOL_CONFIRM, APPROVE }` 后 Run `COMPLETED`，工具命中 1 次、审计转 `OK`，模型请求共 3 次（首轮未重跑） |
| F-5 | 驳回回灌可读文本 | 成立 | `tool.confirm.reject`：`COMPLETED`，回灌「用户拒绝调用该工具」，审计 `DENIED` |
| F-6 | 重连补发 | 成立 | `tool.confirm.reconnect`：同一 `toolCallId`（`call_mock_0`）在建立下行流后再次收到 `tool_call_confirm_required`，再次确认成功 |
| F-7 | 挂起中断连的收尾 | 成立 | `tool.confirm.disconnect`：Run `CANCELLED`、审计 `CANCELLED`、挂起列随取消链路清空；重复确认返回 `客户端未注册，请重新连接` |

### 3.7 内置工具（T 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| T-1 | `calculator` 正确且安全 | 成立 | `tool.builtin.calculator`：`(2+3)*4 = 20`、`7 % 4 = 3`、`-3 % 2 = -1`、`1.5 + 2.25 = 3.75`；超长表达式与语法错误返回「参数不合法：…」，不 panic |
| T-2 | `current_time` 口径固定 | 成立 | `tool.builtin.current_time`：`utc` 输出 `2026-11-25T11:17:32.887Z（Unix 毫秒：…）`，`local` 输出 `+08:00` 偏移；非法 `zone` 返回「参数不合法：zone 只支持 utc 或 local」 |
| T-3 | `file_read` 白名单生效 | 成立 | `tool.builtin.file`：白名单外路径被拒且错误文本不含绝对路径；超 `max_bytes`（64 字节）不读入；二进制被拒 |
| T-4 | `file_search` 遍历期剪枝 | 成立 | 同用例：`target` 目录内的同名文件不出现在结果里，`nested/keep.txt` 命中（大小写不敏感），结果顺序稳定 |
| T-5 | `http_request` 默认拒绝 | 成立 | `tool.builtin.http`：默认配置下调用被拒；显式允许后白名单 host 命中 2 次、白名单外无连接 |
| T-6 | 不跟随重定向 | 成立 | 同用例：假服务端返回 302 指向白名单外地址，工具原样返回 302，未跟随 |
| T-7 | 结果截断 | 成立 | `tool.truncate`：`max_result_chars` 调小后结果为 `2026-11-25` 并带「结果已截断」提示，截断落在字符边界 |
| T-8 | 文件操作声明与实现一致 | 成立 | `tool.decl`：五个工具 `fileAccess` = `None` / `None` / `Read` / `Read` / `None`，`default_concurrency()` 四条映射逐项正确（`None`/`Read` → `Parallel`，`Write`/`ReadWrite` → `Exclusive`），实际并发为 `Parallel ×5`；**人工**：逐个核对 `builtin/` 实现——`calculator` / `current_time` / `http_request` 不触本地文件，`file_read` 只读，`file_search` 只遍历；没有 `Write` / `ReadWrite` 声明，也没有任何工具写文件 |

### 3.8 客户端（W 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| W-1 | 事件可达客户端 | 成立 | `tool.events`：下行流收到 `ToolCallStarted` / `ToolCallConfirmRequired` / `ToolCallCompleted` 三个事件，顺序与终态事件一致（顺序见 S-4） |
| W-2 | TUI 展示与确认 | 成立 | `tool.tui`：`y` → `Confirm{APPROVE}`、`n` → `Confirm{REJECT}`、无待确认时按键不上行、重复按键不上行；`DENIED` / `OK` 各自成行；`cargo clippy -p lib-bin-tui --all-targets -- -D warnings` 通过 |
| W-3 | WebUI 展示与确认 | 成立 | `make ui-check` 退出码 0；`tool.web`（jsdom）覆盖三个事件分支、确认不可重复点击与未完成标记；`tool.web.e2e`（真实 Chromium）走完「待确认 → 允许 → 完成」，截图落在 `lingting-agent-ui/tests/.artifacts/tool-card-confirm.png` 与 `tool-card-done.png`（人工目测：与气泡同字号、卡片宽度上限 560px、状态标签走 antd 语义类） |
| W-4 | 确认上行同源 | 成立 | TUI（`lib-bin-tui/src/worker.rs`）与 WebUI（`lingting-agent-ui/src/api/link.ts`）都只调生成的 SDK 方法（`clientPost` / `Client::client_post`），请求体字段来自 `ClientPostPO`；页面与 worker 无 `api/…` 字面量 |

### 3.9 回归（R 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| R-1 | 无工具时行为零变化 | 成立（口径修订） | `chat.multiturn` / `chat.context` / `chat.truncate` / `run.cancel` 全绿，请求消息序列、裁剪与 token 语义与 P2 一致；**但**判据原文「Mock `last-request` 中无 `tools` 键」在 P3 不成立：默认配置下有 4 个可见工具，请求体按 G-1 携带工具定义（见 §四 D-P3-028） |
| R-2 | 接口变更收敛 | 成立 | `grep -rho 'path = "api/…"'` 共 27 条，去掉仅 `debug` + `metrics` 构建注册的 `api/probe/metrics` 为 26 条；P2 的 25 条中只有 `client/update → client/post` 改名，另新增 `tool/audit/list`；其余路径与 DTO 字段只增不改 |
| R-3 | 状态机与终态事务不变 | 成立 | `run.cancel` / `run.failed_auth` / `run.failed_timeout` / `run.failed_upstream` / `persist.*` 全绿；成功 Run 的最后一条消息是助手消息（`tool.loop`），失败 / 取消 Run 无最终助手消息（`tool.rounds` / `tool.cancel`） |
| R-4 | 全量回归 | 成立 | `make test ARGS="--all"` 退出码 0：103 个用例全部通过，报告 `target/tests/run-1795605450775-43892/README.md` |
| R-5 | 发布构建可用 | 成立 | `cargo build --release -p bin-agent` 退出码 0；`api/probe/metrics` 由 `#[cfg(all(debug_assertions, feature = "metrics"))]` 注册，release 与 `make build` 不带该特性；`http_request` 默认仍为 deny（`ToolRisk::Network` 的缺省等级是拒绝，见 `agent-tool/src/definition.rs` / `registry.rs`） |
| R-6 | 旧数据可读 | 成立 | `persist.migration` 的「`0001` 时代」夹具：只跑 `0001`、账本只记 version 1，迁移后旧消息的三个工具字段为 `null`、账本为 `[1, 2]` |

---

## 四、偏差与修订

1. **R-1 的「请求体无 `tools` 键」不成立**（新增 [D-P3-028](../../.steps/p3/decisions.md)）。P3 的 G-1 就是让模型看到工具清单，默认配置下 `calculator` / `current_time` / `file_read` / `file_search` 四个工具始终可见，因此任何一次请求都带 `tools`。判据改写为「消息序列、裁剪与 token 语义与 P2 一致」，用例侧由 `chat.*` 全绿取证。
2. **A-2 的反向依赖清单需包含 `manager-agent`**。装配层直接依赖 `agent-tool`（构建 `ToolRegistry` 与 `ToolExecutor`），方向仍是「上层依赖下层」，不构成反向边；验收清单已按实际反查结果更新。`docs/workspace.md` 的 `manager-agent` 段落同步保留该事实。
3. **`docs/api/runtime.md` 的改名说明不再写旧路径字面量**，改述为「上行通道在 P3 由 `update` 动作改为 `post`」；`docs/reports/` 下两份历史验收报告（`p1.1-022` / `p2.1-008.2`）里的 `client/update` **保留原文**，它们是当时事实的记录，不作为现行文档。
4. **`docs/workspace.md` 的「`service-core` 不依赖任何 `agent-*` Crate」是过期表述**，本轮改为「只依赖类型层的 `agent-core` 与 `agent-tool`」。
5. **`docs/api/session.md` 补 `WAITING`、三个工具事件、`RunEvent` 的 6 个工具字段与「工具轮次与挂起」一节**，`docs/develop.md` 的会话业务接口段补 P3 的三处变化；`docs/api/readme.md` 补接口总数 26 条与领域计数。
6. **`lib-db` 段补迁移清单**（`0001` / `0002`）与 `service-core` 的工具审计职责，避免「结构文档只在规划期描述过一次」。

---

## 五、与 P2 / P1.1 决策的冲突处

| 冲突 | 处理 | 出处 |
|---|---|---|
| D-P2-008「流式期间不写库」 | 保留该条，另加**每轮边界落库**：进入下一轮之前本轮的 assistant `tool_calls` 与全部 TOOL 结果已提交；最终助手消息与 `COMPLETED` 仍是同一事务 | D-P3-024；`tool.loop` 的 L-7 取证 |
| P2「同会话串行」 | 加**唯一例外**：`WAITING`（挂起等确认）不占全局并发许可，同会话可继续执行后续 Run，历史顺序可能不等于发起顺序 | D-P3-023 / D-P3-027；`tool.confirm.approve` 的 F-2 / F-3 取证 |
| P1.1 上行契约 `POST api/client/update` | 破坏性改名为 `api/client/post`，同一批次改完服务端、两侧 SDK、CLI、TUI、WebUI 与用例，不保留旧路径 | D-P3-022；R-2 取证 |
| D-P3-023「挂起回 `QUEUED`」 | 修订为回 `WAITING`（`WAITING → QUEUED → RUNNING`），队列仍只取 `QUEUED` | D-P3-027；`tool.confirm.*` 四条用例 |
| P2 的 25 条接口 | 只改名 1 条、新增 1 条，不删不改其余 | R-2 |

---

## 六、已知限制（README §七 第 1~5 条）与对应判据

| # | 限制 | 对应判据 / 处置 |
|---|---|---|
| 1 | **确认没有超时，会被遗忘**：挂起的 Run 只能由「确认 / 取消 / 释放占用或断连 / 服务关闭」四条路径收尾，客户端关掉后该 Run 会一直停在 `WAITING` | F-7（断连收尾为 `CANCELLED`）、P-7（`[tools]` 无确认超时字段）；不设自动清理是 D-P3-023 的有意取舍 |
| 2 | **挂起打破同会话串行**：历史里可能出现「后发的 Run 先落消息」，客户端不能假设全局序号 | F-3（挂起期间第二个 Run 可 `COMPLETED`）；客户端按 `runId` 分组，TUI / WebUI 均按到达顺序渲染 |
| 3 | **接口改名是破坏性变更**：P1.1 时代的客户端与已发布 SDK 会直接 404 | R-2（只留 `client/post`）；`client.post` 用例覆盖现契约 |
| 4 | **`messages` 表变宽**：`tool_call_id` / `tool_name` / `tool_calls` 三列对普通消息恒为 `NULL` | R-6（旧库可读、新字段为 `null`）、S-7 |
| 5 | **挂起恢复依赖「最后一个 tool_call 没有配对结果」这一推断**：写坏配对就恢复失败 | L-7（每轮边界落库）、F-4（恢复后不重跑已完成轮次） |

---

## 七、数据来源口径声明

- 用例断言的数据只来自：客户端收到的 `RunEvent`、接口响应（`run/get` / `message/list` / `api/tool/audit/list` / `client/post`）、Mock 的 `/mock/last-request`、以及服务端进程内快照 `GET api/probe/metrics`。
- 不读日志、不解析事件文件文本：`crates/bin-tests/src` 内 `read_to_string` 的调用只指向 PID 文件、事件文件与前端源码，`cases/tool/` 下 0 处。
- 敏感信息未泄漏的判定在**写出侧**：脱敏在 `agent-tool/src/audit.rs` 与事件构造处完成，审计与事件里都不出现明文（P-6）；没有「事后扫描日志找明文」的断言。

---

## 八、遗留问题

1. **C-3 缺用例**：「确认点在中间的批量调用」没有专门用例（现为静态核对 + 两个间接用例）。补一个 `tool.parallel` 的确认变体即可闭环，需要新增用例代码与执行授权。
2. **T-8 的人工部分只在报告里留结论**：`fileAccess` 声明与实现的一致性靠人工核对，编译器不校验；新增工具时必须重做这一步。
3. **`stress` 类用例的容量假设对带工具的 Run 不再成立**：一个多轮工具 Run 会长时间占用全局许可；`stress.boundary` 本轮仍全绿（它不使用工具），带工具容量评估留到 P6（README §七 第 13 条）。
4. **审计表随 Run 级联删除**：`tool_calls` 以 `ON DELETE CASCADE` 挂在 `runs` 上，将来若加「删除 Run」接口需要重新决策（README §七 第 6 条）。
