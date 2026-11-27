# 6A 验收清单

**状态：已完成（Batch A ~ E 全部结案）。** 十组共 59 条：除 `O-3` 记为「未采集」（无 `metrics` 特性的构建不注册该探针）外全部成立，另有 6 处判据原文与实现的偏差在[报告](../../../docs/reports/p6a-020-acceptance.md) §6.2 逐条说明。**任一条不通过（或未取得授权、或明确记为未采集后未给出替代证据），6A 不视为完成。**

证据只允许来自：接口响应、客户端事件、`metrics` 快照、源码静态核对、`git diff`（限定到本次改动文件）。**不得读取、解析或检索 `server.log` 与日志目录**（仓库根 `AGENTS.md` 的测试数据采集条款）。

验证方式只有四类：

- **命令**：可直接执行的命令与期望退出码。
- **用例**：`make test ARGS="resume"` 等，用例名即证据。
- **静态核对**：`cargo tree`、源码检索、`git diff`、逐字段比对表、调用点计数。
- **人工**：只能人工判定的项，写明判定动作。

授权列的含义：

| 取值 | 含义 |
|---|---|
| **需授权** | 需要执行命令或运行用例，必须先取得明确授权；未授权时该项在报告里写「未授权」，不得留空、不得算通过 |
| **不需授权** | 只读核对（源码检索、`cargo tree`、`git diff`、逐字段比对），不执行构建与用例 |

---

## 1. 门禁（B-1 ~ B-4）

| 编号 | 条目 | 验证方式 | 授权 |
|---|---|---|---|
| B-1 | workspace 全量可编译 | `cargo build --workspace` 退出码 0，无 error | 需授权 |
| B-2 | 格式一致 | `cargo fmt --all --check` 退出码 0 | 需授权 |
| B-3 | Clippy 零告警 | `cargo clippy --workspace --all-targets -- -D warnings` 退出码 0 | 需授权 |
| B-4 | 生成物与前端门禁 | `make gen`（记录路由数与接口数，6A 相对当前各 **+1**，只新增 `api/run/history`）、`make gen-check`、`make ui-check` 退出码均为 0 | 需授权 |

## 2. 架构与依赖（A-1 ~ A-6）

| 编号 | 条目 | 验证方式 | 授权 |
|---|---|---|---|
| A-1 | 检查点类型不进协议 crate | 在 `crates/agent-core/src/session/checkpoint.rs` 检索 `rig_core` / `rmcp` / `framework_` 三个关键词，命中数为 0；`cargo tree -p agent-core` 里没有 `rig-core` / `rmcp` / `sqlx` / `lib-db` / `service-core` | 不需授权 |
| A-2 | 没有新增 Crate | `crates/` 下不存在 `agent-checkpoint` / `agent-resume` / `manager-core` / `types-api`；`Cargo.toml` 的 members 数量不变 | 不需授权 |
| A-3 | SQL 只在 repository | 全仓 `grep -rn "run_checkpoints" --include=*.rs` 只命中 `crates/lib-db/migrations/` 与 `crates/service-core/src/repository/`；`agent-runtime` / `manager-agent` / `lib-web` 无 SQL 字面量 | 不需授权 |
| A-4 | 装配层不承载循环逻辑 | `crates/manager-agent/src/` 下不出现检查点 payload 构造、不出现恢复点解析（在该目录检索 `ResumePoint` 与 `CheckpointPhase`，命中只允许出现在转发调用与日志字段） | 不需授权 |
| A-5 | 运行时只经端口访问业务 | `agent-runtime` 对检查点与恢复的调用全部经 `service-core::session` 的端口方法；`cargo tree -p agent-runtime` 无 `lib-db` / `sqlx` | 不需授权 |
| A-6 | 结构文档待同步清单 | 任务结案说明里给出 `docs/workspace.md` 需要补的四条（`service-core` 的 `checkpoint` 模块、`agent-core` 的检查点类型、`agent-tool` 的 `ToolCallRecord` 新增字段且**没有**新增可重放自述、`agent-tool → agent-runtime` 反向边仍不存在）；本轮 `git status` 无文档改动 | 不需授权 |

## 3. 状态机与终态唯一（S-1 ~ S-7）

| 编号 | 条目 | 验证方式 | 授权 |
|---|---|---|---|
| S-1 | 状态集合 | 静态核对 `RunStatus`：既有六值的 wire 值逐字不变，新增仅 `PAUSED`；`is_terminal()` / `is_active()` / `can_cancel()` 各只有一处定义 | 不需授权 |
| S-2 | 强杀自愈为 `PAUSED` | 用例 `resume.crash`：强杀后重启，`api/run/get` 的 `status = PAUSED`、`interruptedReason` 为固定值、`finishedAtMs` 为空 | 需授权 |
| S-3 | 终态不被改写 | 用例 `resume.terminal`：对已 `COMPLETED` / `CANCELLED` / `FAILED` 的 Run 分别触发一次恢复（占用 + 发消息）与一次 `api/run/cancel`，各自都不改变该 Run 的 `status`、`error`、`finishedAtMs`（逐字段相等）；`api/run/cancel` 对终态返回既有口径。**注**：原文「调用 `api/run/resume` 返回 409」与 D-P6-009 冲突（该接口不存在），以决策为准 | 需授权 |
| S-4 | 终态即裁剪检查点 | 用例 `resume.checkpoint`：`fail_run` 之后 `steps` 条数 ≤ `CHECKPOINT_KEEP_LAST`（裁剪与状态推进同一事务，用例不手工触发清理）；`resume.terminal` 在隔离服务端上断言取消后的条数与 `checkpointsPruned` 增量逐值相等 | 需授权 |
| S-5 | 恢复不产生第二条助手消息 | 用例 `resume.crash`：崩溃前已提交 1 条助手消息，恢复完成后 `api/message/list` 中该 Run 的助手消息条数仍为 1 | 需授权 |
| S-6 | 恢复中再次中断仍是一个 `PAUSED` | 用例 `resume.terminal`：恢复执行中强杀 → 重启，`api/run/get` 是单条 `PAUSED`，`resumeCount` 单调不减（≥ 1） | 需授权 |
| S-7 | 探活口径 | 用例 `resume.crash` 断言 `api/probe/health` 的 `runningRunCount` 不把 `PAUSED` 计入（`PAUSED` 期间为 0）；静态核对统计 SQL 只 `WHERE status = 'RUNNING'` | 需授权 |

## 4. 检查点（C-1 ~ C-7）

| 编号 | 条目 | 验证方式 | 授权 |
|---|---|---|---|
| C-1 | 三种 phase 各有唯一落点 | 静态核对调用点：`MODEL_DONE` / `TOOLS_DONE` / `AWAITING_CONFIRM` 各只有一个构造函数（可被多次调用，但构造与追加只有一处）；取消路径不写检查点 | 不需授权 |
| C-2 | 序号连续 | 用例 `resume.checkpoint`：`api/run/history` 的 `steps[].seqNo` 从 1 开始连续，无缺口 | 需授权 |
| C-3 | 指针同事务 | 用例 `resume.checkpoint`：注入检查点插入失败，`api/run/get` 的 `lastCheckpointSeq` 与失败前相等；静态核对「插行 + 更新指针」在同一事务函数内 | 需授权 |
| C-4 | payload 最小事实 + 保留策略 | 静态核对序列化结构字段白名单（phase / round / message 上界 / 工具组摘要）；用例 `resume.checkpoint` 断言单条 payload ≤ 2 KiB、终态后 `steps` 条数 ≤ `keep_last`（缺省 20） | 需授权 |
| C-5 | 格式版本单一产点 | 静态核对：在 `crates/` 检索 `CHECKPOINT_FORMAT_VERSION`，定义处（`const` 声明）只有一处；用例 `resume.checkpoint` 断言写入行的 `formatVersion = 1` | 需授权 |
| C-6 | 未知版本不可恢复 | 用例 `resume.broken`：把检查点的 `formatVersion` 改成当前+1 后恢复 → Run 落 `FAILED`，`error` 是 `Unsupported` 的固定文案，工具审计行数不增加 | 需授权 |
| C-7 | 检查点写失败 → `FAILED` | 用例 `resume.checkpoint` 的注入式失败分支：Run 落 `FAILED`、`error` 为固定文案、无助手消息、`steps` 为空 | 需授权 |

## 5. 恢复（R-1 ~ R-9）

| 编号 | 条目 | 验证方式 | 授权 |
|---|---|---|---|
| R-1 | 强杀后能恢复并完成 | 用例 `resume.crash`：`mock:slow` 停在 `RUNNING` → `start_kill` → 重启（`PAUSED`）→ 重新注册 + 占用 + **发一条消息** → 收到该 `PAUSED` Run 的 `run_completed`，`api/run/get` 的 `status = COMPLETED`；判定动作以「占用 + 发消息」为唯一路径，不用 sleep 猜窗口 | 需授权 |
| R-2 | 触发条件与不触发条件 | 用例 `resume.trigger`：① 只 `occupancy/create` 不发消息 → `PAUSED` 的 `status` / `updatedAtMs` / `resumeCount` 逐字段不变；② 占用 + 发消息 → `PAUSED` 被推进（`resumeCount` +1）；③ 全仓检索 `api/run/resume` 与 `resume_on_occupy` 命中数为 0 | 需授权 |
| R-3 | 重复触发幂等 | 用例 `resume.idempotent`：同一 `PAUSED` Run 在两条消息连续到达时只被恢复一次（`resumeCount` 只 +1），期间 `tool_calls` 行数不增加 | 需授权 |
| R-4 | 不重跑已提交轮次 | 用例 `resume.crash`：恢复后 `api/run/history` 的 `steps` 序号与崩溃前衔接（不重置为 1），且已存在的助手消息内容逐字符不变 | 需授权 |
| R-5 | 未配对调用一律不重放 | 用例 `resume.notify`：已配对 → 不执行（`tool_calls` 行数不变）；`PENDING` → 不执行，落 1 条 `ERROR` 审计 + 1 条 TOOL 结果消息，文本含「上次执行被中断：」；旧行无 `PENDING` → 同第二类；两类未配对的工具调用次数均为 0 | 需授权 |
| R-6 | 损坏不可恢复 | 用例 `resume.broken`：五类损坏（序号缺口、消息缺失、非末组缺结果、`PENDING` 不在末组、phase 与轨迹不符）各自落 `FAILED` 且文案互不相同 | 需授权 |
| R-7 | 旧 Run 无检查点 | 用例 `resume.broken`：升级前建的、无检查点的 Run 在发消息后被拒绝恢复（不进入执行），`api/run/history` 的 `steps` 为空且 `interruptedReason` 给出 `NoCheckpoint` 固定文案 | 需授权 |
| R-8 | 事件文件不是恢复前提 | 用例 `resume.crash`：强杀后**删除**该会话的事件目录再重启、占用、发消息，恢复仍成功（证明恢复只读 SQLite）；`api/run/history` 不因事件文件缺失而变化 | 需授权 |
| R-9 | 恢复期间取消仍终态 | 用例 `resume.terminal`：恢复执行中调 `api/run/cancel` → `CANCELLED`，不再产生任何后续事件，`resumeCount` 不再增长 | 需授权 |

## 6. 持久化与迁移（P-1 ~ P-5）

| 编号 | 条目 | 验证方式 | 授权 |
|---|---|---|---|
| P-1 | 迁移幂等 | 用例 `persist.migration`：同一数据库连续执行两次迁移，两次退出码 0，`run_checkpoints` 只有一张、`runs` 新列不重复 | 需授权 |
| P-2 | 旧库可升级 | 用例 `persist.migration`：用 `0001` + `0002` 的数据（含终态 Run、工具审计行、消息）升级到 `0003`，逐字段读回相等；`resumeCount = 0`、`interruptedReason` 与 `lastCheckpointSeq` 为 `NULL` | 需授权 |
| P-3 | 新列对旧逻辑无害 | 用例 `persist.migration` 与 `run` / `tool` 既有用例：未写新列的路径（普通 Run、旧审计行）行为与改动前逐字段一致 | 需授权 |
| P-4 | 升级不可回滚（人工） | 人工判定动作：确认 `crates/lib-db/migrations/` 只有前进迁移、无 down 脚本；确认任务结案说明里写「升级前备份 `agent.db`」；把回滚策略登记为 6E 输入 | 不需授权 |
| P-5 | 检查点不无限增长 | 用例 `resume.checkpoint`：Run 落终态后 `api/run/history` 的 `steps` 条数 ≤ `keep_last`；`metrics` 的 `checkpointsPruned` 与删除条数相等 | 需授权 |

## 7. 接口与客户端（I-1 ~ I-5）

| 编号 | 条目 | 验证方式 | 授权 |
|---|---|---|---|
| I-1 | 接口清单 | 静态核对 `crates/lib-web/src/session/api.rs` 的 `#[web_api_*]` 新增恰好**一条**（`api/run/history`）；`api/run/resume` 不存在；`make gen` 输出「路由 +1 / 接口 +1」 | 需授权 |
| I-2 | `history` 语义 | 用例 `resume.history`：`steps` 按 `seqNo` 升序、字段只有 `seqNo` / `phase` / `toolRound` / `createdAtMs`（无 payload 原文）；无检查点 Run 返回空数组；未知 Run 返回 404 | 需授权 |
| I-3 | 恢复的事件时间线与顺序 | 用例 `resume.trigger`：一次发消息同时触发「恢复旧 Run」与「新建 Run」，SSE 上恢复 Run 的 `run_completed` 序号早于新 Run 的 `run_started`；`api/run/history` 显示 `resumeCount` 与「上下文重新组装」标记（若实现为字段则断言字段，若为文案则断言文案原样出现），不承诺上下文逐字节一致 | 需授权 |
| I-4 | TUI | 用例 `resume.tui`：`PAUSED` 显示为「已中断（发送消息可继续）」+ `interruptedReason` 原样；中断态下输入框可用；发消息后按事件里的 `runId` 分别更新两条 Run | 需授权 |
| I-5 | WebUI | 用例 `resume.web`（含前端单元用例）：面板显示 `PAUSED` 与原因、给出「发送消息可继续」提示、不出现恢复按钮；客户端源码检索 `api/run/resume` 命中数为 0；`pnpm exec tsc --noEmit` 通过 | 需授权 |

## 8. 可观测（O-1 ~ O-4）

| 编号 | 条目 | 验证方式 | 授权 |
|---|---|---|---|
| O-1 | `resume` 段字段 | 用例 `resume.metrics`：`api/probe/metrics` 的 `resume` 段含六个字段（`checkpointsWritten` / `checkpointsPruned` / `resumeAttempts` / `resumeSucceeded` / `resumeRejected` / `resumeUnrecoverable`） | 需授权 |
| O-2 | 数值可复算 | 用例 `resume.metrics`：`checkpointsWritten` 等于 `api/run/history` 的 `steps` 条数；`resumeSucceeded` 等于 `api/run/get` 的 `resumeCount` 之和；`resumeUnrecoverable` 等于该轮 `resume.broken` 场景数 | 需授权 |
| O-3 | 未开特性显式「未采集」 | 未开 `metrics` 的构建下 `api/probe/metrics` 不注册 → 报告写「未采集：接口不存在」，不写成「全 0」也不静默跳过 | 需授权 |
| O-4 | 日志不是数据源 | 静态核对：`crates/bin-tests/src/cases/resume*` 无 `server.log` / `read_to_string` 日志读取；`git diff` 无新增的采集型 `warn!` / `debug!` | 不需授权 |

## 9. 错误与边界（E-1 ~ E-5）

| 编号 | 条目 | 验证方式 | 授权 |
|---|---|---|---|
| E-1 | 未占用不恢复 | 用例 `resume.trigger`：未占用时发消息 → 新 Run 停在 `QUEUED`（既有行为），`PAUSED` Run 逐字段不变；占用后消息才推进恢复 | 需授权 |
| E-2 | 只有 `PAUSED` 会被推进 | 用例 `resume.trigger`：同一会话存在 `QUEUED` / `WAITING` / 三个终态的行时发消息，只有 `PAUSED` 行被恢复；其余行的 `status`、`updatedAtMs`、`resumeCount` 逐字段不变 | 需授权 |
| E-3 | 检查点不含敏感内容 | 静态核对 payload 字段白名单 + 用例 `resume.checkpoint`：`api/run/history` 与检查点原始列中检索 `api_key` / `AGENTS.md` 正文串 / 系统提示词串，0 命中 | 需授权 |
| E-4 | 不泄漏绝对路径 | 用例 `resume.broken`：全部失败文案里检索 `AGENT_HOME` 绝对路径，0 命中 | 需授权 |
| E-5 | 未知 Run | 用例 `resume.history`：`api/run/history` 对不存在的 `runId` 返回 404 + 统一错误契约 `ApiError`；`api/message/create` 对不存在的 `sessionId` 仍是既有 404 口径（未被恢复逻辑改变） | 需授权 |

## 10. 回归与兼容（K-1 ~ K-6）

| 编号 | 条目 | 验证方式 | 授权 |
|---|---|---|---|
| K-1 | Skill 语义未变 | `make test ARGS="skill"` 全绿（P5 的 15 条用例），且请求体里索引小节的构造口径未变（`skill.index` / `skill.budget` 断言原样通过） | 需授权 |
| K-2 | 工具审计兼容 | `make test ARGS="tool"` 全绿（既有 28 条）；`tool.audit` 的返回字段除新增 `startedAtMs` 外逐字段不变 | 需授权 |
| K-3 | 自愈口径变更已登记 | 静态核对 `persist.running_heal` 的断言已从 `FAILED` 改为 `PAUSED`，且该改动在任务结案说明里显式登记为「口径变更」；`make test ARGS="persist"` 全绿 | 需授权 |
| K-4 | Run 与运行时未回归 | `make test ARGS="run"`、`ARGS="runtime"` 全绿 | 需授权 |
| K-5 | 全量回归 | `make test ARGS="--all"` 全绿，用例总数 = 138（P5 基线）+ 6A 新增数，两个数字都写进报告 | 需授权 |
| K-6 | 事件类型未新增 | 静态核对 `RunEventKind` 的取值清单与 P5 结案时逐字相同（`git diff crates/agent-core/src/session/event.rs` 为空）；`PAUSED` 只通过 `api/run/get` 的 `status` 可见 | 不需授权 |

---

## 11. 结论口径

| 情形 | 报告怎么写 |
|---|---|
| 用例通过 / 命令退出码 0 / 静态核对成立 | 「成立」+ 证据（用例名或命令或检索表达式） |
| 未取得授权 | 「未授权」+ 待执行的命令或用例名；**不计入通过** |
| 无法采集（构建未开特性、平台不支持、符号链接无权限等） | 「未采集」+ 名称 + 原因；**不计入通过**，并在 §结转风险里给替代证据或留作 `KI-P6A-xxx` |
| 用例失败 | 写明用例名与失败断言；日志路径只作为人工排查线索，不作为数据来源 |

未采集项与结转风险统一登记在阶段报告 `docs/reports/p6a-020-acceptance.md` 的末节，编号沿用 `KI-P6A-xxx`。
