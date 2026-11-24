# P3 验收清单

**状态：规划期（未开工）。** 判据已展开，尚未取证。结论与证据将汇总为 `docs/reports/p3-032-acceptance.md`。

每条都要能实际验证；**任一条不通过，P3 不视为完成**。

验证方式只有四类：

- **命令**：可直接执行的命令与期望退出码 / 输出特征。
- **用例**：`make test ARGS="tool"` / `ARGS="client"` 下的 `crates/bin-tests` 用例（用例名即证据）。
- **静态核对**：`cargo tree`、源码检索、`git diff` 限定到本次改动文件、逐字段比对表。
- **人工**：只能人工判定的项，必须写明判定动作。

除「用例」类外，不得以「跑一遍看看」作为证据；**未获明确授权不新增单元测试**（`AGENTS.md` 第 28 条只允许 `bin-tests`），**未获明确授权也不执行任何用例**——未授权的条目在报告中标注「未授权」，不得留空、不得算通过。

---

## 1. Build（B-1 ~ B-4）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| B-1 | workspace 全量可编译 | `cargo build --workspace` 退出码 0，无 error |
| B-2 | 格式一致 | `cargo fmt --all --check` 退出码 0 |
| B-3 | Clippy 零告警 | `cargo clippy --workspace --all-targets -- -D warnings` 退出码 0 |
| B-4 | SDK 与前端门禁 | `make gen`、`make gen-check`、`make ui-check` 均退出码 0 |

## 2. Architecture（A-1 ~ A-6）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| A-1 | `agent-tool` 依赖方向正确 | `cargo tree -p agent-tool` 只有 `agent-core` / `agent-context-core` / `lib-core` / 通用工具库；**没有** `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `lib-web*` / `bin-*` |
| A-2 | 工具不反向依赖运行时 | `cargo tree -i agent-tool` 只有 `agent-runtime` / `service-core` / `bin-tests`；`grep -rn "agent_runtime" crates/agent-tool/src` 为空 |
| A-3 | Runtime 不认识具体工具 | `grep -rnE "calculator\|current_time\|file_read\|file_search\|http_request" crates/agent-runtime/src` 为空 |
| A-4 | Rig 类型不出 `agent-route/src/rig/` | `grep -rn "rig_core" crates --include=*.rs` 只命中 `crates/agent-route/src/rig/` |
| A-5 | 未创建被禁止的 Crate | 仓库中不存在 `manager-core` / `types-api` / `bin-tui` / `bin-web` / `bin-mock-server` / `agent-tool-builtin` |
| A-6 | 结构文档同步 | `docs/workspace.md` 的 `agent-tool` 段、`service-core → agent-tool` 依赖与 `Cargo.toml` 的 members 逐条对照一致 |

## 3. Contract & Schema（S-1 ~ S-8）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| S-1 | 参数 schema 单一来源 | 静态核对：所有工具的参数 schema 都来自 `schema::of::<*Args>()`；全仓检索不到手写 `"type": "object"` 参数 schema |
| S-2 | 校验与 schema 同源 | 用例 `tool.schema`：同一份 `*Args` 的必填 / 可选 / 枚举 / 数字四类字段，schema 声明与 serde 接受行为一致 |
| S-3 | 模型可见清单稳定 | 用例 `tool.schema`：两次执行清单顺序相同；`deny` 的工具不在清单里，`confirm` 的在 |
| S-4 | 事件契约兼容 | 用例 `tool.loop`：`seq` 从 0 连续递增；`event:` 行与 JSON `event` 同值；P2 的五个事件字段位置与含义不变（逐字段比对表）；新增事件为 `tool_call_started` / `tool_call_completed` / `tool_call_confirm_required` |
| S-5 | 审计载荷字段完整 | 用例 `tool.audit`：`toolCallId` / `toolName` / `toolStatus` / `toolDurationMs` / `fileAccess` 有值，`i64` 字段在 wire 上是字符串 |
| S-6 | 工具调用流聚合正确 | 用例 `tool.stream`：三次 `Arguments` 分片拼成完整 JSON；缺 `End` 时落 `FAILED` 且不发出半截参数；纯文本流事件序列与 P2 一致 |
| S-7 | 工具轨迹进历史 | 用例 `tool.loop`：`message/list` 返回的历史里，中间轮的 assistant 消息带 `toolCalls`，TOOL 消息带 `toolCallId`，且 `seq` 升序 |
| S-8 | `client/post` 契约完整 | 用例 `client.post`：`PING` / `INQUIRY_ACK` / `TOOL_CONFIRM` 三值各有正确响应；四种字段组合错误各返回固定 400 文案 |

## 4. Loop（L-1 ~ L-8）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| L-1 | 自主选择工具并继续推理 | 用例 `tool.loop`：Mock 第一轮要求调用 `calculator`，第二轮给出最终答案；断言最终助手消息内容来自第二轮 |
| L-2 | 参数非法被回灌 | 用例 `tool.invalid_args`：Run 以 `COMPLETED` 结束，模型收到的工具结果文本含「参数不合法：」且点明字段 |
| L-3 | 执行超时被回灌 | 用例 `tool.timeout`：Run 以 `COMPLETED` 结束，结果文本含「工具执行超时」 |
| L-4 | 执行失败被回灌 | 用例 `tool.failed`：Run 以 `COMPLETED` 结束，结果文本含「工具执行失败：」且不含内部路径 |
| L-5 | 轮次上限生效 | 用例 `tool.rounds`：`max_tool_rounds = 1` 时 Run 落 `FAILED`，`error` 为固定文案；助手消息不写库 |
| L-6 | 多轮请求携带完整轨迹 | 用例 `tool.loop`：Mock 侧 `last-request` 的第二轮请求含 assistant `tool_calls` 与配对的 tool 结果消息 |
| L-7 | 每轮边界落库 | 用例 `tool.loop`：第一轮工具执行完成后、第二轮请求发出前，`message/list` 已能查到本轮的 assistant `toolCalls` 与全部 TOOL 消息（时序由 Mock 侧在收到第二轮请求时反查服务端确认） |
| L-8 | Context 超预算即失败 | 用例 `tool.context` 的变体（把结果上限与历史预算压到最小）：Run 落 `FAILED`「上下文超出模型预算」，不静默丢工具结果 |

## 5. Context 裁剪（X-1 ~ X-3）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| X-1 | 组完整性 | 用例 `tool.context`：裁剪后历史里不存在「assistant `tool_calls` 缺 TOOL 结果」或「孤儿 TOOL 消息」 |
| X-2 | 整组丢弃 | 用例 `tool.context`：预算只够保留最近一组时，旧组整体消失、新组完整 |
| X-3 | 无工具时行为不变 | 用例 `chat.context`：Item 序列与 metadata 与 P2 基线一致（`git diff` + 用例断言） |

## 6. Concurrency & Cancellation（C-1 ~ C-5）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| C-1 | 一轮内并行执行 | 用例 `tool.parallel`：两个调用在同一轮发出，两个 `tool_call_started` 都在任一 `tool_call_completed` 之前到达 |
| C-2 | 结果顺序稳定可复现 | 用例 `tool.parallel`：返回顺序与模型给出的调用顺序一致；两次执行顺序相同 |
| C-3 | 确认点截断后续调用 | 用例 `tool.parallel` 的确认变体：确认点之后的调用没有被执行（审计条数可核） |
| C-4 | 取消中断工具执行 | 用例 `tool.cancel`：取消后不再有新事件，慢响应服务端连接被关闭，`tool_call_completed` 状态为 `CANCELLED`，`PENDING` 审计行已收尾 |
| C-5 | 无孤儿任务 | 用例 `tool.cancel` 收尾断言：取消后 1s 内无新事件、无新增审计记录、`run/get` 为 `CANCELLED`（人工补充：`metrics` 快照里 `running_now` 归零） |

## 7. Permission & Audit（P-1 ~ P-7）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| P-1 | 默认拒绝的工具不进上下文 | 用例 `tool.denied`：默认配置下 `http_request` 不在 `last-request` 的 `tools` 清单里 |
| P-2 | 未授权调用被拒绝且可读 | 用例 `tool.denied`：模型凭空调用 `http_request`，结果文本为「工具未授权：http_request」，Run 仍 `COMPLETED` |
| P-3 | 拒绝不触发执行 | 用例 `tool.denied`：审计状态为 `DENIED`，`durationMs` 为个位数毫秒级（未触网），无 HTTP 连接产生 |
| P-4 | 每次调用都有审计记录 | 用例 `tool.audit`：审计条数 == `tool_call_started` 条数，`toolCallId` 一一对应；挂起中的调用以 `PENDING` 存在 |
| P-5 | 可按 Run 查询 | 用例 `tool.audit`：`GET api/tool/audit/list` 按 `runId` 返回该 Run 的记录，按时间升序；未知 `runId` 返回 `[]` |
| P-6 | 参数脱敏 | 用例 `tool.audit`：审计 `arguments` 里命中的键值为 `***`，明文不出现在任何响应与事件里（写出侧检测，不在事后扫描日志） |
| P-7 | 配置校验 | 用例 `tool.config`：非法 `[tools]` 配置启动失败且文案点名字段；`[tools]` 里不存在「确认超时」字段（静态核对） |

## 8. Confirmation（F-1 ~ F-7）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| F-1 | 确认前不执行 | 用例 `tool.confirm.approve`：`confirm` 工具的 `tool.call` 在确认前不被调用（假工具 panic 兜底）；审计记录为 `PENDING`、无结果 |
| F-2 | 挂起回 `QUEUED` 且不占位 | 用例 `tool.confirm.approve`：`run/get` 为 `QUEUED` 且 `pendingToolCallId` 非空；`metrics` 快照 `running_now` 不含它；无后台任务残留 |
| F-3 | 不阻塞同会话后续 Run | 用例 `tool.confirm.approve`：挂起期间同一会话的第二个 Run 能执行到 `COMPLETED` |
| F-4 | 批准后重新排队并执行 | 用例 `tool.confirm.approve`：`client/post { TOOL_CONFIRM, APPROVE }` 后 Run 重新执行并 `COMPLETED`，审计转为 `OK`，不重跑已完成的轮次（Mock 请求次数可核） |
| F-5 | 驳回回灌可读文本 | 用例 `tool.confirm.reject`：Run `COMPLETED`，模型收到「用户拒绝调用该工具」，审计为 `DENIED` |
| F-6 | 重连补发 | 用例 `tool.confirm.reconnect`：断连重连后收到同 `toolCallId` 的再次 `tool_call_confirm_required`，再次确认成功 |
| F-7 | 挂起中断连的收尾 | 用例 `tool.confirm.disconnect`：Run 落 `CANCELLED`、`pendingToolCallId` 清空、`PENDING` 审计收尾为 `CANCELLED`；重复确认返回固定 400 |

## 9. Builtin Tools（T-1 ~ T-8）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| T-1 | `calculator` 正确且安全 | 用例 `tool.builtin.calculator`：四则与括号正确；除零、语法错误、超长表达式返回固定文案不 panic |
| T-2 | `current_time` 口径固定 | 用例 `tool.builtin.current_time`：输出含 ISO8601 与 Unix 毫秒；非法 `zone` 返回 `InvalidArguments` |
| T-3 | `file_read` 白名单生效 | 用例 `tool.builtin.file`：白名单外路径被拒绝且错误文本不含真实绝对路径；超 `max_bytes` 不读入；二进制被拒绝 |
| T-4 | `file_search` 遍历期剪枝 | 用例 `tool.builtin.file`：排除目录内的文件不出现在结果里；结果条数与顺序稳定 |
| T-5 | `http_request` 默认拒绝 | 用例 `tool.builtin.http`：默认配置下调用被拒绝；显式允许后可访问白名单 host，白名单外被拒绝且无连接 |
| T-6 | 不跟随重定向 | 用例 `tool.builtin.http`：3xx 原样返回状态码（假服务端返回 302 指向白名单外地址） |
| T-7 | 结果截断 | 用例 `tool.truncate`：`max_result_chars` 调小后结果被截断并带「结果已截断」提示；截断发生在字符边界 |
| T-8 | 文件操作声明与实现一致 | 用例 `tool.decl`：五个内置工具的 `fileAccess` 为 `None` / `None` / `Read` / `Read` / `None`，`default_concurrency()` 四条映射逐项正确；**人工**：逐个核对工具实现确实只做声明范围内的文件操作，并确认没有工具声明 `Write` / `ReadWrite` 却实际写文件 |

## 10. Client（W-1 ~ W-4）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| W-1 | 事件可达客户端 | 用例 `tool.events`：客户端在 `api/client/stream` 上收到三个工具事件且与终态事件顺序一致 |
| W-2 | TUI 展示与确认 | 用例 `tool.tui`：三个事件转成对应 `Update`；无 pending 时 `y` / `n` 不产生上行请求；`cargo clippy -p lib-bin-tui --all-targets -- -D warnings` 通过 |
| W-3 | WebUI 展示与确认 | `make ui-check` 退出码 0；`useChat.ts` 三个分支有类型收窄、无 `any`；确认按钮不可重复点击（静态核对） |
| W-4 | 确认上行同源 | 静态核对：TUI 与 WebUI 都只经生成的 SDK 方法调用 `api/client/post`，不手写 HTTP；请求体字段来自 `ClientPostPO` |

## 11. Regression（R-1 ~ R-6）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| R-1 | 无工具时行为零变化 | 用例 `chat.multiturn` / `chat.context` / `run.cancel` 全绿；Mock `last-request` 中无 `tools` 键（与 P2 请求体一致） |
| R-2 | 接口变更收敛 | 静态核对：P2 的 25 条接口中只有 1 条被改名（`client/update` → `client/post`）、1 条新增（`tool/audit/list`）；其余路径与 DTO 字段只增不改；接口总数 26（不含 debug + `metrics` 限定的 `api/probe/metrics`） |
| R-3 | 状态机与终态事务不变 | 用例 `run.cancel` / `run.failed_*` / `persist.*` 全绿；成功 Run 的最后一条消息必是助手消息，失败 / 取消 Run 必无最终助手消息（中间轮次消息允许存在，见 D-P3-024） |
| R-4 | 全量回归 | `make test ARGS="--all"` 退出码 0 |
| R-5 | 发布构建可用 | `cargo build --release -p bin-agent` 成功；release 产物不带 `api/probe/metrics`，`http_request` 默认仍为 deny |
| R-6 | 旧数据可读 | 静态核对 + 用例 `persist.migration`：`0001` 时期的数据（无工具列 / 无挂起列）迁移后可读，`Message` 新字段为 `null` |

---

## 判据与验收方向的对应

| README §六 | 判据 |
|---|---|
| 1 自主选择工具并继续推理 | L-1、L-6、S-4 |
| 2 三种失败都被回灌、不卡死 | L-2、L-3、L-4、L-5、L-8 |
| 3 并行且顺序稳定可复现 | C-1、C-2、C-3 |
| 4 取消能中断工具、无残留 | C-4、C-5 |
| 5 每次调用有审计、可按 Run 查 | P-4、P-5、P-6 |
| 6 未授权的工具不会被调用 | P-1、P-2、P-3、T-5 |
| 7 需确认的工具在确认前不执行 | F-1 ~ F-7 |

配套结构性判据：A-1 ~ A-6（依赖与边界）、S-1 ~ S-8（契约）、X-1 ~ X-3（裁剪）、T-1 ~ T-8（内置工具）、W-1 ~ W-4（客户端）、R-1 ~ R-6（回归）。
