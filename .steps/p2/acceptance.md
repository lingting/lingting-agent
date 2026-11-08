# P2 验收清单

**状态：未开工。** 每条都要能实际验证；**任一条不通过，P2 不视为完成**。

验证方式只有四类：

- **命令**：可直接执行的命令与期望退出码 / 输出特征。
- **用例**：`make test ARGS="<前缀>"` 下的 `crates/bin-tests` 用例（用例名即证据）。
- **静态核对**：`cargo tree`、源码检索、`git diff` 限定到本次改动文件、逐字段比对表。
- **人工**：只能人工判定的项，必须写明判定人与判定动作。

除「用例」类外，不得以「跑一遍看看」作为证据；**未获明确授权不新增单元测试**（`AGENTS.md` 第 28 条只允许 `bin-tests`）。

---

## 1. Build（B-1 ~ B-6）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| B-1 | workspace 全量可编译 | `cargo build --workspace` 退出码 0，无 error |
| B-2 | 格式一致 | `cargo fmt --all --check` 退出码 0 |
| B-3 | Clippy 零告警 | `cargo clippy --workspace --all-targets -- -D warnings` 退出码 0 |
| B-4 | SDK 生成门禁 | `make gen` 与 `make gen-check` 均退出码 0（清空重建 + 产物完整 + 生成代码参与编译） |
| B-5 | 前端类型检查 | `make ui-check`（`tsc --noEmit`）退出码 0 |
| B-6 | Release 可分发 | `cargo build --release -p bin-agent` 成功；产物在空目录可 `server` 启动（沿用 P1-020 X-1/X-3 方法）。体积与冷启动时间只记录、不设阈值 |

## 2. Architecture（A-1 ~ A-6）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| A-1 | `agent-core` 是最底层领域 crate | `cargo tree -p agent-core` 只有 `lib-core` + `serde`/`specta`/`strum`/`framework-proc-*`；无 tokio / rig / axum / sqlx |
| A-2 | `agent-context-core` 只定义抽象 | `cargo tree -p agent-context-core` 只有 `agent-core` + `lib-core`；源码无 `async fn` |
| A-3 | `agent-context` 不含执行逻辑 | `cargo tree -p agent-context` 无 `agent-runtime` / `agent-route` / `lib-web*` / `bin-*` |
| A-4 | Rig 只在 `agent-route` | `cargo tree -i rig-core` 结果中只出现 `agent-route`；`grep -r "rig" crates --include=*.rs` 只命中 `crates/agent-route/src/rig/` |
| A-5 | 依赖方向成立 | `cargo tree` 无循环；`agent-runtime` 不依赖 `lib-web*` / `lib-bin-*` / `bin-*`；`service-core` 不依赖 `agent-runtime` / `agent-route` / `agent-context*` |
| A-6 | 未创建被禁止的 Crate | 仓库中不存在 `manager-core` / `types-api` / `bin-tui` / `bin-web` / `bin-mock-server` / `agent-tool` / `agent-mcp` / `agent-skill`（目录检索为空） |

## 3. Domain Model（D-1 ~ D-5）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| D-1 | 契约类型名与字段与 P1 一致 | 逐项比对表：`Message` / `MessageRole` / `Run` / `RunStatus` / `RunEvent` / `RunEventKind` / `SessionSummary` / `MessageSendVO` / 6 个 `*PO`·`*Params`（名称、字段、顺序、`Option`、类型） |
| D-2 | serde wire 行为一致 | 生成物比对（P2-008 记录）：camelCase 字段名、`i64` 走字符串、枚举 `SCREAMING_SNAKE_CASE`、`RunEventKind` 为 snake_case |
| D-3 | 状态机规则落地 | 源码核对 `RunStatus::can_transition_to` 与 `decisions.md` D-P2-006 的转换表一致；非法转换返回 `AgentError::InvalidTransition` |
| D-4 | 终态不可再转 | 用例 `run.cancel`（重复取消返回 400，文案与 P1 一致）+ `persist.running_heal`（终态不被自愈逻辑覆盖） |
| D-5 | 类型唯一来源 | `grep -rn "types_core::session" crates` 为空；`crates/types-core` 不含 session 类型 |

## 4. SQLite（S-1 ~ S-7）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| S-1 | 数据库位置正确 | `<AGENT_HOME>/data/agent.db` 存在（用例与人工各一次） |
| S-2 | 迁移可重复执行 | 用例 `persist.migration` |
| S-3 | 重启后数据完整 | 用例 `persist.session` |
| S-4 | 约束真实生效 | `0001_init.sql` 含主键 / 外键 / `CHECK` / `UNIQUE(session_id, seq)`；启动日志无约束错误 |
| S-5 | 消息顺序稳定 | 用例 `persist.session` 与 `chat.multiturn` 断言 `seq` 升序且与内容一一对应 |
| S-6 | 事务一致性 | 成功 Run 必有 1 条助手消息；失败 / 取消 Run 必无助手消息（`stream.failed_*` 与 `run.cancel` 断言） |
| S-7 | 并发写入不撞锁 | 用例 `run.concurrency` / `run.queue_limit` 运行期日志中无 `database is locked` / `SQLITE_BUSY` |

## 5. Provider（P-1 ~ P-7）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| P-1 | 真实 HTTP 调用 | 用例 `chat.multiturn`（服务端通过真实 HTTP 访问 mock server） |
| P-2 | 配置生效 | 环境变量注入（`AGENT_PROVIDER_BASE_URL` 等）后用例通过；`config.toml` 同名项可覆盖默认值 |
| P-3 | 流式增量 | 用例 `chat.multiturn` 断言「拼接所有 `message_delta` == 完整回复」且 `message_delta` 条数 > 1 |
| P-4 | usage 落库 | 用例 `chat.multiturn` 断言 `run/get` 的 `inputTokens` / `outputTokens` 有值且 > 0 |
| P-5 | 错误映射 | 用例 `stream.failed_auth` / `stream.failed_upstream` |
| P-6 | 空闲超时 | 用例 `stream.failed_timeout`（`AGENT_LLM_IDLE_TIMEOUT_MS` + `stall` 场景） |
| P-7 | 无第三方类型泄漏 | A-4 复用；`grep -rn "rig" crates/agent-route/src --include=*.rs` 只命中 `rig/` 子目录 |

## 6. Context（C-1 ~ C-6）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| C-1 | 历史按序进入请求 | 用例 `chat.multiturn`，断言 mock 侧 `last-request` 的 messages 顺序 |
| C-2 | System Prompt 在最前 | 同上（首条 message 为 system） |
| C-3 | 超预算被裁剪 | 用例 `chat.truncate` |
| C-4 | 最新用户消息不被裁 | `chat.truncate` 断言最后一条为本次输入 |
| C-5 | 会话之间隔离 | 用例 `run.concurrency`（两会话请求体互不含对方内容） |
| C-6 | 估算器可替换 | 源码核对：`TokenEstimator` 为 trait，构造点只有一处，`HeuristicEstimator` 是默认实现 |

## 7. Agent Loop（L-1 ~ L-6）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| L-1 | 一个 Run 一个执行任务 | 源码核对 `RunRegistry::insert_once` + 用例 `run.cancel`（重复取消不产生第二个执行） |
| L-2 | 状态推进正确 | 用例 `chat.multiturn`：`run_started`（RUNNING）→ deltas → `run_completed`（COMPLETED），`run/get` 终态一致 |
| L-3 | 单生产方 | 用例：同一 Run 重复订阅返回 400（沿用 P1 `session.message` 的负向断言方式，新增一条显式断言） |
| L-4 | 终态与消息同事务 | S-6 复用 |
| L-5 | 异常不留 `RUNNING` | 用例 `persist.running_heal`；服务端日志无 panic 行 |
| L-6 | 未实现 Tool Calling | 仓库无 `agent-tool`；无 `tool_call_*` 事件；代码中无 tool loop |

## 8. Streaming（T-1 ~ T-6）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| T-1 | 帧格式不变 | 用例 `session.message`（沿用，断言 `event:` 行与 `data` 同值、空行分隔） |
| T-2 | `seq` 从 0 连续 | 各 `chat.*` / `run.*` 用例断言严格递增且以 0 起始 |
| T-3 | 心跳可用 | 用例 `stream.heartbeat` |
| T-4 | `run_failed` 可用 | 用例 `stream.failed_*`（4 条） |
| T-5 | `server_shutdown` 不变 | 用例 `session.stream_terminal`（沿用）+ `run.shutdown` |
| T-6 | 无空帧 / 空 delta | 用例断言不存在 `delta` 为空的 `message_delta`；`RunEvent` 字段集合未变（D-1 复用） |

## 9. Cancellation（N-1 ~ N-5）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| N-1 | 取消落终态 | 用例 `run.cancel`：`CANCELLED` + `finishedAtMs` 有值 + 无助手消息 |
| N-2 | 取消真正中断模型调用 | `run.cancel` 读 mock `/mock/stats`：连接关闭计数 +1 |
| N-3 | 断线等价取消 | 用例 `run.disconnect`：800ms 内 `CANCELLED`，mock 侧连接关闭 |
| N-4 | 关闭联动 | 用例 `run.shutdown`：末条为 `server_shutdown`（`status=CANCELLED` + 可读 `error`） |
| N-5 | 竞态只有一方获胜 | 用例 `run.cancel` 的重复取消断言 + 源码核对条件更新（`WHERE status IN (...)`） |

## 10. Concurrency（R-1 ~ R-4）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| R-1 | 多会话并发不串扰 | 用例 `run.concurrency` |
| R-2 | 同会话串行 | 用例 `run.session_serial`（第二个 Run 在第一个终态前保持 `QUEUED`） |
| R-3 | 全局上限生效 | 用例 `run.queue_limit`（`AGENT_MAX_CONCURRENT_RUNS=1`，采样 `runningRunCount ≤ 1`） |
| R-4 | 排队超时可配 | 源码核对 `run_queue_timeout_ms` 的读取与超时落 `FAILED` 的分支；用例可选（若实现则加入 `run.queue_limit`） |

## 11. Recovery（V-1 ~ V-4）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| V-1 | 重启后历史完整 | 用例 `persist.session` |
| V-2 | `RUNNING` 残留自愈 | 用例 `persist.running_heal`（`FAILED` + 「服务重启，执行已中断」） |
| V-3 | `QUEUED` 残留自愈 | 用例 `persist.queued_heal`（`CANCELLED` + `finishedAtMs`） |
| V-4 | 数据库不损坏 | 用例 `persist.migration` + 重启后可继续写入（`persist.resume_chat`） |

## 12. API Compatibility（K-1 ~ K-6）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| K-1 | 19 条接口 method / path 不变 | 生成物比对：`packages/sdk-rust/src/api.rs` 与 `packages/sdk-ts` 的接口清单与 P1-020 记录一致 |
| K-2 | 会话 7 条接口出入参字段不变 | 逐字段比对表（沿用 P1-020 A-5 方法） |
| K-3 | 错误契约不变 | `{code, message}` 形状不变；新增的「未知模型」返回 400 且保持同形状（用例 `session.error` 扩展断言） |
| K-4 | `RunEventKind` 6 值不变 | 生成物枚举取值比对；无新增枚举值 |
| K-5 | 客户端只改展示层 | `git diff` 限定 `lib-bin-cli` / `lib-bin-tui` / `lingting-agent-ui`：只允许失败态、排队态、错误文案相关改动，无 Transport / SDK 调用重写 |
| K-6 | 生成物语义零差异 | P2-008 与 P2-035 两次比对记录：DTO 名称 / 字段 / 类型 / 可选性 / 枚举取值零差异 |

## 13. Security（X-1 ~ X-5）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| X-1 | 密钥不入日志 | 在 `AGENT_PROVIDER_API_KEY=<canary>` 下跑 `chat.multiturn` 与 `stream.failed_auth`，检索服务端日志文件与 `server.log` 无 canary |
| X-2 | 密钥不入响应 | 检索失败响应体与 `run/get`、`message/list`、`probe/health` 的输出无 canary |
| X-3 | `Debug` 脱敏 | 源码核对 `Secret` 的 `Debug`/`Display` 为 `***`；无 `#[derive(Debug)]` 直接暴露 key 的结构 |
| X-4 | 错误不含内部细节 | `stream.failed_*` 断言 `error` 不含 `http`、`/v1`、盘符路径、`SELECT`、堆栈关键字 |
| X-5 | 未新增配置接口 | 接口总数仍为 19（`make gen` 输出「接口 19 条」） |

## 14. End-to-End（E-1 ~ E-8）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| E-1 | 完整对话闭环 | 用例 `chat.multiturn`（会话 → 发消息 → 流式 → 终态 → 消息列表） |
| E-2 | 多轮历史生效 | 用例 `chat.multiturn`（3 轮，断言请求体） |
| E-3 | 失败路径端到端 | 用例 `stream.failed_auth` |
| E-4 | 取消路径端到端 | 用例 `run.cancel` |
| E-5 | 断线路径端到端 | 用例 `run.disconnect` |
| E-6 | 服务关闭路径端到端 | 用例 `run.shutdown` / `session.stream_terminal` |
| E-7 | 重启恢复路径端到端 | 用例 `persist.running_heal` / `persist.resume_chat` |
| E-8 | 完整门禁 | `make test ARGS="--all"`（全部用例）、`cd lingting-agent-ui && pnpm exec vp test run`、`pnpm exec playwright test` 全通过；`make ci` 若因 Framework `path` 依赖失败，记录失败步骤与原因，不作为阻塞条件 |

---

## 交付判据

1. 上述 14 类条目全部有结论与证据，结论写入 `docs/reports/p2-035-acceptance.md`。
2. 偏差与遗留必须逐条写「处置 + 归属阶段」，不允许出现无归属项。
3. `make ci` 的 CI 失败原因单独记录，且明确「与 P2 功能无关、P6 统一处理」。
