# P2 端到端验收报告（P2-032）

- **阶段**：P2 基础 Agent Runtime
- **验收日期**：2026-11-21
- **判据来源**：`.steps/p2/acceptance.md`（15 类共 92 条）、`.steps/p2/tasks.md`（P2-031 / P2-032）
- **结论**：**15 类判据全部通过**（3 条判据表述按实现口径修订，见第六节）。`tasks.md` 的 P2-001 ~ P2-032 全部完成。
- **用例执行**：`make test ARGS="--all"`，73 个用例全部通过（含 `web.ui` 调度 `pnpm exec vp test run` 的 31 个前端单元用例、`web.e2e` 调度 `pnpm exec playwright test` 的 4 个浏览器端到端用例）。
- **用例产物**：`target/tests/run-1795266209158-12032/`（`README.md` 为汇总与索引，按领域分文件：`chat.md` / `run.md` / `session.md` / `persist.md` / `llm.md` / `web.md` / `tui.md` / `runtime.md` / `sdk.md` / `probe.md` / `client.md`）。
- **门禁**：`make ci`（`gen-check` + `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings` + `tsc --noEmit`）退出码 0。
- **人工项**：本阶段 92 条判据均有命令、用例或静态核对证据；TUI 真实终端观感沿用 P1-015 的人工结论（见第六节第 9 条）。

---

## 一、Build（B-1 ~ B-6）

| # | 结论 | 证据 |
|---|---|---|
| B-1 | 通过 | `cargo build --workspace` 退出码 0，无 error |
| B-2 | 通过 | `cargo fmt --all --check` 退出码 0 |
| B-3 | 通过 | `cargo clippy --workspace --all-targets -- -D warnings` 退出码 0 |
| B-4 | 通过 | `make gen` 两次产出「内容无变化，未写盘」；`make gen-check` 输出「通过：两份 SDK 可从零重建，产物完整且可编译」 |
| B-5 | 通过 | `make ui-check`（`pnpm exec tsc --noEmit`）退出码 0 |
| B-6 | 通过 | `make release` 成功（3m36s，含前端 `pnpm build` 与产物嵌入）；`target/release/bin-agent.exe` 17,375,232 字节（16.6 MiB）；在空目录、空 `AGENT_HOME` 下启动 `server` 子命令，进程存活并生成 `data/agent.db` 与 `logs/` |

## 二、Architecture（A-1 ~ A-8）

| # | 结论 | 证据 |
|---|---|---|
| A-1 | 通过 | `cargo tree -p agent-core -e normal --depth 1`：`framework-proc-auto` / `framework-proc-core` / `lib-core` / `serde` / `serde_with` / `specta` / `strum` / `strum_macros`，无 tokio / rig / axum / sqlx |
| A-2 | 通过 | `cargo tree -p agent-context-core` 只有 `agent-core` + `lib-core`；`grep -rn "async fn\|tokio\|std::fs" crates/agent-context-core/src` 无命中 |
| A-3 | 通过 | `cargo tree -p agent-context`：`agent-context-core` + `agent-core` + `tracing`，无 `agent-runtime` / `agent-route` / `lib-web*` / `bin-*` |
| A-4 | 通过（口径修订） | `cargo tree -i rig-core` 只命中 `agent-route`（仅它直接依赖）；`rig-core` 的使用全在 `crates/agent-route/src/rig/` |
| A-5 | 通过 | `cargo tree` 无循环；`agent-runtime` 不含 `lib-web*` / `lib-bin-*` / `bin-*`；`service-core` 不含 `agent-runtime` / `agent-route` / `agent-context*` |
| A-6 | 通过 | 端口 trait `RunDriver` 定义在 `crates/service-core/src/session/driver.rs:104`；实现只在 `crates/agent-runtime/src/port.rs`；注册点是 `crates/lib-bin-web/src/server.rs` 的 `install_driver`；`grep -rn agent_runtime crates/service-core` 无命中 |
| A-7 | 通过 | `ls crates` 只有 17 个已批准成员，不存在 `manager-core` / `types-api` / `bin-tui` / `bin-web` / `bin-mock-server` / `agent-tool` / `agent-mcp` / `agent-skill` |
| A-8 | 通过 | `cargo tree -p lib-db -e normal --depth 1`：`lib-core` + `sqlx` + `tokio`；源码仅 `migrate::run` 等函数名命中 `run`，无 `sessions` / `messages` / `runs` 业务表名 |

## 三、Domain Model（D-1 ~ D-5）

| # | 结论 | 证据 |
|---|---|---|
| D-1 | 通过 | `make gen` 两次「内容无变化，未写盘」：Rust / TS 两侧 DTO 与 P1.1 记录一致；`grep -rn "types_core::session" crates` 无命中，`types-core::runtime` 仍在原处 |
| D-2 | 通过 | 生成物比对（D-P2-008 记录）：camelCase、`i64` 走字符串、默认枚举 `SCREAMING_SNAKE_CASE`、`RunEventKind` / `ClientEventKind` 为 snake_case，本次生成零差异 |
| D-3 | 通过 | `crates/agent-core/src/session/run.rs` 的 `RunStatus::can_transition_to` 与 D-P2-004 转换表一致；`transition_to` 对非法转换返回 `AgentError::InvalidTransition` |
| D-4 | 通过 | `run.cancel`（重复取消 400，文案与 P1 一致）+ `persist.running_heal`（第二次重启后 `Failed`、原因与 `finishedAtMs` 均不被改写） |
| D-5 | 通过 | `grep -rn "types_core::session" crates` 无命中；`crates/types-core` 不含 session 类型 |

## 四、SQLite（S-1 ~ S-8）

| # | 结论 | 证据 |
|---|---|---|
| S-1 | 通过（口径修订） | release 二进制在空 `AGENT_HOME` 下启动后生成 `data/agent.db`；事件文件路径为 `<AGENT_HOME>/data/events/<sessionId>/<runtimeSessionId>.jsonl`（`flow::journal_path` 与 `runtime.journal` 用例断言），与 `agent.db` 同级 |
| S-2 | 通过 | `persist.migration`：同一 data 目录连续启动两次，第二次成功、数据保留、日志无错误 |
| S-3 | 通过 | `persist.session`：重启后 2 条消息的 id / 内容 / 角色逐条一致，会话摘要 `messageCount = 2` |
| S-4 | 通过 | `crates/lib-db/migrations/0001_init.sql` 含三张表主键、`REFERENCES ... ON DELETE CASCADE`、`UNIQUE(session_id, seq)`，**无 `CHECK`**（D-P2-030）；`persist.migration` 第二次启动日志无约束错误 |
| S-5 | 通过 | `persist.session`（重启后顺序与内容一致）+ `chat.multiturn`（6 条消息用户/助手交替，历史按 `seq` 升序进入请求体） |
| S-6 | 通过 | `chat.multiturn` 成功 Run 写 1 条助手消息；`run.failed_auth` / `run.failed_upstream` / `run.failed_timeout` / `run.cancel` 均断言消息列表只有 1 条用户消息 |
| S-7 | 通过 | `run.concurrency`（`trace` 级）：服务端日志 809 行，无 `database is locked` / `SQLITE_BUSY` / `panic` |
| S-8 | 通过 | `grep "SELECT\|INSERT\|UPDATE" crates/service-core/src` 只命中 `repository/{session,message,run}.rs` 与 `repository/mod.rs` 的文档注释 |

## 五、Provider（P-1 ~ P-7）

| # | 结论 | 证据 |
|---|---|---|
| P-1 | 通过 | `chat.multiturn`：服务端经真实 HTTP 访问 Mock LLM（服务端 `[provider] base_url` 指向 Mock），3 轮对话完成 |
| P-2 | 通过 | 各 SelfServed 用例把 `config.toml` 写进隔离 `AGENT_HOME` 并指向 Mock；改配置即刻生效（如 `max_concurrent_runs = 1` 使 `run.concurrency` 观测到 RUNNING 最大值为 1、`run_queue_timeout_ms = 1000` 使 `run.queue_timeout` 触发超时） |
| P-3 | 通过 | `chat.multiturn` 断言「拼接所有 `message_delta` == 完整回复」且条数 > 1（用例源码断言）；`llm.mock` 侧记录 6 帧拼接一致 |
| P-4 | 通过 | `chat.multiturn`：最后一次 Run 终态 `Completed` 且 `inputTokens` / `outputTokens` 有值且 > 0；`session.message` 记录输出 token = 8 |
| P-5 | 通过 | `run.failed_auth`（`调用模型失败：认证失败`）、`run.failed_upstream`（`上游服务错误`）、`run.failed_timeout`（`响应超时`） |
| P-6 | 通过 | `session.error`：未知模型 400 `模型不可用：no-such-model-...` |
| P-7 | 通过（口径修订） | A-4 复用；`rig-core` 的使用只在 `crates/agent-route/src/rig/{openai,stream,error,mod}.rs` |

## 六、Context（X-1 ~ X-6）

| # | 结论 | 证据 |
|---|---|---|
| X-1 | 通过 | `chat.context`：请求体 6 条消息，历史按 `seq` 升序居中 |
| X-2 | 通过 | `chat.context`：首条为 system（系统提示词取自 `AGENT_HOME/AGENTS.md`） |
| X-3 | 通过 | `chat.truncate`：`context_max_messages = 2` 时请求体只留 3 条，最旧历史被裁掉 |
| X-4 | 通过 | `chat.truncate`：末条为本次输入 |
| X-5 | 通过 | `session.isolation`：两个会话消息互不可见、统计各为 1；`run.concurrency` 两个会话各自跑完、请求体互不串扰 |
| X-6 | 通过 | `TokenEstimator` 是 trait（`agent-context-core/src/policy.rs:45`），`HeuristicEstimator` 的唯一构造点在 `agent-context/src/builder.rs` 的 `ContextBuilder::new`，换估算器走 `with_estimator` |

## 七、Agent Loop（L-1 ~ L-6）

| # | 结论 | 证据 |
|---|---|---|
| L-1 | 通过 | 执行由会话执行队列串行驱动（`manager-agent/src/runner/queue.rs`，一条 `mpsc` 串行任务）；`run.cancel` 重复取消返回 400，不产生第二次执行 |
| L-2 | 通过 | `chat.multiturn`：`run_started`（RUNNING）→ `message_delta` → `run_completed`（COMPLETED），`run/get` 终态一致 |
| L-3 | 通过 | 事件只经 `mpsc::Sender<RunEvent>` 产出（`agent-runtime/src/feed.rs`）；`grep "run_event\|journal_path\|jsonl" crates/agent-runtime/src` 无命中（事件文件只由 `service-core` / `manager-agent` 的写者打开） |
| L-4 | 通过 | S-6 复用（终态与助手消息同事务） |
| L-5 | 通过 | `persist.running_heal`：强杀后自愈成 `FAILED`，服务端日志无 panic 行 |
| L-6 | 通过 | 无 `agent-tool` crate；`grep -rn tool_call crates` 只命中 `agent-core/src/session/event.rs` 的注释；代码中无 tool loop |

## 八、Run Integration（G-1 ~ G-6）

| # | 结论 | 证据 |
|---|---|---|
| G-1 | 通过 | `session.trigger`：未占用时 Run 停在 `QUEUED` 且无事件文件；占用后同一 Run 执行完成、8 条事件落盘 |
| G-2 | 通过 | 同一用例断言「不占用会话时 Run 永不执行」，日志有原因记录 |
| G-3 | 通过 | `run.journal_consistency`：同一 Run 的下行流与事件文件各 8 条，`event` / `seq` 逐条一致，`seq` 为 0..7 |
| G-4 | 通过 | `run.execute`：未注入执行端口时 Run 落 `Failed`、原因 `执行器未装配`、事件文件以 `run_failed` 收尾；真实执行一半由 `chat.multiturn` 与其余 `run.*` 覆盖 |
| G-5 | 通过 | `run.disconnect`：丢弃下行流后 Run 落 `Cancelled`，事件文件以 `run_cancelled` + `客户端连接已断开` 收尾；`runtime.occupancy` 覆盖占用释放链路 |
| G-6 | 通过 | `persist.occupancy_reset`：重启后 `occupancy/get` 返回 404，重新注册 + 重新占用后同一会话可继续对话 |

## 九、Streaming（T-1 ~ T-5）

| # | 结论 | 证据 |
|---|---|---|
| T-1 | 通过 | `runtime.client`（建流 200 `text/event-stream`、drop 响应体即断流）、`session.message`（下行 8 条、最后序号 7）帧格式不变 |
| T-2 | 通过 | `run.journal_consistency` 断言 `RunEvent.seq` 从 0 严格递增；`runtime.client` / `runtime.stream` 断言 `ClientEvent.seq` 独立递增 |
| T-3 | 通过 | `grep -rn Heartbeat crates/agent-runtime/src` 无命中（运行时不产出心跳）；保活由客户端 `PING` 与下行 `: keepalive` 承担（`tui.heartbeat` 空闲期收到 3 次 pong） |
| T-4 | 通过 | `run.failed_auth` / `run.failed_upstream` / `run.failed_timeout` 三条 |
| T-5 | 通过 | `run.shutdown`（事件文件末条 `server_shutdown`，原因「服务正在关闭」）+ `session.stream_terminal`（流以 `client_closed(Some(ServerShutdown))` 收尾） |

## 十、Cancellation（N-1 ~ N-6）

| # | 结论 | 证据 |
|---|---|---|
| N-1 | 通过 | `run.cancel`：`Cancelled` + `finishedAtMs` 有值 + 无助手消息 + 原因「用户取消执行」 |
| N-2 | 通过 | `run.cancel` 读 Mock `/mock/stats`：连接中断计数为 1（O-1 结论：drop 模型流即关闭底层连接） |
| N-3 | 通过 | `run.disconnect`：在跑 Run 落 `Cancelled`，事件文件含 `run_cancelled`，Mock 侧中断计数 1 |
| N-4 | 通过 | `run.shutdown`：事件文件末条为 `server_shutdown`（`status=CANCELLED` + 可读 `error`） |
| N-5 | 通过 | 重复取消返回 400；`crates/service-core/src/repository/run.rs` 的终态更新均为条件更新（`WHERE status = ?` / `WHERE status IN (...)`） |
| N-6 | 通过 | `run.journal_consistency`：每个 Run 的最后一条事件必属终态集合，且只出现一次 |

## 十一、Concurrency（Q-1 ~ Q-4）

| # | 结论 | 证据 |
|---|---|---|
| Q-1 | 通过 | `run.concurrency`：两个会话并发执行互不串扰，两个 Run 都抵 `Completed` |
| Q-2 | 通过 | 同一会话第二个 Run 在第一个终态前保持 `QUEUED`（`session.trigger` + 执行队列单任务串行） |
| Q-3 | 通过（口径修订） | `run.concurrency`：`max_concurrent_runs = 1` 时采样 103 次，同时处于 `RUNNING` 的最大值为 1；`probe.health` 的 `runningRunCount` 取值可读 |
| Q-4 | 通过 | `run.queue_timeout`：`run_queue_timeout_ms = 1000` 时排队 Run 落 `Failed`「执行排队超时」，占位 Run 正常完成 |

## 十二、Recovery（V-1 ~ V-5）

| # | 结论 | 证据 |
|---|---|---|
| V-1 | 通过 | `persist.session`（重启后消息顺序与内容一致） |
| V-2 | 通过 | `persist.running_heal`：`Failed` + 「服务重启，执行中的 Run 已终止」 |
| V-3 | 通过 | `persist.queued_heal`：`Cancelled` + 「服务重启，排队中的 Run 已取消」+ `finishedAtMs` |
| V-4 | 通过 | `persist.migration`（迁移可重复、日志无错误）+ `persist.session`（重启后可继续写入） |
| V-5 | 通过 | `persist.occupancy_reset`：重启后 404，重新占用可继续 |

## 十三、API Compatibility（K-1 ~ K-7）

| # | 结论 | 证据 |
|---|---|---|
| K-1 | 通过 | `make gen` 输出「路由 25 条，接口 25 条」，接口清单与 P1.1 记录一致 |
| K-2 | 通过 | 会话 6 条接口出入参零差异（`make gen` 两次「内容无变化，未写盘」，沿用 P1-020 A-5 比对方法） |
| K-3 | 通过 | 错误契约仍为 `{code, message}`；`session.error` 的「未知模型」400 保持同形状 |
| K-4 | 通过 | `packages/sdk-rust/src/types.rs` 的 `RunEventKind` 仍为 7 值（`run_started` / `message_delta` / `run_completed` / `run_failed` / `run_cancelled` / `server_shutdown` / `heartbeat`），无新增 |
| K-5 | 通过 | `git show --stat 6b6c02c`（Batch G）：客户端改动只在 `lib-bin-tui/src/{state,ui}.rs` 与 `lingting-agent-ui/src/{api/link.ts,pages/chat/{MessageList.tsx,useChat.ts}}`，属失败 / 取消 / 排队态展示与文案，无 Transport 或 SDK 调用重写 |
| K-6 | 通过 | P2-008 与本次 `make gen` 两次比对：DTO 名称 / 字段 / 类型 / 可选性 / 枚举取值零差异 |
| K-7 | 通过 | 接口总数仍为 25（`make gen` 输出与 K-1 一致），未新增配置接口 |

## 十四、Security（Z-1 ~ Z-5）

| # | 结论 | 证据 |
|---|---|---|
| Z-1 | 通过 | `llm.canary`：带 canary 的 `config.toml` 起真实服务端（`trace` 级），跑成功一轮 + 认证失败一轮后检索 `server.log`（156 行）与 5 个日志文件（合计 312 行），无 canary |
| Z-2 | 通过 | `llm.canary`：`run/get`、`message/list`、`probe/health`、下行成功 / 失败事件共 5 处响应文本无 canary；`llm.security` 另覆盖 7 处可输出文本 |
| Z-3 | 通过 | `crates/types-core/src/config/provider.rs`：`Secret` 的 `Debug` / `Display` 均输出 `***`，取值只能走 `expose`；`llm.security` 断言 `format!("{provider:?}")` / `format!("{}", config.api_key)` 等无 canary |
| Z-4 | 通过 | `run.failed_auth` / `run.failed_upstream` / `run.failed_timeout` 的错误文案为固定脱敏文本（不含 `http` / `/v1` / 路径 / `SELECT` / 堆栈关键字） |
| Z-5 | 通过 | K-7 复用 |

## 十五、End-to-End（E-1 ~ E-8）

| # | 结论 | 证据 |
|---|---|---|
| E-1 | 通过 | `chat.multiturn`：注册 → 建流 → 占用 → 发消息 → 流式 → 终态 → 消息列表（6 条）全链路 |
| E-2 | 通过 | `chat.multiturn`：3 轮对话，最后一次请求体 5 条消息按序进入 |
| E-3 | 通过 | `run.failed_auth`：401 → `run_failed` → 数据库终态 `Failed` → 无助手消息 |
| E-4 | 通过 | `run.cancel`：取消接口 + `run_cancelled` + Mock 侧连接中断 1 |
| E-5 | 通过 | `run.disconnect`：丢流 → `run_cancelled` + 「客户端连接已断开」 |
| E-6 | 通过 | `run.shutdown` / `session.stream_terminal` / `probe.shutdown` / `web.shutdown` |
| E-7 | 通过 | `persist.running_heal` / `persist.queued_heal` / `persist.occupancy_reset` |
| E-8 | 通过 | `make test ARGS="--all"` 73 个用例全过（含 `web.ui` 的 31 个前端单元用例、`web.e2e` 的 4 个 Playwright 用例）；`make ci` 退出码 0，无 CI 失败项需要挂账 |

---

## 十六、偏差与遗留

每条都给出处置与本阶段归属，没有无归属项。

1. **A-4 / P-7 的 grep 表述过严**：`grep -rn "rig" crates --include=*.rs` 除 `crates/agent-route/src/rig/` 外还会命中 `crates/agent-route/src/lib.rs`（`pub mod rig;`）与 `crates/agent-runtime/src/port.rs`（`use agent_route::rig::OpenAiProvider;`），后两者是模块声明与端口引用，不是对 `rig-core` 的依赖。**处置**：判据改为「`cargo tree -i rig-core` 只命中 `agent-route`，`rig-core` 使用只在 `agent-route/src/rig/`」。**归属**：P2（acceptance.md 已修订）。
2. **Q-3 采样口径**：原判据写「采样 `runningRunCount ≤ 1`」，用例实际采样两个 Run 的 `run/get` 状态。**处置**：判据改为「采样两个 Run 状态，同时 RUNNING 最大值 = 1」，`probe/health` 的 `runningRunCount` 由 `probe.health` 覆盖取值可读。**归属**：P2（acceptance.md 已修订）。
3. **S-1 的事件目录懒创建**：没有任何占用时 `<AGENT_HOME>/data/events/` 不存在（首次占用时创建）。**处置**：判据改为「事件文件位于 `data/events/`，与 `agent.db` 同级，首次占用时创建」。**归属**：P2（acceptance.md 已修订）。
4. **S-7 / L-5 的日志级不够**：`run.concurrency` 与 `persist.running_heal` 原先以 `warn` 级启动服务端，日志为空无法判定锁竞争。**处置**：新增 `Server::start_with_level` / `serve_once_level` / `serve_with_level`，这两个用例改用 `trace` 级，现分别扫描 809 行与 1 行日志。**归属**：P2（已完成）。
5. **Z-1 / Z-2 缺自动化**：原判据只能人工检索日志。**处置**：新增 `llm.canary`（真实服务端 + 带 canary 的 `config.toml`，成功与失败两条路径，检索 312 行日志与 5 处响应）。**归属**：P2（已完成）。
6. **三个用例此前缺失**：`run.execute`（G-4）、`persist.running_heal`（D-4 / L-5 / V-2 / E-7）、`llm.canary`（Z-1 / Z-2）在 acceptance 中被引用但仓库里不存在。**处置**：本次在 `crates/bin-tests` 补齐。**归属**：P2（已完成）。
7. **`rig-core` / `sqlx` 成本未单独计量**：只记录总账——release 全量构建 3m36s（含前端构建与产物嵌入），产物 16.6 MiB，不设阈值。**处置**：不做特性开关对比。**归属**：P6（发布与性能度量）。
8. **CI 无失败项**：`make ci` 本次全部通过，没有需要「与 P2 功能无关、P6 统一处理」的 Framework `path` 依赖失败。**处置**：如实记录。**归属**：无。
9. **TUI 真实终端观感沿用 P1**：raw 模式、备用屏幕、退出后终端恢复未重跑（P1-015 的人工结论仍有效）。**处置**：发布前再复核一次。**归属**：P6（跨平台发布）。

## 十七、P3 输入清单

- **Tool Calling 未实现**：`RunEventKind` 仍是 7 值，P2 未加值；P3 需要 `tool_call_*` 事件时按 D-P2-004 追加枚举值即可（`role` / `status` 列存 serde wire 值、不加 `CHECK`，见 D-P2-030 与 S-4）。
- **执行入口唯一**：`service_core::session::RunDriver`（`RunInput` / `RunOutcome` / `RunStop`）是会话执行队列唯一的执行端口，实现只在 `agent-runtime`，注册点是 `lib-bin-web`。P3 的 tool loop 应扩展 `agent-runtime`，不得让工具协议进入 `agent-route`（那里只做 Provider 传输）。
- **Context 装配位置**：请求组装在 `agent-context`（`ContextBuilder` + 可替换的 `TokenEstimator`）。工具结果注入必须走同一装配，不要在 `agent-runtime` 里另拼请求体，否则 X-1 ~ X-4 的裁剪口径会分裂。
- **事件通道**：事件只经 `Sender<RunEvent>` 单生产方产出，写序固定为「先落盘 flush、再转发」。工具事件必须走该通道，不得直接写事件文件。
- **持久化**：`sessions` / `messages` / `runs` 三张表 + `UNIQUE(session_id, seq)`，消息 `seq` 在 `INSERT` 子查询里分配。工具调用记录需要新表或新列时，按 `0002_*.sql` 追加迁移（迁移账本机制见 `lib-db` 的 `migrate`）。
- **配置面**：`types-core::config` 的 `[provider]` / `[agent]` 是唯一配置结构，新增项要同时补装载校验（参考 `llm.config` 的非法配置全拒绝清单）。
- **取消链路**：`RunCancels` 按 `runId` 触发取消信号，会话停止时批量触发。工具执行的中断也要挂到这条链上，否则 N-2 的「取消真正中断上游调用」会退化成只改数据库状态。
- **用例基建可直接复用**：`bin-tests` 的 `Server::start_with_level` / `serve_once_level`、`MockServer`（`mock:<场景>` 关键字 + `/mock/stats` / `/mock/last-request`）已能表达慢速流、认证失败、上游 5xx、静默超时、中途断连，P3 的工具场景可在此基础上加场景值。
- **接口计数仍在下降通道**：本次验收接口总数 25、服务端推送只有 `api/client/stream` 一条；P3 若新增工具查询接口，需同步 `docs/api/*.md` 与两份 SDK 的生成物。
