# P6A-020 阶段验收与报告（可靠性：Checkpoint / Resume / Run Persistence / Task Recovery）

- **子阶段**：6A 可靠性（20 个任务 / 5 个批次）
- **任务**：P6A-020（取证覆盖 Batch A ~ E 的全部已落地实现、`resume.*` 用例域 12 条、门禁与生成物）
- **前置**：Batch A ~ D 已在前四个提交落地（`6af692b` ~ `be94cd9`）；Batch E（P6A-017 ~ P6A-020）在本轮结案
- **执行日期**：2026-11-28
- **环境**：Windows 11 / Git Bash；`bin-agent` 以 `metrics` 特性构建；用例并发度取默认 `available_parallelism()`
- **判据来源**：[`.steps/p6/6a/acceptance.md`](../../.steps/p6/6a/acceptance.md)（10 组 59 条）；任务清单见 [`tasks.md`](../../.steps/p6/6a/tasks.md)，决策见 [`decisions.md`](../../.steps/p6/6a/decisions.md)
- **测试授权**：本轮在**明确授权**下执行用例与门禁（需求方确认「授权：用例 + 门禁都跑」）。最终代码的定向执行：`resume`（12/12，`run-1795771595433-34504`）、`llm.config`（1/1，`run-1795771244801-29484`）、`skill`（15/15，`run-1795771247486-42336`）、`tool`（28/28，`run-1795771255790-37816`）、`persist`（5/5，`run-1795771266366-16600`）、`run`（10/10，`run-1795771270190-26988`）、`runtime`（18/18，`run-1795771281618-32376`）、`resume` 复跑（12/12，`run-1795772169277-44416`）；最终代码的全量 `make test ARGS="--all"` → **150/150 通过**（`run-1795772185789-27720`）。数字全部来自用例报告与 `metrics` 快照，**不读日志**。
- **复核轮（2026-11-28）**：三处补正后在授权下复跑——`resume`（12/12，`run-1795776896689-29020`）、`persist`（5/5，`run-1795776913883-38792`）、`run`（10/10，`run-1795776916389-41664`）、`tool`（28/28，`run-1795776926497-36840`）、`runtime`（18/18，`run-1795776936914-11608`）；全量 `make test ARGS="--all"` → **150/150 通过**（`run-1795776947333-39336`）。门禁 `make check` / `make gen-check` / `make ui-check` 退出码均为 0，`make gen` 仍是路由 / 接口各 **33** 条。
- **本轮改动面**：`types-core`（两个新配置项与两个上限）、`lib-bin-cli`（装载校验）、`agent-runtime`（失败分类 + Run 级重试 + 重试计数）、`service-core`（载荷字节计数）、`manager-agent` / `lib-bin-web`（快照转写）、`bin-tests`（新增 `resume.retry`、写放大与清理复算断言、配置校验断言）。**没有新增接口、没有新增 Crate、没有新增事件类型。**

---

## 一、结论

1. **十组判据逐条有结论**：`B` / `A` / `S` / `C` / `R` / `P` / `I` / `O` / `E` / `K` 共 **59 条**（复核轮新增 `S-4`「终态即裁剪检查点」），除 `O-3` 外**全部成立**；`O-3` 按「未采集」显式记录（无特性构建的 `metrics` 快照），另有 8 处「判据原文与实现的偏差」在 §6.2 逐条说明，**没有「未授权」项**。逐条证据见 §二 ~ §五，「未采集」与结转风险见 §六，复核轮的三处代码补正在 §八。
2. **失败有分类、重试有上限且可复算**：`resume.retry` 证明上游 503（可重试分类）按 `run_max_retries = 2` 被重发 **2 次**（Mock 侧 `scenarios.upstream` 计数 = 3 = 1 次失败 + 2 次重试），用尽后落固定文案「模型调用重试次数已用尽」；认证失败（不可重试分类）**只请求上游 1 次**、文案仍是原始脱敏文案、重试计数不变。`limit.retries` 快照值 = 2，等于配置上限。
3. **缺省不重试是硬保证**：`run_max_retries` 缺省 `0`，`run.failed_upstream` / `run.failed_auth` / `run.failed_timeout` 三条既有用例（10/10 全绿）断言的仍是各自的原始脱敏文案——说明「不重试」路径与改动前逐字段一致。
4. **写放大是数字而不是感觉**：两轮工具 + 正常完成的 Run 有 **5 条**检查点（起点 + 每轮模型边界 + 每轮工具边界），载荷共 **428 字节**、平均 **85 字节/条**，单条上界断言 **2048 字节**（复核轮加入起点检查点的 `assistant_message_id` 后由 272 / 54 变为这两个值）；`resume.checkpoint` 的存储半段另外断言 25 条写入 → 落终态时在同一事务里删除 5 条 → 保留 20 条。
5. **检查点写入没有引入新的阻塞模式**：写入与指针更新在同一个事务里、走既有连接池路径（`Repository::begin`），全仓 `checkpoint` 相关代码没有 `spawn_blocking`、没有 `block_on`、没有新线程；`O-2` 的「落库次数」用「事件帧 + 检查点写入」两项快照复算（`resume.metrics` 读出 16）。
6. **契约漂移面是可枚举的**：生成物相对 P5 基线只多一项——`api/run/history` 一个只读接口（路由 / 接口各 33 条，`make gen` 输出「路由 33 条，接口 33 条」）；`RunStatus` 的 `PAUSED` 与 `Run` 的三个新字段是 Batch A / D 已落地的产物；`api/run/resume` 与 `resume_on_occupy` 全仓检索命中 **0**。

---

## 二、门禁与结构（B / A 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| B-1 | workspace 全量可编译 | 成立 | `cargo build --workspace` 退出码 0（`Finished dev profile`） |
| B-2 | 格式一致 | 成立 | `cargo fmt --all --check` 退出码 0 |
| B-3 | Clippy 零告警 | 成立 | `cargo clippy --workspace --all-targets -- -D warnings` → `No issues found` |
| B-4 | 生成物与前端门禁 | 成立 | `make gen`（**路由 33 条 / 接口 33 条**，两份 SDK「内容无变化，未写盘」）、`make gen-check`（「两份 SDK 可从零重建，产物完整且可编译」）、`make ui-check`（`pnpm exec tsc --noEmit`）退出码均为 0；`make check`（fmt + clippy）退出码 0 |
| A-1 | 检查点类型不进协议 crate | 成立（传递依赖口径见 §六） | `crates/agent-core/src/session/checkpoint.rs` 里 `rig_core` / `rmcp` 命中 **0**；`cargo tree -p agent-core --depth 1` = `framework-proc-auto` / `framework-proc-core` / `lib-core` / `serde` / `serde_with` / `specta` / `strum*`，**没有** `rig-core` / `rmcp` / `sqlx` / `lib-db` / `service-core`。唯一 `framework_` 命中是 `use framework_proc_auto::auto_enum;`（`AGENTS.md` 第 20 条要求的 API 标记宏），不是协议类型 |
| A-2 | 没有新增 Crate | 成立 | `crates/` 下 20 个目录与 `Cargo.toml` 的 `members` 20 项一一对应；不存在 `agent-checkpoint` / `agent-resume` / `manager-core` / `types-api` |
| A-3 | SQL 只在 repository | 成立 | `grep -rln "run_checkpoints" --include=*.rs crates` 的产品 crate 命中只有 `crates/service-core/src/repository/checkpoint.rs`（`crates/agent-core/.../checkpoint.rs` 是一处文档注释里的列名说明）；`agent-runtime` / `manager-agent` / `lib-web` 的 SQL 字面量检索（`"(SELECT` / `"(INSERT` / `"(UPDATE` / `"(DELETE`）命中 0 |
| A-4 | 装配层不承载循环逻辑 | 成立 | `crates/manager-agent/src/` 里 `CheckpointPhase` 命中 **0**、`ToolCallDigest` 命中 **0**、`RunCheckpoint::new` 命中 **0**（检查点载荷构造只在 `agent-runtime/src/loop_/checkpoint.rs` 一处）；`ResumePoint` 只出现在 `runner/queue.rs`（`run_one` 的转发参数）与 `runner/resume.rs`（判定结果转发） |
| A-5 | 运行时只经端口访问业务 | 成立（传递依赖口径见 §六） | `cargo tree -p agent-runtime --depth 1` 的 14 条直接依赖里**没有** `lib-db` / `sqlx`（两者只经 `service-core` 传递出现）；`grep -rn "sqlx" crates/agent-runtime/src` 命中 0；检查点与恢复的调用全部是 `service_core::session::{SessionService, RunDriver, ResumePlan}` |
| A-6 | 结构文档待同步清单 | 成立 | 清单见 §九（复核轮之前的记录）；复核轮已按授权把清单落地，见 §七 |

---

## 三、状态机与检查点（S / C 组）

### 3.1 状态机与终态唯一（S 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| S-1 | 状态集合 | 成立 | `RunStatus` 的既有六值 wire 值逐字不变，新增只有 `Paused`（`PAUSED`）；`is_terminal()` / `is_active()` / `can_cancel()` 各只有一处定义（`crates/agent-core/src/session/run.rs`），终态与未结束集合各只有一个常量（`TERMINAL` / `ACTIVE`） |
| S-2 | 强杀自愈为 `PAUSED` | 成立 | `resume.crash`：强杀重启后 `run/get` 的 `status = PAUSED`、`interruptedReason = 「服务重启，执行中的 Run 已中断」`、`finishedAtMs` 为空、`resumeCount = 0` |
| S-3 | 终态不被改写 | 成立（原文的一处口径见 §六） | `resume.terminal`：对已 `PAUSED` 的 Run 取消后落 `CANCELLED`；对已终态的 Run 二次取消返回「不可取消」且 `status` / `error` / `finishedAtMs` / `updatedAtMs` 逐字段不变；随后发消息只执行新 Run（`run_completed`），已终态 Run 的 `status` / `updatedAtMs` 仍不变 |
| S-4 | 终态即裁剪检查点 | 成立（复核轮补正，见 §八） | `resume.checkpoint`：`fail_run` 之后 `steps` 条数 ≤ `CHECKPOINT_KEEP_LAST`，裁剪与状态推进在同一事务里完成，用例不再手工调清理；`resume.terminal` 在隔离服务端上断言取消后的条数与 `checkpointsPruned` 增量逐值相等 |
| S-5 | 恢复不产生第二条助手消息 | 成立 | `resume.crash`：恢复完成后该 Run 的助手消息**恰好 1 条**；`resume.terminal` 的「两次中断都不应补写助手消息」再验一次 |
| S-6 | 恢复中再次中断仍是一个 `PAUSED` | 成立 | `resume.terminal`：恢复执行中强杀 → 重启，同一 `runId` 仍是单条 `PAUSED`、`resumeCount` 从 0 涨到 1 后不再重置、`finishedAtMs` 为空 |
| S-7 | 探活口径 | 成立 | `resume.crash` 断言 `PAUSED` 期间 `api/probe/health` 的 `runningRunCount = 0`；统计 SQL 只 `WHERE status = 'RUNNING'`（`service-core` 的探活查询，注释写明 `PAUSED` 不计入） |

### 3.2 检查点（C 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| C-1 | 三种 phase 各有唯一落点 | 成立 | `RunCheckpoint::new` 全仓只有一处调用点（`agent-runtime/src/loop_/checkpoint.rs` 的 `append`）；四个写入点都在对应提交之后；取消路径（`cancelled`）不调用 `append` |
| C-2 | 序号连续 | 成立 | `resume.checkpoint`：两轮工具 + 正常完成的 `steps` 序号 `1..5` 连续、phase 依次 `MODEL_DONE / MODEL_DONE / TOOLS_DONE / MODEL_DONE / TOOLS_DONE`；存储半段另断言 `1..3`。「人为制造缺口后判损坏」属 `R-6`（见 §六） |
| C-3 | 指针同事务 | 成立 | `resume.checkpoint`：注入 `run_checkpoints` 插入失败后 `lastCheckpointSeq` 保持 1、库内仍只有 1 行；解除注入后接着写 `seq 2`、指针前移到 2（序号不重置）。实现上「插行 + 更新指针」在同一 `transaction` 内（`Repository::append_checkpoint`） |
| C-4 | payload 最小事实 + 保留策略 | 成立 | 载荷顶层白名单 = `["assistant_message_id", "format_version", "tool_calls"]`（复核轮加入助手消息 ID，只在起点有值），摘要白名单 = `["status", "tool_call_id"]`；单条 ≤ 2048 字节；终态后保留 20 条（`CHECKPOINT_KEEP_LAST`）、`resume.checkpoint` 断言 25 → 20（裁剪在 `fail_run` 事务内完成） |
| C-5 | 格式版本单一产点 | 成立 | `grep -rn "CHECKPOINT_FORMAT_VERSION" --include=*.rs crates \| grep const` 只有一处 `crates/agent-core/src/session/checkpoint.rs:16`；`resume.checkpoint` 断言读回行的 `format_version` 与载荷里的 `format_version` 都是 `1` |
| C-6 | 未知版本不可恢复 | 成立 | `resume.broken` ⑥：`format_version = 99` 后触发恢复 → Run 落 `FAILED` + 「检查点格式版本不受支持，Run 不可恢复」，工具审计行数不变 |
| C-7 | 检查点写失败 → `FAILED` | 成立 | `resume.checkpoint` 的注入式失败段：Run 落 `FAILED`、`run_failed` 与 `run/get` 的 `error` 都是「检查点写入失败」、无助手消息、该 Run 的检查点 0 条 |

---

## 四、恢复、持久化、接口与可观测（R / P / I / O 组）

### 4.1 恢复（R 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| R-1 | 强杀后能恢复并完成 | 成立 | `resume.crash`：`mock:slow` 停在 `RUNNING` → `start_kill` → 重启（`PAUSED`）→ 重新注册 + 占用 + **发一条消息** → 收到该 Run 的 `run_completed`，`run/get` 的 `status = COMPLETED`、`resumeCount = 1`；判定动作是「等到首段增量」而不是 sleep |
| R-2 | 触发条件与不触发条件 | 成立 | `resume.trigger`：① 只 `occupancy/create` 不发消息（观察窗口 2 秒）→ `PAUSED` 的 `status` / `updatedAtMs` / `resumeCount` 逐字段不变；② 占用 + 发消息 → `resumeCount` 0 → 1；③ `api/run/resume` 与 `resume_on_occupy` 全仓（`crates/`、`lingting-agent-ui/src/`、`packages/`）命中 0 |
| R-3 | 重复触发幂等 | 成立 | `resume.idempotent`：连续两条消息后 `resumeCount = 1`（只恢复一次），该 Run 的工具审计行数保持 0（没有重复执行） |
| R-4 | 不重跑已提交轮次 | 成立 | `resume.crash` 第二段：一轮工具之后强杀，已提交的助手消息内容逐字符不变，`steps` 从 3 条起**前缀逐条相同、总数不减**；`resume.crash` 第一段的 `steps` 为 `[1]`（不重置为 0） |
| R-5 | 未配对调用一律不重放 | 成立 | `resume.notify`：已配对 → 审计 0 条、不执行；`PENDING` → 不执行、落 1 条 `ERROR` 审计 + 1 条 TOOL 消息、文本含「上次执行被中断：服务重启」；旧库行无 `PENDING` → 同第二类。两类未配对形态的工具调用次数均为 0（以 `tool_calls` 行数为准） |
| R-6 | 损坏不可恢复 | 成立 | `resume.broken` ①~⑥ 各自落 `FAILED` 且文案互不相同（`RESUME_GAP` / `RESUME_MESSAGES_LOST` / `RESUME_TRACE_BROKEN` / `RESUME_PENDING_LEAK` / `RESUME_PHASE_MISMATCH` / `CHECKPOINT_UNSUPPORTED`），六条都断言「审计行数不变」；快照侧 `resumeUnrecoverable = 6` |
| R-7 | 旧 Run 无检查点 | 成立 | `resume.broken` ⑦：删掉该 Run 的全部检查点后触发恢复 → 不进入执行、不改任何行（`status` / `updatedAtMs` / `resumeCount` / `finishedAtMs` 逐字段不变），`run/history` 的 `steps = []` 且 `interruptedReason` 可读；快照侧 `resumeRejected = 1` |
| R-8 | 事件文件不是恢复前提 | 成立 | `resume.crash`：强杀后**删除** `<AGENT_HOME>/data/events/<sessionId>` 再重启、占用、发消息，恢复仍成功（`status = COMPLETED`、`resumeCount = 1`、助手消息 1 条） |
| R-9 | 恢复期间取消仍终态 | 成立 | `resume.terminal`：恢复执行中调 `api/run/cancel` → `CANCELLED` + 固定原因 + 有结束时间，`resumeCount` 不再增长（保持 1），后续不再产生该 Run 的事件 |

### 4.2 持久化与迁移（P 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| P-1 | 迁移幂等 | 成立 | `persist.migration`：同一数据库连续两次迁移退出码 0，`run_checkpoints` 只有一张、`runs` 新列不重复；迁移账本 `[1, 2, 3]` |
| P-2 | 旧库可升级 | 成立 | `persist.migration`：`0001` + `0002` 的旧库（含终态 Run、工具审计行、消息）升级到 `0003` 后逐字段读回相等；`resumeCount = 0`、`interruptedReason` / `lastCheckpointSeq` 为 `NULL` |
| P-3 | 新列对旧逻辑无害 | 成立 | `persist.migration` + `run`（10/10）+ `tool`（28/28）全绿；`run.failed_auth` / `failed_upstream` / `failed_timeout` 的文案与字段断言未放宽 |
| P-4 | 升级不可回滚（人工） | 成立 | `crates/lib-db/migrations/` 只有 `0001_init.sql` / `0002_tool_calls.sql` / `0003_checkpoints.sql` 三个前进迁移，**没有** down 脚本；任务结案说明写明「升级前备份 `agent.db`」（§7.1）；回滚策略登记为 6E 输入（§六 `KI-P6A-003`） |
| P-5 | 检查点不无限增长 | 成立（复核轮补正，见 §八） | 存储半段（`resume.checkpoint`）：25 条写入 → 未终态时一条不删 → 落 `FAILED` 时**在同一事务里**删除 5 条、保留 20 条，`checkpointsPruned` 至少增长 5。隔离服务端的 `resume.terminal`：取消一个 `PAUSED` Run 后 `steps` 从 25 裁到 20，同一次取消的 `checkpointsPruned` 增量与删掉的条数**逐值相等**（该段的步骤基数为 1，增量 = 0，因为取消前已被前一次终态收尾裁过） |

### 4.3 接口与客户端（I 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| I-1 | 接口清单 | 成立 | `crates/lib-web/src/session/api.rs` 的 `#[web_api_*]` 新增恰好一条（`api/run/history`）；`api/run/resume` 不存在；`make gen` 输出「路由 33 条，接口 33 条」（P5 基线 32 / 32，**+1**） |
| I-2 | `history` 语义 | 成立 | `resume.history`：`steps` 按 `seqNo` 升序、从 1 连续；字段白名单 = `["createdAtMs", "phase", "seqNo", "toolRound"]`（无 payload 原文）；无检查点 Run 返回 `steps = []` 且 `status = QUEUED`；未知 `runId` → 404 |
| I-3 | 恢复的事件时间线与顺序 | 成立（一处口径见 §六） | `resume.trigger`：一次发消息同时触发「恢复旧 Run」与「新建 Run」，SSE 上恢复 Run 的 `run_completed`（seq 14）早于新 Run 的 `run_started`（seq 15）。「上下文重新组装」当前没有独立字段：`RunHistory` 的字段是 `run` / `resumeCount` / `interruptedReason` / `steps`，恢复不承诺上下文逐字节一致（D-P6-003 第 3 条） |
| I-4 | TUI | 成立 | `resume.tui`：状态栏显示「已中断（发送消息可继续）：服务重启，执行中的 Run 已中断」（原因原样）；中断态下输入框可用；发消息后两条 Run 各有一条助手回复（共 2 条），`resumeCount = 1` |
| I-5 | WebUI | 成立 | `resume.web`：前端工程 `tests/resume-paused.test.tsx` / `tests/resume-banner.test.tsx` 共 8 条通过（提示文案、提示条、输入框可用、无恢复按钮、双 Run 归类）；客户端源码 40 个文件里 `api/run/resume` / `resume_on_occupy` 命中 0；`make ui-check` 退出码 0 |

### 4.4 可观测（O 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| O-1 | `resume` 段字段 | 成立 | `resume.metrics`：`api/probe/metrics` 的 `resume` 段是**七个**固定键（`O-1` 的六个契约字段 + P6A-018 的 `checkpoint_payload_bytes`）：`checkpoints_pruned` / `checkpoints_written` / `resume_attempts` / `resume_rejected` / `resume_succeeded` / `resume_unrecoverable` / `checkpoint_payload_bytes` |
| O-2 | 数值可复算 | 成立 | `resume.metrics`：`checkpointsWritten` == 两个 Run 的 `steps` 条数之和、`resumeSucceeded` == 各 Run 的 `resumeCount` 之和、`resumeAttempts = 1`；`resume.broken`：`resumeUnrecoverable = 6`（六类损坏）、`resumeRejected = 1`（无检查点）；`resume.checkpoint`：写入 / 删除增量与条数逐值相等；`resume.retry`：`limit.retries = 2` == 配置次数 |
| O-3 | 未开特性显式「未采集」 | **未采集** | 未开 `metrics` 的构建下 `api/probe/metrics` 不注册（`#[cfg(all(debug_assertions, feature = "metrics"))]`），因此该构建里**接口不存在**。本轮所有用例都在 `metrics` 特性构建下执行，未构造「无特性构建的快照」；报告写「未采集：接口在未开特性的构建里不注册」，不写成「全 0」。替代证据：`make gen` 的生成物里 `probe/metrics` 命中 0（证明它不进发布产物与 SDK） |
| O-4 | 日志不是数据源 | 成立 | `crates/bin-tests/src/cases/resume*` 里没有 `server.log` 读取；`read_to_string` 的命中只有 `resume/web.rs` 读客户端源码（静态检索用）；本轮 `git diff` 新增的日志只有 `resume` 段的一条 `warn!`（恢复判定的既有格式，不是采集型 `debug!` / 阈值日志） |

---

## 五、错误边界与回归（E / K 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| E-1 | 未占用不恢复 | 成立 | `resume.trigger`：未占用时发消息 → 新 Run 停在 `QUEUED`（既有行为），`PAUSED` 的 Run 逐字段不变 |
| E-2 | 只有 `PAUSED` 会被推进 | 成立 | `resume.trigger`：同一会话存在 `QUEUED` / `WAITING` / 终态行时发消息，只有 `PAUSED` 被恢复（`resumeCount` +1），排队中的 Run `resumeCount` 保持 0 |
| E-3 | 检查点不含敏感内容 | 成立 | 载荷字段白名单（`format_version` + 工具组摘要）静态核对 + `resume.checkpoint` 对 `api_key` / `AGENTS.md` / 「系统提示词」三个串的检索命中 0 |
| E-4 | 不泄漏绝对路径 | 成立 | `resume.broken` 对六条失败文案检索 `AGENT_HOME` 绝对路径命中 0；`resume.notify` 对回灌文本做同样的检索命中 0 |
| E-5 | 未知 Run | 成立 | `resume.history`：不存在的 `runId` → 404 + 统一错误契约；`api/message/create` 对不存在的 `sessionId` 仍是既有 404 口径（未被恢复逻辑改变） |
| K-1 | Skill 语义未变 | 成立 | `make test ARGS="skill"` **15/15 通过**（`run-1795771247486-42336`），`skill.index` / `skill.budget` 的断言原样通过 |
| K-2 | 工具审计兼容 | 成立 | `make test ARGS="tool"` **28/28 通过**（`run-1795771255790-37816`）；`tool.audit` 的字段除 Batch B 已登记的 `startedAtMs` 外逐字段不变 |
| K-3 | 自愈口径变更已登记 | 成立 | `persist.running_heal` 的断言已是 `PAUSED` + `interrupted_reason`（Batch C 结案说明登记为「口径变更」）；`make test ARGS="persist"` **5/5 通过** |
| K-4 | Run 与运行时未回归 | 成立 | `make test ARGS="run"` **10/10**、`ARGS="runtime"` **18/18** 全绿；`run_max_retries = 0` 的三条失败用例断言的仍是原始脱敏文案 |
| K-5 | 全量回归 | 成立 | `make test ARGS="--all"` → **150/150 通过**（`run-1795772185789-27720`）= P5 基线 **138** + 6A 新增 **12**（`resume` 域 12 条：`checkpoint` / `terminal` / `crash` / `trigger` / `idempotent` / `notify` / `broken` / `history` / `metrics` / `retry` / `tui` / `web`） |
| K-6 | 事件类型未新增 | 成立 | `git diff crates/agent-core/src/session/event.rs` 为空；`PAUSED` 只经 `api/run/get` 的 `status` 可见，事件里不出现新的 kind |

---

## 六、未采集项与结转风险（`KI-P6A-xxx`）

### 6.1 未采集（不计入通过）

| 编号 | 项 | 原因 | 替代证据 |
|---|---|---|---|
| — | `O-3` 无特性构建的 `metrics` 快照 | 未开 `metrics` 的构建里 `api/probe/metrics` 根本不注册，取不到快照 | `make gen` 的生成物里 `probe/metrics` 命中 0（不进发布产物与 SDK）；`#[cfg(all(debug_assertions, feature = "metrics"))]` 静态核对 |

### 6.2 口径差异（判据原文与实现的偏差，逐条说明）

| 编号 | 判据原文 | 实际口径 | 原因 |
|---|---|---|---|
| — | `A-1`「`checkpoint.rs` 检索 `rig_core` / `rmcp` / `framework_` 命中数为 0」 | `rig_core` / `rmcp` 命中 0；`framework_` 命中 1 | 唯一命中是 `use framework_proc_auto::auto_enum;`——`AGENTS.md` 第 20 条要求 API 类型用 `#[auto_*]` 框架宏标记。判据本意是「协议类型不进来」，因此按 `rig_core` / `rmcp` 判定 |
| — | `A-5`「`cargo tree -p agent-runtime` 无 `lib-db` / `sqlx`」 | 直接依赖无；传递依赖有（`service-core → lib-db → sqlx`） | `cargo tree` 默认展开传递依赖。按 `--depth 1` 判定「运行时没有直接拿到 SQL 能力」，与 `grep -rn "sqlx" crates/agent-runtime/src` 命中 0 一致 |
| — | `S-3`（第一条）「对终态 Run 调用 `api/run/resume` 返回 409」 | 该接口不存在（D-P6-009 定案不新增恢复接口） | 判据原文与决策冲突，以 D-P6-009 为准；第二条 `S-3`（恢复触发 + `api/run/cancel`）才是有效口径，已逐字段取证 |
| — | `C-2`「人为制造缺口后读回能被判为损坏」 | 由 `resume.broken` ①（`RESUME_GAP`）取证，不在 `resume.checkpoint` 里 | 序号缺口的判损坏路径在恢复判定（`loop_::checkpoint::plan`），`resume.checkpoint` 只覆盖存储层 |
| — | `I-3`「`api/run/history` 显示『上下文重新组装』标记（若实现为字段则断言字段，若为文案则断言文案）」 | 既没有字段也没有文案 | D-P6-003 第 3 条只要求「恢复时 Context 重新组装，并在步骤历史里显示」。当前实现选择了「不记录上下文指纹、不加标记字段」——加字段会与 P5 的 Skill 集合、`AGENTS.md` 的现场读取口径纠缠。见 `KI-P6A-004` |
| — | `O-1` 六个字段名写作 camelCase（`checkpointsWritten` 等） | 快照键名是 **snake_case**（`checkpoints_written` 等），且是**七个**键 | 既有七段（`chain` / `stop` / `journal` / `limit` / `mcp` / `skills` / `audit`）全部是 snake_case；为一段单独改 camelCase 会造出两套命名。第七个键 `checkpoint_payload_bytes` 是 P6A-018 的写放大口径 |
| — | `P-5`「`metrics` 的 `checkpointsPruned` 与删除条数相等」 | `resume.checkpoint` 的存储半段断言「至少增长」（进程内共享计数器）；「逐值相等」在 `resume.terminal` 的隔离服务端上断言 | 存储半段在同进程并行跑，共享原子量会被其它用例累加；隔离服务端的服务端进程独占计数器，可以做等值断言 |
| — | P6A-014 第 1 条「`resume_rejected` 按拒绝原因分桶」 | 快照里是**单个标量**（「没有检查点」与「判定读库失败」两个计数器求和） | 两个原因对外只需要一个「被拒绝」的数量；分桶会多出两个没有消费方的键。拒绝原因仍可从日志与 `resume.broken` 的用例分支区分 |
| — | P6A-017 第 4 条「配置非法是启动期硬错误（退出码 2）」 | `llm.config` 断言的是配置解析报错，未起子进程断言退出码 | 重试项的校验与其他 `[agent]` 项走同一个 `validate_agent`，`mcp.config` 已对同一路径的退出码 2 取证 |

### 6.3 结转风险

| 编号 | 风险 | 说明与后续 |
|---|---|---|
| `KI-P6A-001` | `PAUSED` 是契约变更 | 新增枚举值影响两份 SDK 的生成产物与 TUI / WebUI 的穷尽匹配。本子阶段的门禁（`make gen-check` / `make ui-check`）与 `S-1` / `I-4` / `I-5` 三处断言都覆盖了它，但**下游 API 消费者（第三方）无法枚举**：需要在 6E 的发布说明里显式登记「`RunStatus` 新增 `PAUSED`」 |
| `KI-P6A-002` | 副作用不可回滚的两类未知 | ① 工具执行期间崩溃：`PENDING` 开始标记只让「有开始无结束」可见，**不能证明副作用是否已经发生**；② 恢复时一律不重放，模型看到的只是「上次执行被中断：<原因>」，是否重新发起由模型决定。副作用补偿事务、工具幂等键协议登记为 6E 输入 |
| `KI-P6A-003` | 迁移不可回滚 | `lib-db` 只做前进迁移，`0003` 没有 down 脚本。升级前必须备份 `agent.db`（人工判定项 `P-4`）；回滚策略（能否降级 binary、旧库能否被新代码读）登记为 6E 输入 |
| `KI-P6A-004` | 恢复不承诺上下文一致 | 检查点只记录「已提交消息的上界」，Context 在恢复时**重新组装**：`AGENTS.md`、Skill 集合、Provider 配置、模型预算都可能与崩溃时不同。`RunHistory` 因此没有「上下文重新组装」标记（`I-3` 的口径差异）。若将来要求可追溯，应先给 `PAUSED` 加保留期（`D-P6-013` 的重开前提） |
| `KI-P6A-005` | Run 级重试只覆盖「同一次 LLM 请求」 | 已经推过 `message_delta` 的轮次不重试（重发会让客户端看到两段拼接文本），因此「流中断在半途」仍然是直接失败。若需要「整轮重发」，得先给客户端一个「丢弃前一段」的协议，属 6D / 6E |
| `KI-P6A-006` | 事件文件与恢复的时间线差异 | 恢复产生新的运行时会话与新的文件，`seq` 从 0 重新计数；客户端必须按 `run_id` 归并两段事件，不能假设 `seq` 连续（`I-4` / `I-5` 已按此实现）。轮转仍可能删掉含 `PAUSED` 的会话目录（`D-P6-013` 不采纳保护） |
| `KI-P6A-007` | 用例设施的偶发风险 | 全量回归首跑出现过两次**与本改动无关**的失败：`skill.security`（`run-1795770644353-38740`）与 `mcp.file_access`（`run-1795771422154-40604`），单独复跑与后续全量均通过。前者是既有 Skill 用例的时序敏感，后者是 MCP 替身的三次写声明未全部被调用（`KI-P5-004` 同类的「测试设施偶发风险」），按判据口径不计入 6A |
| `KI-P6A-008` | 检查点表仍会随 Run 时长增长 | 长 Run 每轮 2 条；终态时按 `CHECKPOINT_KEEP_LAST = 20` 裁剪。**未结束**的 Run（含长期 `PAUSED`）一条都不删——这是「可恢复性优先」的取舍，代价是长期中断的 Run 会占住它的检查点行 |

---

## 七、文档同步记录（复核轮已按授权执行）

复核时发现四份文档与代码不一致（`docs/api/session.md` 的「本阶段进度」段仍写「随 P6A-009 ~ P6A-013 落地」，而它们已全部落地），已按需求方授权同步：

| 文档 | 已补内容 |
|---|---|
| `docs/api/session.md` | ① 接口清单补 `api/run/history`，`api/message/create` 补「已占用时先恢复 `PAUSED` Run」的副作用；② 入参表补 `RunHistoryParams`；③ 新增 `RunHistory` / `RunStep` 两个载荷类型章节；④ 「检查点」章节改为「落终态即裁剪（事务内）」、补助手消息 ID 与「恢复复用同一 ID」的口径，并删掉已过时的「本阶段进度」段 |
| `docs/api/probe.md` | `metrics` 的 `resume` 段七个字段（snake_case）与复算口径；`limit` 段新增的 `retries` |
| `docs/workspace.md` | ① `service-core` 的 `repository/checkpoint.rs` 与 `observability.rs`；② `agent-core` 的检查点类型；③ `agent-tool` 的 `ToolCallRecord.started_at_ms`（并注明**没有**新增可重放自述）；④ `manager-agent` 的 `runner/resume.rs`（恢复段） |
| `docs/develop.md` | `[agent]` 的 `run_max_retries` / `run_retry_backoff_ms`（缺省与上限、启动期硬错误）；失败分类与重试口径；检查点保留策略改为「裁剪在推进终态的同一事务里完成」 |
| `.steps/p6/6a/README.md` / `.steps/README.md` | 状态行 |

## 八、复核轮补正（本轮新增）

| 项 | 原状 | 补正 |
|---|---|---|
| 终态裁剪 | 只有 `cancel_run` 事后清理一次；`COMPLETED` / `FAILED` 不清理，检查点会随 Run 生命周期一直留在库里 | 裁剪收敛到推进终态的同一事务：`Repository::prune_checkpoints_in`（唯一实现）由 `with_run_completion` / `fail_run` / `cancel_run` 调用，`cancel_session_runs` 走会话级版本；`SessionService::prune_checkpoints` 的事后入口删除 |
| 起点检查点的助手消息 ID | `CheckpointPayload` 没有该字段，`P6A-005` 第 1 条悬空 | `CheckpointPayload.assistant_message_id`（只在起点有值）→ `ResumePoint.assistant_message_id`（从第 1 条取回）→ 恢复的 `run_started` 与收尾复用同一 ID |
| `acceptance.md` 编号 | 两条 `S-3`、缺 `S-4` | 合并为一条 `S-3`（并注明与 D-P6-009 的冲突以决策为准），新增 `S-4`「终态即裁剪检查点」 |
| 文档口径差异 | 两条未登记（`resume_rejected` 分桶、配置非法退出码） | 补进 §6.2 |
| 用例与门禁复跑 | 补正前只跑过旧代码 | 授权下复跑：`resume` 12/12、`persist` 5/5、`run` 10/10、`tool` 28/28、`runtime` 18/18，全量 `--all` **150/150**；`make check` / `gen-check` / `ui-check` 全部 0 退出。载荷实测由 272 字节 / 54 字节变为 **428 字节 / 85 字节**（已回填 §一 第 4 条） |

## 九、文档待同步清单（复核轮之前的记录，保留备查）

| 文档 | 待补内容 |
|---|---|
| `docs/api/session.md` | ① `RunStatus` 取值表补 `PAUSED`（语义：非终态、只能由启动自愈产生、只能由「占用 + 发消息」消费）；② 「Run 状态机与检查点」章节的转换表补 `RUNNING → PAUSED` / `PAUSED → RUNNING` / `PAUSED → CANCELLED`；③ `api/run/history` 章节（字段白名单、`steps` 升序、404 口径）；④ `api/message/create` 的副作用说明（成功后会先恢复该会话的 `PAUSED` Run，再执行新 Run） |
| `docs/api/probe.md` | `metrics` 的 `resume` 段七个字段（snake_case）与复算口径；`limit` 段新增的 `retries` |
| `docs/workspace.md` | ① `service-core` 的 `checkpoint` 模块与 `observability` 模块；② `agent-core` 的检查点类型（`RunCheckpoint` / `CheckpointPhase` / `CHECKPOINT_FORMAT_VERSION` / `ResumePoint`）；③ `agent-tool` 的 `ToolCallRecord` 新增 `started_at_ms`（并注明**没有**新增可重放自述）；④ `manager-agent` 的 `runner/resume.rs`（恢复段）；⑤ 注明 `agent-tool → agent-runtime` 的反向边仍不存在 |
| `docs/develop.md` | `[agent]` 的两个新配置项（`run_max_retries` 缺省 `0` = 不重试、上限 10；`run_retry_backoff_ms` 缺省 500ms、`0` = 立即重试、上限 60000ms）与「非法取值是启动期硬错误（退出码 2）」 |
| `.steps/p6/6a/README.md` / `.steps/README.md` | 状态行（本报告与 `tasks.md` 已随本轮更新） |

### 9.1 升级前人工动作

- 升级到 `0003` 前**备份 `<AGENT_HOME>/data/agent.db`**（`P-4` 的人工判定项；`lib-db` 只有前进迁移）。
- 升级后首次启动会执行自愈：执行中的 Run 落 `PAUSED`、排队中与等确认的 Run 落 `CANCELLED`。这是**行为变更**，运维侧要知道「重启后看到 `PAUSED` 是正常的，发一条消息即可继续」。
