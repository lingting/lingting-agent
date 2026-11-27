# 6A 任务清单

**状态：已完成（20 个任务 / 5 个批次全部结案）。** 子阶段目标与边界见 [README.md](README.md) §一 / §二，判据见 [acceptance.md](acceptance.md)，决策见 [decisions.md](decisions.md)，阶段报告见 [`docs/reports/p6a-020-acceptance.md`](../../../docs/reports/p6a-020-acceptance.md)。

批次之间强序，批次内可并行。**每个任务的「测试要求」只允许在 `crates/bin-tests` 落地用例；其他 Crate 不得写 `#[cfg(test)]`**（仓库 `AGENTS.md` 第 28 条）。**所有需要执行用例的步骤都必须先取得明确授权**；未授权时只推进代码、静态检查与文档部分，相关验收项保持未勾选。

本子阶段的四条硬线：

1. **恢复只读 SQLite**：事件文件是重放与排障通道，不是恢复依据（D-P6-002）。
2. **检查点只落在已提交的边界**：模型轮次或工具批次没有提交，就绝不写检查点；反之，写了检查点就等于「到此为止的进度都已落库」。
3. **终态唯一**：任何路径都不得改写 `COMPLETED` / `CANCELLED` / `FAILED`；恢复成功与正常完成走同一条收尾事务（D-P6-005）。
4. **副作用不重复**：恢复时**一次工具调用都不重跑**；未配对的调用一律回灌「上次执行被中断：<简短原因>」，是否重新发起由模型决定（D-P6-006 / D-P6-007）。

---

## Batch A：事实源、状态机与检查点存储（P6A-001 ~ P6A-004）

> 依赖：无。本批次只造地基：状态机有可恢复中间态、`0003` 迁移可重复执行、检查点能写能读且带格式版本。此时运行时还没有任何检查点写入点，也没有恢复触发路径。
>
> **Batch A 结案（已执行）**：P6A-001 / 002 / 003 全部落地并验收；P6A-004 的格式版本与两类解码结果已落地，「三类固定文案」按需求方决定结转到 P6A-005 / P6A-007（Batch A 没有使用点，提前定义会被 `clippy -D warnings` 的 `dead_code` 判失败），因此 P6A-004 保持未勾。
>
> 验收证据：`cargo check --workspace --all-targets`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全部 0 退出；`make gen` 路由 / 接口各 32 条（未新增）；用例 `persist.migration`（`0003` 幂等 + 旧库升级 + 账本 `[1, 2, 3]` + 落地形态）与新增的 `resume.checkpoint`（序号 / 载荷 / 保留策略 / 解码 / 指针同事务）通过。
>
> 需要回填的口径差异：（1）生成物比 P6A-019 的白名单多两项——`Run` 的三个新字段与 `CheckpointPhase` 枚举（`RunStep.phase` 需要它）；（2）`A-3` 的 `run_checkpoints` 检索会命中 `bin-tests` 的注入夹具，该判据应限定到产品 crate；（3）`S-1` 的「无第二处手写终态列表」在 `bin-tests` 的 SDK 类型辅助函数上不成立（生成的枚举没有 `is_terminal()`），按需求方授权保持原样；（4）P6A-003 第 5 条要求「不新增错误体系」，但 P6A-004 要区分 `Unsupported` / `Corrupt`，因此新增一层 `CheckpointError` 包裹 `DbError` 与解码变体，读库失败仍是原 `DbError`。

- [x] **P6A-001 Run 状态机引入可恢复中间态 `PAUSED`**

  **目标**：把「非预期中断」与「已经判死」分开，使崩溃残留的 Run 有一个既非终态、也不能被执行队列自动拾取的状态。

  **前置依赖**：无。

  **涉及文件 / Crate**：`crates/agent-core/src/session/dto.rs`（`RunStatus`）、`crates/agent-core/src/session/`（状态集合辅助函数）、`crates/service-core/src/repository/run.rs`（条件更新与自愈 SQL）、`crates/service-core/src/session/service.rs`（`cancel` 的前置状态集合）、生成物 `packages/sdk-rust` / `packages/sdk-ts`（`make gen`）。

  **实施步骤**：
  1. `RunStatus` 增加 `Paused`（wire 值 `PAUSED`），文档注释写明「非终态、只能由启动自愈产生、只能由『占用 + 发消息』这条触发路径消费」。
  2. 状态集合收敛到一处：`is_terminal()`（`Completed` / `Cancelled` / `Failed`）、`is_active()`（`Queued` / `Running` / `Waiting` / `Paused`）、`can_cancel()`（四个非终态）。禁止在调用点各自写 `matches!`。
  3. 条件更新按新集合重算：`start_run` 只接受 `QUEUED`；`cancel` 接受四个非终态并统一落 `CANCELLED`；终态写入仍然 `WHERE status = 'RUNNING'`（或 `'WAITING'`），影响 0 行即不写。
  4. 事件侧不加新事件类型：`PAUSED` 只通过 `api/run/get` 的 `status` 字段可见（`K-6`）。
  5. `docs/api/session.md` 的字段取值表需要同步——**本轮不改文档**，把待同步清单记在任务结案说明里。（结案：`docs/api/session.md` 已同步 `PAUSED`、`Run` 的三个新字段与「Run 状态机与检查点」章节；`docs/develop.md` 同步了状态集合与检查点约定。）

  **验收标准**：
  - [x] `RunStatus` 的既有六个取值与 wire 值逐字不变，新增值只有 `Paused`。
  - [ ] 全仓 `matches!(… RunStatus …)` 的终态判断都走 `is_terminal()`；`grep` 无第二处手写终态列表。（结案：产品 crate 成立；`bin-tests` 的 SDK 类型枚举没有 `is_terminal()`，两处辅助函数保留手写列表，按需求方授权不动。）
  - [x] `start_run` 对 `PAUSED` 返回「未推进」（影响 0 行），不产出事件。
  - [x] `make gen` 产出的两份 SDK 含 `PAUSED`；`cargo check --workspace --all-targets` 通过。

  **测试要求**：本任务不新增用例。`persist.running_heal` 的口径变更随 P6A-009 一起改写并申请授权；本任务只做 `cargo check` / `make check` / `make gen` 这类静态门禁。（结案：本任务确实未新增用例。）

- [x] **P6A-002 迁移 `0003`：检查点表、Run 恢复列与工具开始标记**

  **目标**：检查点、中断原因、恢复次数、工具「开始标记」有落点；迁移幂等、可用 `PRAGMA` 预检、旧库升级后既有行可读。

  **前置依赖**：P6A-001。

  **涉及文件 / Crate**：`crates/lib-db/migrations/0003_checkpoints.sql`（新）、`crates/lib-db/src/migrate.rs`（迁移登记）、`crates/service-core/src/repository/run.rs` 与 `tool.rs`（行映射与列清单）。

  **实施步骤**：
  1. 新表：
     ```sql
     CREATE TABLE IF NOT EXISTS run_checkpoints (
         id               TEXT    PRIMARY KEY,
         run_id           TEXT    NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
         seq_no           INTEGER NOT NULL,
         phase            TEXT    NOT NULL,
         tool_round       INTEGER NOT NULL,
         last_message_seq INTEGER NOT NULL,
         payload          TEXT    NOT NULL,
         format_version   INTEGER NOT NULL,
         created_at_ms    INTEGER NOT NULL,
         UNIQUE (run_id, seq_no)
     );
     CREATE INDEX IF NOT EXISTS idx_checkpoints_run ON run_checkpoints (run_id, seq_no);
     ```
  2. `runs` 追加 `interrupted_reason TEXT`、`resume_count INTEGER NOT NULL DEFAULT 0`、`last_checkpoint_seq INTEGER`（`ADD COLUMN` 由迁移 runner 的 `PRAGMA table_info` 预检，保证幂等）。
  3. `tool_calls` 追加 `started_at_ms INTEGER`（`PENDING` 行有值，执行完成后保留原值不覆盖）。
  4. 枚举列（`phase`）继续存 serde wire 值、不加 `CHECK`（沿用 `0001` 的约定，P3 加枚举值不需要迁移）。
  5. 行映射与查询：`run.rs` 的 `Run` 映射补三个新列；`tool.rs` 的记录映射补 `started_at_ms`（缺省 `None`）。

  **验收标准**：
  - [x] 空库全量迁移成功；连续执行两次迁移不报错、不重复建表。
  - [x] 用 `0001` + `0002` 建出的旧库升级后：`runs` / `messages` / `tool_calls` 的既有列逐字段可读，新列为 `NULL`（`resume_count` 为 `0`）。
  - [x] `cargo check -p lib-db -p service-core` 通过；`crates/lib-db/migrations/` 下不存在修改过的 `0001` / `0002`。
  - [x] `run_checkpoints` 的 `UNIQUE(run_id, seq_no)` 存在（重复序号写入直接报错，不静默覆盖）。

  **测试要求**：本任务不写用例。迁移与升级由 P6A-016 的 `resume.checkpoint` / `persist.migration` 覆盖（结案：`persist.migration` 已扩到 `0003` 并验收）。

- [x] **P6A-003 检查点写入与读回（唯一入口）**

  **目标**：检查点的追加、读回与清理各只有一个入口，事务边界经 `lib-db` 产生，序号由存储层给出而不是调用方猜。

  **前置依赖**：P6A-002。

  **涉及文件 / Crate**：`crates/service-core/src/repository/checkpoint.rs`（新）、`crates/service-core/src/repository/mod.rs`、`crates/service-core/src/session/service.rs`、`crates/service-core/src/lib.rs`。

  **实施步骤**：
  1. `append(run_id, checkpoint) -> Result<i64>`：在同一事务里「插入检查点行 + 更新 `runs.last_checkpoint_seq`」，返回本次 `seq_no`；插入冲突按 `DbError` 上抛，不重试。
  2. `latest(run_id) -> Result<Option<RunCheckpoint>>`（按 `seq_no` 倒序取一条）、`list_of_run(run_id) -> Result<Vec<RunCheckpoint>>`（按 `seq_no` 升序）。
  3. `prune_terminal(run_id, keep_last) -> Result<usize>`：Run 落终态时保留最近 `keep_last` 条、删除其余（`keep_last` 由装配层传，缺省 20）。
  4. `payload` 只允许「恢复所需的最小事实」：phase、轮次、`last_message_seq` 上界、工具组摘要（`tool_call_id` + 状态）。禁止放上下文全文、系统提示词、Provider 原始响应。
  5. 任一读库失败都映射成既有 `DbError`，不新增错误体系。

  **验收标准**：
  - [x] 同一 Run 的 `seq_no` 从 1 开始连续递增；人为制造缺口后，读回结果能被上层判为损坏（不静默跳过）。（结案：序号连续由 `resume.checkpoint` 验收；「缺口判损坏」已随 P6A-007 落地，由 `resume.broken` 的 `RESUME_GAP` 分支取证。）
  - [x] 插入失败时 `runs.last_checkpoint_seq` 保持原值（同事务回滚）。
  - [x] `payload` 的序列化结果不含 `api_key`、不含系统提示词全文、不含 `AGENTS.md` 内容（静态核对序列化结构与调用点）。
  - [x] Run 落终态后 `list_of_run` 的条数 ≤ `keep_last`。

  **测试要求**：由 P6A-016 的 `resume.checkpoint` 覆盖（序号连续性、事务回滚、清理策略）。（结案：存储半段已由新增的 `resume.checkpoint` 覆盖并通过。）

  **复核补正（清理路径）**：原实现只在 `SessionService::cancel_run` 里事后调一次清理，`COMPLETED` / `FAILED` 两条收尾路径不清理，与「Run 落终态时按策略删除检查点」不符。现已收敛为「裁剪挂在推进终态的事务里」：`Repository::prune_checkpoints_in`（唯一实现）由 `with_run_completion` / `fail_run` / `cancel_run` 在同一事务内调用，`cancel_session_runs`（会话停止收尾）另有会话级版本；`SessionService::prune_checkpoints` 的事后入口已删除，改为只读的 `checkpoints_of_terminal_run`。

- [x] **P6A-004 检查点格式版本化与不识别版本的处置**

  **目标**：检查点格式有版本号，遇到不认识的版本不猜测、不静默降级，直接判「不可恢复」。

  **前置依赖**：P6A-003。

  **涉及文件 / Crate**：`crates/agent-core/src/session/checkpoint.rs`（新：`RunCheckpoint` / `CheckpointPhase` / `CHECKPOINT_FORMAT_VERSION`）、`crates/agent-core/src/session/mod.rs`、`crates/service-core/src/repository/checkpoint.rs`（编解码）、`crates/agent-runtime/src/error.rs`（固定文案）。

  **实施步骤**：
  1. 领域类型：`CheckpointPhase`（`ModelDone` / `ToolsDone` / `AwaitingConfirm`，wire 值 `MODEL_DONE` / `TOOLS_DONE` / `AWAITING_CONFIRM`）；`RunCheckpoint { seq_no, phase, tool_round, last_message_seq, payload, format_version }`。
  2. `CHECKPOINT_FORMAT_VERSION: u32 = 1` 只定义一次，写入时同时进列与 payload。
  3. 解码规则：`== 当前版本` 严格解码（未知字段忽略、缺字段报错）；`> 当前版本` 返回 `Unsupported`；`== 0` 或缺失返回 `Corrupt`。三类各自固定文案，集中在 `error.rs`。
  4. 上层契约：`Unsupported` / `Corrupt` 一律映射为「不可恢复」，Run 落 `FAILED` 并写固定原因；**不降级成「从头重跑」**。
  5. 不留兼容分支：不为未来版本预留 `match … => 猜测` 的兜底。

  **验收标准**：
  - [x] `CHECKPOINT_FORMAT_VERSION` 全仓只出现一处定义。
  - [x] 三类错误各有独立文案，文案里不含绝对路径、不含 payload 内容。（结案：`error.rs` 的 `CHECKPOINT_UNSUPPORTED` / `CHECKPOINT_CORRUPT` 与五条 `RESUME_*` 各自独立；`resume.broken` 逐条断言文案，`E-4` 的绝对路径检索命中 0。）
  - [x] 静态核对：`Unsupported` 与 `Corrupt` 的处置路径相同且都不进入恢复执行。

  **测试要求**：由 P6A-016 的 `resume.broken` 覆盖（未知版本、缺字段、序号缺口三类）。（结案：未知版本与缺字段已由 `resume.checkpoint` 覆盖；序号缺口属 P6A-007。）

---

## Batch B：循环内检查点与重复执行边界（P6A-005 ~ P6A-008）

> 依赖：Batch A。本批次结束时，一次正常 Run 的每个已提交边界都有检查点；工具执行有「开始标记」；恢复前能判定「可恢复 / 损坏」；未配对调用一律不重放并产出中断通知。此时还**没有**恢复触发路径，也还没改自愈。

> **Batch B 结案（已执行；范围收敛到 P6A-005 / 006）**：按需求方决定，P6A-007（恢复点解析）与 P6A-008（中断通知）结转到 Batch C 的恢复入口（P6A-010）一起落地——本批次内它们没有调用方，提前定义会被 `clippy -D warnings` 的 `dead_code` 判失败（与 Batch A 结转 P6A-004 文案同一做法）。
>
> 验收证据：`cargo check --workspace --all-targets`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全部 0 退出；`make gen` 路由 / 接口各 32 条（未新增）；`make test ARGS="--all"` **139 个用例全通过**（含 `resume.checkpoint` 的运行时三段与 `tool` / `mcp` / `skill` / `run` / `persist` / `web` / `tui` 回归）。
>
> 口径变更（都由 P6A-006 的「执行前写开始标记」引起，已在用例注释里登记）：
>
> 1. `tool.confirm` 的 F-1 与 `mcp.confirm`：挂起期间「审计查询能看到 `PENDING` 行」不再成立——查询口径按 D-P6-008 过滤 `PENDING`，断言改为直接读库内行。
> 2. `tool.cancel` 与 `mcp.cancel`：「取消的批次不落库（审计为空）」不再成立——取消的批次会留下一条收尾为 `CANCELLED`、没有结果的开始标记。
>
> 需要回填的口径差异：
>
> 1. P6A-005 第 1 条要求起点 `MODEL_DONE` 的 payload 记 `assistant_message_id`：本轮**未加**该字段（`CheckpointPayload` 仍是「版本 + 工具组摘要」），因为它的唯一使用点是 P6A-010 的重发 `run_started`；届时应与恢复入口一起加，避免现在加一个没有读取点的字段。**（已回填）** 该字段已随复核补上：`CheckpointPayload.assistant_message_id` 只在起点检查点（`tool_round = 0` 的 `MODEL_DONE`）有值，`ResumePoint.assistant_message_id` 从第 1 条检查点取回，`drive` 重发的 `run_started` 与最终收尾都用它，客户端因此不会因为恢复多出一个助手气泡。
> 2. 工具边界的条数口径：一轮工具提交后写 `MODEL_DONE` 与 `TOOLS_DONE` 各一条，因此「两轮工具 + 正常完成」= 5 条（起点 1 + 每轮 2），与 P6A-018 的数字一致。
> 3. `append_checkpoint` 的序号改为「插入语句内现算 + `RETURNING seq_no`」：原实现先 `SELECT` 再 `INSERT`，事务以读快照起步，并发 Run 时升级写锁会直接拿到 `SQLITE_BUSY_SNAPSHOT`（不等 `busy_timeout`）；`run.concurrency` 因此暴露过一次「检查点写入失败」。属实现细节，不改验收口径。
> 4. `make ui-check` 仍是既有失败：`Run` 的三个新字段自 Batch A 起没有同步到 UI 测试夹具（`tests/tool-events.test.tsx` / `tests/use-chat.test.tsx` 缺 `resumeCount` 等），与本次改动无关，留给 P6A-019 的门禁收口。

- [x] **P6A-005 轮次边界的检查点写入**

  **目标**：每个「模型轮次完成」「工具批次提交」「工具确认挂起」都留检查点，且检查点只在对应事务已提交之后出现。

  **前置依赖**：P6A-003、P6A-004。

  **涉及文件 / Crate**：`crates/agent-runtime/src/loop_.rs`、`crates/agent-runtime/src/loop_/tool.rs`、`crates/agent-runtime/src/loop_/request.rs`（无逻辑改动，仅确认 `Final` 的落点）、`crates/service-core/src/session/service.rs`（追加检查点的业务方法）。

  **实施步骤**：
  1. `run_started` 之后写第一条 `MODEL_DONE`：`tool_round = 0`、`last_message_seq` = 当前会话最大 `seq`、payload 记 `assistant_message_id`。
  2. 每轮 `request::stream` 返回 `Final` 后先走既有收尾事务（助手消息 + 终态），再写检查点；工具轮次则在 `run_round` 的 `append(messages)` 提交后写 `TOOLS_DONE`（`tool_round` 为本轮序号）。
  3. 确认挂起：`suspend()` 的「`record_tool_call(pending)` + `RUNNING → WAITING`」提交后写 `AWAITING_CONFIRM`。
  4. 检查点写失败一律 `FAILED`（D-P6-010），原因走 `error::CHECKPOINT` 单一文案；**不**降级为「继续跑但没检查点」。
  5. 取消路径不写检查点（D-P6-005 的终态路径）。

  **验收标准**：
  - [x] 三种 phase 各只有一个构造函数（可被多次调用，但构造与追加只有一处）（静态核对调用点数量）。（结案：构造与追加只有 `loop_/checkpoint.rs` 的 `append` 一处，`RunCheckpoint::new` 全仓只在它里面出现。）
  - [x] 检查点写入点全部位于对应提交之后：先 `append` / 终态事务提交，再 `append_checkpoint`。（结案：四处写入点都在 `run_started` 发出后、`append(messages)` 提交后、`suspend_run` 提交后、唤醒收尾的 `append(messages)` 提交后。）
  - [x] 一次「两轮工具 + 正常完成」的 Run，检查点条数与「起点 + 每轮模型边界 + 每轮工具边界」之和相等，且 `seq_no` 连续。（结案：`resume.checkpoint` 断言 5 条、序号 1..5、phase 依次 `MODEL_DONE` / `MODEL_DONE` / `TOOLS_DONE` / `MODEL_DONE` / `TOOLS_DONE`，`lastCheckpointSeq = 5`。）
  - [x] 检查点写失败时 Run 落 `FAILED`，`runs.error` 是固定文案，且**不产生**助手消息。（结案：`resume.checkpoint` 的注入段——触发器让 `run_checkpoints` 插入失败，`run_failed` 与 `run/get` 的 `error` 都是「检查点写入失败」，该 Run 的检查点 0 条、消息里没有助手消息。）

  **测试要求**：由 P6A-016 的 `resume.checkpoint` 覆盖（正常 Run 的检查点清单与条数、写失败路径由注入式失败构造）。（结案：已授权执行，运行时三段全绿。）
- [x] **P6A-006 工具调用的「开始标记」前移**

  **目标**：崩溃时能区分「这次调用从未开始」与「可能已经执行」——这是中断通知口径与审计完整性的唯一事实来源（恢复时**不放行任何重放**，见 P6A-008）。

  **前置依赖**：P6A-002。

  **涉及文件 / Crate**：`crates/agent-runtime/src/loop_/tool.rs`（`audit()` 的拆分与前移）、`crates/service-core/src/repository/tool.rs`（`insert_pending` / `finish`）、`crates/agent-tool/src/audit.rs`（`ToolCallRecord::pending` 复用，`started_at_ms` 字段）、`crates/agent-tool/src/executor.rs`（只透传，不改重试与超时）。

  **实施步骤**：
  1. 执行前：把本轮全部调用以 `PENDING` + `started_at_ms = now` 一次性写入 `tool_calls`（`INSERT` 冲突按数据不一致处理）。
  2. 执行后：按 `(run_id, tool_call_id)` **条件更新**为最终状态、结果与 `duration_ms`（`WHERE status = 'PENDING'`）；影响 0 行即数据不一致 → `FAILED`（`error::TOOL_TRACE_BROKEN` 复用）。
  3. 兼容旧行：`started_at_ms IS NULL` 的行按旧语义解释为「执行后写入」，恢复时按「未开始即中断」处理；判断只看这一列，不看列以外的东西。
  4. 事件顺序不变：`tool_call_started` 仍在真正执行之前发出；`tool_call_completed` 仍在结果落库之后发出。
  5. `ToolCallRecord` 增加 `started_at_ms: Option<i64>`，审计查询映射同步（可见性由 D-P6-008 定）。

  **验收标准**：
  - [x] 正常路径的 `tool_calls` 行在结果、时长、`tool_round` 上与改动前逐字段相等（旧用例断言不变）。（结案：`make test ARGS="tool"` 28 条、`ARGS="mcp"` 19 条全绿，断言口径未放宽。）
  - [x] 模拟「写入后、更新前崩溃」：库里留下 `PENDING` 行且 `started_at_ms` 有值。（结案：以挂起段等价取证——`resume.checkpoint` 断言库里是 `PENDING`、`started_at_ms` 有值且没有结果；真正强杀留给 `resume.crash`。）
  - [x] `WHERE status = 'PENDING'` 的条件更新是唯一收尾路径，全仓无第二处无条件 `UPDATE tool_calls SET status`。（结案：`Repository::finish_tool_call` 只命中 `PENDING`；`set_tool_call_status_in` 与取消链路的批量收尾也都带 `AND status = 'PENDING'`。）
  - [x] `agent-tool` 的错误分类、重试与超时判定零改动（`git diff` 限定到该 crate 时只有 `audit.rs` 的字段与 `executor.rs` 的透传）。（结案：只有 `audit.rs` 改了字段——`pending()` 写开始标记、`ToolCallVO` 新增 `startedAtMs`；`executor.rs` 零改动。）

  **测试要求**：由 P6A-016 的 `resume.checkpoint`（`PENDING` 残留）与既有 `tool.audit` 回归覆盖；回归执行需授权。（结案：已授权执行，`resume.checkpoint` 的开始标记段与 `tool` / `mcp` 回归全绿；两条旧断言的口径变更见上方 Batch B 结案说明。）

- [x] **P6A-007 恢复点解析：检查点与工具轨迹的一致性校验**（结案：已随 P6A-010 一起落地——判定为端口方法 `RunDriver::plan_resume`，实现在 `loop_/checkpoint::plan`）

  **目标**：恢复前先判定「能不能恢复」。可恢复 → 产出确定的恢复点；不可恢复 → 直接终态，绝不半恢复。

  **前置依赖**：P6A-005、P6A-006。

  **涉及文件 / Crate**：`crates/agent-runtime/src/loop_/checkpoint.rs`（新：解析与判定）、`crates/agent-runtime/src/loop_/trace.rs`（复用/扩展 `recover`）、`crates/agent-runtime/src/error.rs`（五类固定文案）、`crates/agent-core/src/session/checkpoint.rs`。

  **实施步骤**：
  1. 读最近检查点 + `messages` / `tool_calls`；产出 `ResumePoint { checkpoint_seq, phase, round, unpaired }`（定义在 `agent-core`，`RunInput` 要携带它，`service-core` 不能反向依赖运行时）。
  2. 五项校验（任一不满足即 `Unrecoverable`）：① `seq_no` 无缺口且 `runs.last_checkpoint_seq` 指向最近一条；② `last_message_seq` 上界那条消息仍在；③ 最后工具组之外的组不缺结果（沿用 D-P3-023）；④ `PENDING` 行只出现在最后检查组（或尚未提交的那一轮）；⑤ 检查点记录的 `tool_round` 与已提交工具组数一致。
  3. 判定结果三态：`Recoverable(ResumePoint)` / `Unrecoverable(reason)` / `NoCheckpoint`。`NoCheckpoint` 只对「升级前的旧 Run」出现（`R-7`）。
  4. `Unrecoverable` 在装配层落 `FAILED` 并写固定原因；**不重试、不降级成从头执行**（本任务只产出判定，落终态的调用点在 P6A-011 的队列）。
  5. 与 P3 既有 `trace::recover` 的关系：保留它对「挂起确认唤醒」的用法，恢复路径改成「先 `checkpoint::plan` 再 `trace::recover`」，两处读的是同一份数据，`drive` 再用恢复点与轨迹逐项比对一次。

  **验收标准**：
  - [x] 五类损坏各有独立固定文案，且都不进入执行路径。（结案：`error.rs` 五条独立文案 `RESUME_GAP` / `RESUME_MESSAGES_LOST` / `RESUME_TRACE_BROKEN` / `RESUME_PENDING_LEAK` / `RESUME_PHASE_MISMATCH`；解码失败另有 `CHECKPOINT_UNSUPPORTED` / `CHECKPOINT_CORRUPT`。判定只产出结论，不进入执行；「落 `FAILED`」的端到端证据待 P6A-011 / P6A-016。）
  - [x] 可恢复场景的 `ResumePoint` 字段与最近检查点逐字段相等（`phase` / `round` / `last_message_seq`）。（结案：`ResumePoint` 直接由 `latest` 的 `seq_no` / `phase` / `tool_round` 构造，`unpaired` 取自同一次轨迹读回。）
  - [x] 全仓不存在「部分恢复」分支（先落一半状态再继续执行）。（结案：判定三态互斥，`Recoverable` 之外没有任何写库动作；`drive` 在推进前用恢复点与轨迹逐项比对，不一致即 `FAILED`。）
  - [x] `NoCheckpoint` 与 `Unrecoverable` 的处置不同：前者保留 Run 可用于「放弃恢复 → 终态」，后者直接终态。（结案：两个独立变体，`NoCheckpoint` 不携带原因也不改行；`R-7` 的 `interruptedReason` 文案属 `api/run/history`，随 P6A-013 落地。）

  **测试要求**：由 P6A-016 的 `resume.broken` 覆盖（五类损坏 + `NoCheckpoint`）。（结案：用例尚未落地，`resume.broken` 随 P6A-016 一起写；本次已授权执行的回归为 `resume` 1 条、`tool` 28 条、`mcp` 19 条、`run` 10 条、`persist` 5 条，全部通过。）

- [x] **P6A-008 恢复不重放：中断通知与简短原因**（结案：已随 P6A-010 一起落地——恢复分支的唯一入口是 `tool::resume_interrupted`，不碰 `ToolExecutor`）

  **目标**：把「恢复时一次工具调用都不重跑」写死成可检规则，并把「上次没跑完」这件事以固定文本告知模型；`agent-tool` 的契约保持不变。

  **前置依赖**：P6A-006、P6A-007。

  **涉及文件 / Crate**：`crates/agent-runtime/src/loop_/tool.rs`（恢复分支的唯一分支点）、`crates/agent-runtime/src/loop_/checkpoint.rs`（判定输入）、`crates/agent-runtime/src/error.rs`（通知文本与原因白名单）、`crates/service-core/src/session/service.rs`（写回灌结果的审计行与 TOOL 消息）、`crates/agent-core/src/session/dto.rs`（`Run::interrupted_reason` 的读取）。**`agent-tool` / `agent-mcp` 不改。**

  **实施步骤**：
  1. 恢复执行规则（唯一分支点，全仓只有这一处）：
     - 已配对（有 TOOL 消息）→ 不执行，直接进轨迹；
     - 未配对 + `status = PENDING` → **不执行**；落一条 `ERROR` 审计 + 一条 TOOL 结果消息，文本 `上次执行被中断：<简短原因>`；
     - 未配对且无 `PENDING` 行（旧库行）→ 同上（文本不变，不做「可能已执行」之外的区别）。
  2. `<简短原因>` 由 `error.rs` 的一个白名单函数从 `runs.interrupted_reason` 映射得到（当前只有「服务重启」一类），**不拼接**工具名以外的内容、不带调用参数原文、不含绝对路径、不含日志片段。
  3. 审计状态复用 `ERROR`，**不新增** `ToolCallStatus` 取值；`tool_call_completed` 事件照常发出（带同一条文本）。
  4. 恢复后首个 `run_started` 事件的 `error` 字段复用同一句简短原因（不新增事件类型）。
  5. 明确不引入：`Tool::replayable()` 之类的新自述、工具侧幂等键协议、补偿事务。

  **验收标准**：
  - [x] 全仓不存在「恢复时重放工具」的分支：静态核对 `loop_` 内调用 `ToolExecutor` 的路径只有正常执行路径与确认唤醒路径，恢复分支不产生新的执行调用。（结案：`resume_interrupted` 只写审计行与 TOOL 消息，不调用 `executor`；`execute` / `execute_approved` 仍是仅有的两处执行入口。）
  - [x] 三类形态各有用例断言，且「未配对」两种形态的工具调用次数为 0（以 `tool_calls` 行数为准，不读日志）。（结案：`resume.notify` 三种形态各一个被强杀的 Run——已配对 0 条审计、`PENDING` 收尾成 1 条 `ERROR`、旧库行补 1 条 `ERROR`；回灌文本断言「上次执行被中断：」且不含参数原文与 `AGENT_HOME` 路径。）
  - [x] 通知文本里不含调用参数原文、不含 `AGENT_HOME` 绝对路径、不含工具名以外的可变内容。（结案：文本 = 固定前缀 `上次执行被中断` + `error::interrupted_reason_of` 的白名单映射，库里原文不直接拼接；未登记的原因落「原因未知」。）
  - [x] `crates/agent-tool` 与 `crates/agent-mcp` 在本任务下零改动（`git diff` 为空）。（结案：本次改动不涉及这两个 crate。）

  **测试要求**：由 P6A-016 的 `resume.notify` 覆盖（三类形态 + 文本红线），执行需授权。（结案：用例尚未落地；本次已授权执行的回归见 P6A-007 的结案说明。）

---

## Batch C：自愈、恢复触发与装配（P6A-009 ~ P6A-012）

> 依赖：Batch B。本批次结束时，强杀后的 Run 是 `PAUSED` 而不是 `FAILED`；客户端「占用 + 发一条消息」能从最近检查点继续并跑完、且恢复先于该消息的新 Run；取消与优雅关闭仍然只走终态。

- [x] **P6A-009 启动自愈改造：`RUNNING → PAUSED`**

  **目标**：进程消失后的第一次装配把「执行中」判为可恢复，而不是一律判死；同时保持「排队中 / 等确认」的既有判死语义。

  **前置依赖**：P6A-005、P6A-007。

  **涉及文件 / Crate**：`crates/manager-agent/src/assemble.rs`（`HEAL_RUNNING` 的新用途）、`crates/service-core/src/repository/run.rs`（`with_orphan_healing`）、`crates/agent-core/src/session/dto.rs`（`interrupted_reason` 的载体）、`crates/bin-tests/src/cases/persist.rs`（`persist.running_heal` 改写，需授权）。

  **实施步骤**：
  1. 自愈拆成两段语义：`RUNNING → PAUSED`（写 `interrupted_reason = INTERRUPTED_RUNNING`，**不写** `finished_at_ms`）；`QUEUED` / `WAITING → CANCELLED`（保持 `HEAL_QUEUED`，行为不变）。
  2. `PAUSED` 的行必须同时有「最近检查点」或「无检查点」两种形态之一；自愈本身不判断可恢复性（判定留给 P6A-007 的解析，在恢复入口执行）。
  3. 二次重启：已是 `PAUSED` 的行不被自愈覆盖（`WHERE status = 'RUNNING'` 天然保证）；已终态的行同样不动。
  4. 常量与文案归位：`INTERRUPTED_RUNNING` 与旧的 `HEAL_RUNNING` 的关系写进结案说明——旧文案保留给「不可恢复」的终态路径使用。
  5. 探活 `running_run_count` 仍只数 `RUNNING`（`PAUSED` 不是执行中）。

  **验收标准**：
  - [x] 强杀后重启：Run 为 `PAUSED`、`interrupted_reason` 有固定值、`finished_at_ms` 为空。（结案：`persist.running_heal` 断言 `Paused` + 中断原因 + 无结束时间 + `resumeCount = 0`，且不补写助手消息。）
  - [x] 二次重启：`PAUSED` 行逐字段不变（含 `updated_at_ms` 之外的全部列）。（结案：`persist.running_heal` 第二次重启后状态、中断原因、结束时间、`resumeCount`、`lastCheckpointSeq` 全部不变。）
  - [x] `QUEUED` / `WAITING` 的自愈结果与改动前逐字段一致（仍 `CANCELLED` + `HEAL_QUEUED`）。（结案：自愈事务的排队分支零改动；`persist.queued_heal` 通过。）
  - [x] `api/probe/health` 的 `running_run_count` 不把 `PAUSED` 计入。（结案：统计 SQL 仍只数 `RUNNING`，注释写明 `PAUSED` 不计入；`probe.health` 通过。）

  **测试要求**：改写 `crates/bin-tests/src/cases/persist.rs` 的 `persist.running_heal`（断言从 `FAILED` 改为 `PAUSED` + `interrupted_reason`），并新增 `resume.crash` 于 P6A-016。**用例执行需授权**；未授权时任务不勾选。（结案：已授权执行 `make test ARGS="persist"` 5 条、`ARGS="probe"` 4 条、`ARGS="resume"` 1 条、`ARGS="run"` 10 条全部通过；`resume.crash` 按任务要求留给 P6A-016。）

  **结案说明**：
  - 文案归位：`assemble.rs` 的 `HEAL_RUNNING`（「服务重启，执行中的 Run 已终止」）改为 `INTERRUPTED_RUNNING`（「服务重启，执行中的 Run 已中断」），因为它现在是 `runs.interrupted_reason` 而不是终态 `error` 的取值。（已由 P6A-010 回填：「判定不可恢复 → 落 `FAILED`」用的是 `error.rs` 的检查点/恢复固定文案，不再需要旧文案；`HEAL_RUNNING` 已彻底退场。）
  - `with_orphan_healing` 的 `RUNNING` 分支只写 `status` / `interrupted_reason` / `updated_at_ms`，不清 `pending_tool_*`、不动 `finished_at_ms`、不收尾 `PENDING` 工具行——恢复入口要靠这些残留区分「从未开始」与「可能已执行」（D-P6-007）。
  - 装配日志由「残留 Run 已落成终态」改为「残留 Run 已处理（执行中的转为可恢复中断态）」，避免把 `PAUSED` 说成终态。

- [x] **P6A-010 恢复执行路径（运行时侧）**

  **目标**：`drive` 能从 `PAUSED + ResumePoint` 继续，且这一路径与正常路径共用收尾事务，不产生第二条助手消息、不重复已提交的轮次。

  **前置依赖**：P6A-007、P6A-008。

  **涉及文件 / Crate**：`crates/agent-runtime/src/loop_.rs`（恢复分支入口）、`crates/agent-runtime/src/loop_/checkpoint.rs`、`crates/agent-runtime/src/port.rs`（端口方法）、`crates/service-core/src/session/{service.rs,driver.rs}`（`RunInput` 携带 `ResumePoint`、`RESUME` 条件更新）。

  **实施步骤**：
  1. `RunInput` 增加 `resume: Option<ResumePoint>`；`drive` 在「取许可 → 恢复判定（`RunDriver::plan_resume`）→ `PAUSED → RUNNING`（条件更新）→ 组装 Context」之后与正常路径合流。
  2. `PAUSED → RUNNING` 是条件更新（`WHERE status = 'PAUSED'`），影响 0 行即「已被别人恢复或已取消」→ 直接返回，不产出事件（幂等的基础）。
  3. 推进成功时 `resume_count += 1`，与状态推进同一条语句；同时清空 `interrupted_reason`（D-P6-004：只有 `PAUSED` 才有这个值）。
  4. 事件：恢复的 Run 重新发 `run_started`（`error` 字段带同一句中断原因），**不补发**历史 `message_delta`（客户端按 `run_id` + 步骤历史补齐，见 `I-4` / `I-5`）。
  5. 检查点：恢复后继续按 P6A-005 的规则追加，`seq_no` 接着上一次继续（不重置）；恢复路径不补写起点 `MODEL_DONE`（它没有新的已提交边界）。

  **验收标准**：
  - [x] 恢复路径与正常路径的收尾调用是同一函数（静态核对调用点）。（结案：两条路径最后都走 `complete` / `fail` / `cancelled`，恢复分支没有第二套收尾。）
  - [x] 一次「两轮工具后崩溃 → 恢复」的 Run，最终 `messages` 里助手消息只有一条，工具轮次计数与改动前一致。（结案：`resume.crash` 的普通段断言恢复完成后该 Run 的助手消息恰好 1 条；工具轮次口径由 `resume.checkpoint` 的「起点 + 每轮模型边界 + 每轮工具边界」覆盖。）
  - [x] 同一个 `PAUSED` Run 被并发恢复两次：只有一次推进成功，另一次不产出任何事件、不改任何行。（结案：`resume_paused_run` 是单语句条件更新 `WHERE status = 'PAUSED'`，0 行即返回 `RunOutcome::Cancelled` 且不发事件；端到端证据待 P6A-011 的队列与 `resume.idempotent`。）
  - [x] 恢复后的检查点 `seq_no` 与崩溃前连续（无重置回 1）。（结案：序号仍由存储层按 `MAX(seq_no) + 1` 现算，恢复路径不重置也不重编号。）

  **测试要求**：由 P6A-016 的 `resume.crash` / `resume.idempotent` 覆盖，执行需授权。（结案：用例尚未落地；本次已授权执行的回归为 `resume` 1 条、`tool` 28 条、`mcp` 19 条、`run` 10 条、`persist` 5 条，全部通过。）

  **结案说明**（含口径回填）：
  - 恢复判定的落点在端口而不是 `drive`：`RunDriver::plan_resume`（实现在 `loop_::checkpoint::plan`）由队列在拾取 `PAUSED` 时先调用，`drive` 收到的已是算好的 `ResumePoint`，并在推进前用轨迹再逐项比对一次。任务原文把「解析恢复点」写在 `drive` 内，两者等价，但分开更能保证「不可恢复绝不半恢复」。
  - `ResumePoint` 定义在 `agent-core`（`RunInput` 要携带它，而 `service-core` 不能依赖 `agent-runtime`）；三态 `ResumePlan` 定义在 `service-core/session/driver.rs`。
  - `RunOutcome` 未新增变体：`PAUSED → RUNNING` 影响 0 行时沿用既有 `Cancelled { reason: CANCEL_FALLBACK }`，与正常路径 `QUEUED → RUNNING` 影响 0 行的处置逐字相同（不产事件、不改行）。
  - ⑤ 的口径改为「检查点的 `tool_round` 与已提交工具组数一致」：原文「`TOOLS_DONE` 之后不应还有未提交的助手调用」在崩溃窗口里不成立——工具轮次已提交、还没走到 `suspend` 就崩溃时，`TOOLS_DONE` 与未配对调用同时存在，而这是合法的可恢复形态。
  - ④ 允许 `PENDING` 落在最后检查组**或**尚未提交的那一轮（`rounds + 1`）：崩溃可能发生在「开始标记已写、助手调用还没提交」之间，这类行不参与重放，也不构成损坏。
  - 中断通知白名单：库里存的是完整文案（见 P6A-009），`error::interrupted_reason_of` 按前缀白名单映射成「服务重启」，未登记落「原因未知」；审计状态沿用既有取值 `ERROR`，未新增 `ToolCallStatus` 取值。
  - `NoCheckpoint` 的 `interruptedReason` 文案（`R-7`）留给 P6A-013 的 `api/run/history`；本任务只保证它不进入执行、不改任何行。
  - 队列侧仍传 `resume: None`（拾取与排序由 P6A-011 接入），因此恢复路径目前只有「端口判定 + 运行时执行」两段，还没有触发入口。（已由 P6A-011 接入：队列在 `api/message/create` 之后调 `plan_resume` 并把恢复点交给 `drive`。）

- [x] **P6A-011 恢复触发：占用 + 向该会话发一条消息**（代码与静态门禁已落地；两条需要 `resume.trigger` 的验收项仍未勾）

  **目标**：把恢复挂在既有 Run 触发路径上——**不新增接口、不新增配置**；同一批里先恢复 `PAUSED`，再执行 `QUEUED`，顺序写死且可验。

  **前置依赖**：P6A-010。

  **涉及文件 / Crate**：`crates/service-core/src/session/service.rs`（消息创建后登记恢复）、`crates/manager-agent/src/runner/queue.rs`（队列处理顺序：先 `PAUSED` 后 `QUEUED`）、`crates/manager-agent/src/runtime/session.rs`（唤醒时机）、`crates/lib-web/src/session/api.rs`（**只在 `api/message/create` 的文档注释里说明副作用，不新增路由**）。

  **实施步骤**：
  1. 触发条件只有一条：会话已被占用 ∧ `api/message/create` 成功。消息创建成功后在同一个业务调用里把「该会话有可恢复 Run」这一事实交给执行队列。
  2. 队列处理顺序固定为：① 该会话全部 `PAUSED` Run（`updated_at_ms` 升序，逐个 `PAUSED → RUNNING` 条件更新）→ ② 该会话 `QUEUED` Run（本次消息产生的新 Run 在后）。
  3. 恢复不使用新消息正文：被恢复的 Run 用它自己的原始用户消息（检查点记录的消息上界）。
  4. 只 `occupancy/create` 不发消息时**不触发**恢复（`PAUSED` 保持原样）；显式取消仍走 `api/run/cancel`。
  5. 不新增 `api/run/resume`、不新增 `resume_on_occupy` 之类配置项；不引入后台 sweeper。
  6. `api/message/create` 的返回结构不变（仍返回新建的 Run）；被恢复 Run 的进展只能通过同一条 `api/client/stream` 的 SSE 事件与 `api/run/get` 观察。

  **验收标准**：
  - [x] 「先恢复后 `QUEUED`」的顺序有可复算证据：同一会话同时存在 1 个 `PAUSED` 与本次消息产生的 1 个 `QUEUED` 时，`PAUSED` 那个的 `run_completed` 事件序号早于新 Run 的 `run_started`。（结案：`resume.trigger` 从同一条 SSE 上读到两个序号并断言大小关系；实现上恢复段在 `QUEUED` 段之前，两段在同一个串行任务里。）
  - [x] 只占用不发消息：`PAUSED` Run 的 `status`、`updatedAtMs`、`resumeCount` 逐字段不变。（结案：`resume.trigger` 在只占用之后观察 2 秒，逐字段比对不变；恢复段只由 `resume_pending` 触发，`occupancy/create` 启动队列时不做恢复段。）
  - [x] `api/run/resume` 与 `resume_on_occupy` 在全仓检索命中数为 0（未引入）。（结案：`crates/`、`lingting-agent-ui/src/`、`packages/` 内检索均无命中。）
  - [x] `make gen` 的路由数与接口数与改动前相同（恢复不新增接口）；`api/message/create` 的响应结构逐字段不变。（结案：`make gen` 输出「路由 32 条，接口 32 条」，两份 SDK 内容无变化；`api/message/create` 只改文档注释，返回类型仍是 `MessageSendVO`。）

  **测试要求**：由 P6A-016 的 `resume.trigger`（顺序、只占用不恢复、响应结构不变）覆盖，执行需授权。（未执行：`resume.trigger` 随 P6A-016 一起写；本次未取得用例执行授权，两条依赖它的验收项保持未勾。）

  **结案说明**：
  - 恢复段落在装配层的执行队列（新增 `crates/manager-agent/src/runner/resume.rs`），不在 `drive` 内：队列按 `updated_at_ms` 升序读该会话全部 `PAUSED`，逐条调 `RunDriver::plan_resume`，再按三态处置——`Recoverable` 交给 `run_one` 走与正常路径同一个 `drive`；`Unrecoverable` 用与「未装配执行端口」同一个 `fail` 落 `FAILED` + 固定文案 + `run_failed` 事件；`NoCheckpoint` 与判定读库失败一律**不改行**（前者是升级前的旧 Run，后者是环境问题，都不构成损坏）。
  - 任务原文把「消息创建后登记恢复」写在 `service-core`：实际落点是 `service-core` 的只读 `SessionService::paused_runs` 加上装配层 `send_message` 的 `notify_message`。`service-core` 不能反向依赖运行时队列（`AGENTS.md` 第 6 条），因此「交给执行队列」这个动作只能在装配层完成，与 P6A-010 把判定移到端口是同一取舍。
  - 触发信号用独立的 `resume_pending` 标记，不塞进唤醒通道：唤醒通道容量为 1，连续两条消息的唤醒会被合并丢弃，标记不会。`occupancy/create`（启动队列）与确认唤醒仍只走 `notify_run`，因此「只占用不发消息」不会恢复。
  - `PAUSED → RUNNING` 仍由执行方在拿到全局并发许可后条件更新（P6A-010 已落地），队列只读不推进——这条与任务原文「逐个 `PAUSED → RUNNING` 条件更新」在位置上不同，但语义一致：判定与推进之间隔着许可，恢复不享有特权（D-P6-011）。
  - 未装配执行端口时 `PAUSED` **不判死**（只记日志、保持中断态）：把可恢复的中断态落成终态会永久摧毁可恢复性，而端口缺失是装配问题；这与 `QUEUED` 的 fail-fast（D-P2-028 第 3 条）有意不同。
  - 门禁证据：`cargo check --workspace --all-targets`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全部 0 退出；`make gen` 路由 / 接口各 32 条（未新增）。

- [x] **P6A-012 终态唯一收口：取消 / 关闭 / 恢复中再次中断**

  **目标**：无论怎么叠加，一个 Run 的结局只有一个；`PAUSED` 不会被悄悄留成永久中间态。

  **前置依赖**：P6A-009、P6A-011。

  **涉及文件 / Crate**：`crates/service-core/src/session/service.rs`（`cancel` 覆盖 `PAUSED`）、`crates/manager-agent/src/runtime/session.rs`（优雅关闭）、`crates/manager-agent/src/runner/cancel.rs`、`crates/service-core/src/repository/run.rs`。

  **实施步骤**：
  1. `api/run/cancel` 接受 `PAUSED`：落 `CANCELLED` + 固定原因，`interrupted_reason` 清空，检查点按终态策略清理。
  2. 优雅关闭（`server_shutdown`）不产生 `PAUSED`：既有路径保持 `CANCELLED`，不写检查点、不改 `interrupted_reason`。
  3. 恢复过程中再次强杀：下一次自愈把这次 `RUNNING` 重新判为 `PAUSED`（`resume_count` 已累加，不重置），因此仍然是「一个可恢复的中间态」而不是两条记录。
  4. 终态写入一律条件更新；`PAUSED` 的写入也只对 `RUNNING` 生效，禁止把终态改写成 `PAUSED`。
  5. 取消与恢复的竞态：两者都以条件更新为前提，先到者胜，后者返回「未命中」且不产出事件。

  **验收标准**：
  - [x] `PAUSED` 可取消，结果与普通取消逐字段同形（除 `error` 文案来源）。（结案：`resume.terminal` 两半都断言——存储半边取消 `PAUSED` 后 `status = CANCELLED`、`error = 「用户取消执行」`、`finishedAtMs` 有值、`interruptedReason` 为空，且库内行与返回值逐字段相等；接口半边经 `api/run/cancel` 得到同一形态，`resumeCount` 不变。）
  - [x] 优雅关闭路径对 `PAUSED` 行零改动。（结案：`resume.terminal` 先占用（不发消息，占用本身不触发恢复）再触发 `api/probe/shutdown`，等进程自行退出（exit code 0）后重启读回：`status` / `interruptedReason` / `resumeCount` / `updatedAtMs` / `finishedAtMs` 逐字段与关停前相等。）
  - [x] 「恢复中再次强杀」后状态仍是单个 `PAUSED` 行，`resume_count` 单调不减。（结案：`resume.terminal` 占用 + 一条消息触发恢复，等旧 Run 的首段增量时 `resumeCount` 已为 1；强杀重启后同一 `runId` 仍是 `PAUSED`、`resumeCount` 保持 1、`finishedAtMs` 为空，且本次消息产生的 `QUEUED` Run 落 `CANCELLED`。）
  - [x] 全仓不存在 `UPDATE runs SET status = 'PAUSED'` 以外的 `PAUSED` 写入点，且它带 `WHERE status = 'RUNNING'`。（结案：`RunStatus::Paused` 在产品 crate 里只有四处出现——`paused_runs` 的查询条件、`resume_paused_run` 的条件前提、`stoppable_statuses` 的排除项、`with_orphan_healing` 的 `UPDATE runs SET status = ?` 绑定；唯一写入点的 `WHERE status = ?` 绑定的是 `RunStatus::Running`。）

  **测试要求**：由 P6A-016 的 `resume.terminal` 覆盖（取消、关闭、二次中断、竞态），执行需授权。（结案：`resume.terminal` 已随本任务落地并授权执行——`cargo run -p bin-tests -- resume.terminal` 通过；同批回归 `resume` / `persist` / `run` / `session` / `runtime` / `probe` / `client` 共 46 条全部通过。）

  **结案说明**：
  - 取消落在仓储层的单条条件更新上：`WHERE status IN (未结束四态)`，语句里同时把 `interrupted_reason` 置 `NULL`——`PAUSED` 是它唯一的载体（D-P6-004），清空后取消的行与普通取消同形，没有第二处写。
  - 检查点清理挂在 `SessionService::cancel_run` 成功之后，按终态策略只保留最近 `CHECKPOINT_KEEP_LAST` 条；清理失败**不改取消结果**（终态已提交，检查点只是过程数据，下一次终态清理会再删一次），也不回滚。这是 `prune_checkpoints` 的第一个生产调用点。
  - 停止路径的口径按需求方决定改为「所有停止原因（占用释放 / 断连 / 服务关闭）都不收尾 `PAUSED`」：任务原文只写了 `server_shutdown`，若另两条仍取消 `PAUSED`，一次断连或关掉客户端就会把可恢复的中断态永久判死，与 P6A-015 的「关掉再打开仍看到中断态」矛盾。实现上新加 `Repository::stoppable_statuses()`（从 `RunStatus::ACTIVE` 派生、去掉 `Paused`，不另写状态清单），`cancel_session_runs` 改用它。
  - 第 3、4 条不需要新代码：自愈只写 `status` / `interrupted_reason` / `updated_at_ms` 且 `WHERE status = 'RUNNING'`，因此二次强杀仍是同一个 `PAUSED` 行、`resume_count` 不重置；终态行既不是 `RUNNING` 也不是 `PAUSED`，自愈与恢复都命中 0 行。
  - 竞态由数据库判定：取消是 `WHERE status IN (未结束四态)`、恢复是 `WHERE status = 'PAUSED'`，先写者生效；未命中一方不改行、不产事件（`manager-agent::cancel_run` 只在取消成功时才登记取消信号）。
  - 门禁证据：`cargo check --workspace --all-targets`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全部 0 退出。
  - 用例证据（`resume.terminal`，数字取自用例返回的结果条目）：检查点 25 条 → 取消后 20 条；二次取消返回「不可取消」且 `status` / `error` / `finishedAtMs` / `updatedAtMs` 逐字段不变；优雅关闭后进程自行退出（exit code 0）而中断态逐字段不变；恢复中再次强杀 `resumeCount` 由 0 涨到 1 后保持不变；取消后发消息只执行新 Run（`run_completed`）。用例的服务端快照 `audit.panics = 0`，敏感串命中 0。
  - 用例侧新增的共享动作按 `D-P2.1-013` 放在 `crates/bin-tests/src/util`：`flow::Reopened`（在既有会话上重建链路，跨重启复用同一条对话）、`flow::{create_session,send_message,run_get,messages}`、`server::Server::wait_exit`（等优雅关闭自行走完）。断言只引用接口响应与库内行，不读日志与事件文件。
---

## Batch D：Run Persistence 可观测与客户端（P6A-013 ~ P6A-016）

> 依赖：Batch C。本批次把「恢复」从内部能力变成可查、可看、可操作的对外能力，并把用例落地。

>
> **Batch D 结案（已执行）**：P6A-013 ~ P6A-016 全部落地并验收。
>
> 验收证据：`cargo check --workspace --all-targets`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`make gen-check`、`make ui-check` 全部 0 退出；`make gen` 路由 / 接口各 **33** 条（相对 P5 基线 +1，只新增 `api/run/history`）；`make test ARGS="resume"` **11 个用例全部通过**（`resume.checkpoint` / `terminal` / `crash` / `trigger` / `idempotent` / `notify` / `broken` / `history` / `metrics` / `tui` / `web`）。
>
> 需要回填的口径差异：
>
> 1. `metrics` 快照的字段名沿用既有各段的 **snake_case**（`checkpoints_written` 等），不是 `O-1` 写的 camelCase：既有七段全部是 snake_case，为一段单独改成 camelCase 会造出两套命名；wire 契约以快照的实际键名为准。
> 2. `resume.trigger` 的「只占用不发消息」观察窗口是 2 秒（恢复只能在 `api/message/create` 之后发生），不是等待某个事件；`resume.crash` 用「等到首段增量 / 等到工具结果之后的增量」代替 sleep 定位强杀窗口（`RK-6A-09`）。
> 3. 强杀窗口必须**先杀进程再丢客户端链路**：先 drop 链路会触发该会话的断连清理链，把执行中的 Run 收成 `CANCELLED`，自愈就没有可恢复的残留（已在 `resume::interrupt_batch` / `interrupt_running` 的注释里登记）。
> 4. `resume.crash` 第二段的断言从「步骤条数变多」改为「已提交步骤前缀逐条不变、总数不减」：收尾那一轮是纯文本，不再产生工具边界，因此恢复后条数可能不变。
> 5. `resume.broken` 的「五类损坏」用注入库内行的方式构造（指针不符、消息上界缺失、孤儿 TOOL、`PENDING` 落在更早轮次、轮次不符），另加未知格式版本与无检查点两种；`R-6` 的「五类各自落 FAILED 且文案互不相同」逐条断言。
> 6. `resume.web` 不驱动浏览器：取证来自前端工程的两个用例文件（`tests/resume-paused.test.tsx` / `tests/resume-banner.test.tsx`，共 8 条）+ 客户端源码检索（`api/run/resume` / `resume_on_occupy` 命中 0）。`I-5` 的 `pnpm exec tsc --noEmit` 由 `make ui-check` 覆盖。
> 7. 顺手修掉一个真实缺陷：`CheckpointError::Storage`（读库失败）原本会被判成「不可恢复」把 Run 判死，现在只有 `Decode`（数据问题）才落终态，读库失败返回 `Err` 并保留中断态等下一次触发（`R-7` 的「环境问题不是损坏」）。
> 8. 客户端两处改动超出「只显示提示」的字面范围，但都是同一条判据（`I-4` / `I-5` 的「按事件里的 `runId` 归类」）的必要条件：TUI 的 `RunStarted` / `Completed` / `Failed` 与 WebUI 的助手占位登记都改成按 `runId` 归位；WebUI 另加了「刷新后接着看同一条会话」（`sessionStorage` 存会话 ID），否则页面刷新后看不到中断态。
- [x] **P6A-013 步骤历史只读接口 `api/run/history`**

  **目标**：一个 Run 的步骤历史（检查点序列 + 恢复次数 + 中断原因）可查，客户端据此重建「中断 → 恢复」的时间线。

  **前置依赖**：P6A-003、P6A-012。

  **涉及文件 / Crate**：`crates/lib-web/src/session/api.rs`、`crates/service-core/src/session/service.rs`（只读查询）、`crates/agent-core/src/session/dto.rs`（`RunHistory` / `RunStep` DTO）、生成物 SDK。

  **实施步骤**：
  1. `GET api/run/history?runId=…` 返回 `RunHistory { run, resumeCount, interruptedReason, steps: Vec<RunStep> }`；`RunStep { seqNo, phase, toolRound, createdAtMs }`。
  2. 只读：不触发恢复、不改状态；`payload` 不外泄（只返回派生字段）。
  3. 未知 Run → 404；`steps` 为空是合法响应（旧 Run 无检查点）。
  4. 与 `api/run/get` 的分工写进接口文档待同步清单：`get` 是状态，`history` 是过程。

  **验收标准**：
  - [x] 返回结构里没有 `payload` 原文字段；`steps` 按 `seqNo` 升序。（结案：`resume.history` 断言字段白名单为 `createdAtMs` / `phase` / `seqNo` / `toolRound`，序号从 1 连续。）
  - [x] 旧库 Run（无检查点）返回 `steps = []` 且不报错。（结案：`resume.history` 用「未占用 → Run 停在 QUEUED」的形态取证，`steps = []` 且 `status = QUEUED`。）
  - [x] 接口总数 +1；`make gen` 门禁通过。（结案：`make gen` 路由 / 接口各 33 条，`make gen-check` 0 退出。）

  **测试要求**：由 P6A-016 的 `resume.history` 覆盖（正常、空、404），执行需授权。

- [x] **P6A-014 `metrics` 增加 run / checkpoint / resume 段**

  **目标**：恢复相关的数字可复算，不靠日志。

  **前置依赖**：P6A-012。

  **涉及文件 / Crate**：`crates/manager-agent/src/runtime/metrics.rs`、`crates/agent-runtime/src/metrics.rs`、`crates/service-core/src/repository/checkpoint.rs`（计数点）、`docs/api/probe.md` 的字段清单（待同步）。

  **实施步骤**：
  1. 新增段 `resume`：`checkpoints_written`、`checkpoints_pruned`、`resume_attempts`、`resume_succeeded`、`resume_rejected`（按拒绝原因分桶）、`resume_unrecoverable`。
  2. 全部在 `metrics` 特性下累加；未开特性时是空实现（与既有采集点同一口径）。
  3. 采集点各自在本层（写检查点、清理、推进、判定），只有一个渲染点（`bin-tests` 的 `util::observability`）。
  4. 不新增阈值日志；不为采集新增 `warn!` / `debug!`。

  **验收标准**：
  - [x] 六个字段都能在一次「崩溃 → 恢复 → 完成」里被快照读到，且与接口侧事实逐项相等（检查点条数、`resume_count`）。（结案：`resume.metrics` 断言六键存在，`checkpoints_written` == 本进程新增步骤数、`resume_succeeded` == `resumeCount` 之和、`resume_attempts` == 1，拒绝 / 不可恢复 / 清理均为 0。）
  - [x] 未开 `metrics` 的构建里这些字段不存在，且不引入静态内存（`Cargo.toml` 特性链核对）。（结案：`service-core` 的 `observability` 模块在 `#[cfg(feature = "metrics")]` 下才有 `AtomicU64` 静态量，未开特性时只有空实现与 `Default` 快照；`manager-agent` 的 `metrics` 特性链已加上 `service-core/metrics`。）
  - [x] 全仓无新增的采集型日志调用（静态核对 `git diff`）。（结案：本批次新增的日志只有恢复段的 `info!` / `warn!` 判定说明（既有格式），没有为采集新增 `warn!` / `debug!`。）

  **测试要求**：由 P6A-016 的 `resume.metrics` 覆盖，执行需授权。

- [x] **P6A-015 客户端：展示中断态并说明如何继续**

  **目标**：TUI 与 WebUI 能看见「这个 Run 被打断、可以继续」，并明确告诉用户「发一条消息就会接着跑」；**不新增恢复动作、不新增恢复按钮**。

  **前置依赖**：P6A-011、P6A-013。

  **涉及文件 / Crate**：`crates/lib-bin-tui/src/`（会话视图）、`lingting-agent-ui/src/`（会话面板）、`crates/lib-bin-cli/`（客户端方法经生成的 SDK）。

  **实施步骤**：
  1. 状态映射：`PAUSED` 在 TUI 与 WebUI 都显示为「已中断（发送消息可继续）」，与既有状态色板一致；`interruptedReason` 原样展示。
  2. 提示而不是动作：中断态附一行说明「直接发送消息即可继续」，不新增按键 / 按钮，也不隐藏输入框。
  3. 发送消息后不做特殊处理：恢复的 Run 与新建的 Run 都通过既有的 `api/client/stream` 事件与 `api/run/get` 更新；客户端不得假设「事件里的 `runId` 一定是自己刚发的那个」。
  4. 不做自动重试、不做后台轮询恢复。
  5. 客户端的穷尽匹配必须覆盖新状态（TS 侧类型由生成的 SDK 提供）。

  **验收标准**：
  - [x] 两端都能展示 `PAUSED` + `interruptedReason`，并展示「发送消息可继续」的提示文案。（结案：TUI 状态栏显示「已中断（发送消息可继续）：<原因>」（`resume.tui` 断言渲染帧），WebUI 用提示条显示同一句文案（`pausedNoticeOf` 单一产点，`resume-banner` 用例断言）。）
  - [x] 客户端源码里检索不到 `api/run/resume`（未引入恢复动作）；中断态下输入框仍可用。（结案：`resume.web` 扫 `lingting-agent-ui/src` / `lib-bin-tui/src` / `lib-bin-cli/src`，`api/run/resume` 与 `resume_on_occupy` 命中 0；`resume-banner` 断言输入框未 disabled、没有恢复按钮。）
  - [x] 单次发消息可能导致两条 Run 的事件（恢复的旧 Run + 新的 Run），两端都按事件里的 `runId` 归类，不按「时间顺序」猜。（结案：TUI 的 `RunStarted` / `Completed` / `Failed` 带 `run_id` 并按它归位；WebUI 的助手占位登记改成 `runId → key` 映射；`resume-paused` 的两个用例分别断言「两段回复各归各的气泡」与「旧 Run 先结束时界面仍在等新 Run」。）
  - [x] `pnpm exec tsc --noEmit` 通过（既有 `make ui-check` 口径）。（结案：`make ui-check` 0 退出。）

  **测试要求**：由 P6A-016 的 `resume.tui` / `resume.web` 覆盖（复用 `skill.tui` / `skill.web` 的驱动方式），执行需授权。

- [x] **P6A-016 用例落地：`resume.*` 用例域**

  **目标**：把本子阶段的可执行判据落成用例，域内用例覆盖崩溃、幂等、中断通知、损坏、终态、触发顺序、接口、客户端、可观测。

  **前置依赖**：P6A-009 ~ P6A-015。

  **涉及文件 / Crate**：`crates/bin-tests/src/cases/resume.rs` + `crates/bin-tests/src/cases/resume/`（按需拆）、`crates/bin-tests/src/cases/mod.rs`、`crates/bin-tests/src/util/`（如有新的共享动作，按 `D-P2.1-013` 放 `util`）。

  **实施步骤**：
  1. 用例清单（名称即证据）：
     - `resume.crash`：`mock:slow` 停在 `RUNNING` → `start_kill` → 重启 → `PAUSED` → 占用 + 发一条消息 → 完成，助手消息只有一条。
     - `resume.trigger`：同一会话同时有 1 个 `PAUSED` 与 1 个新消息产生的 `QUEUED` 时，`PAUSED` 的 `run_completed` 事件序号早于新 Run 的 `run_started`；只占用不发消息时 `PAUSED` 三个字段不变；`api/message/create` 响应结构逐字段不变。
     - `resume.idempotent`：同一 `PAUSED` Run 在两条消息连续到达时只被恢复一次（`resumeCount` 只 +1，工具审计行数不增加）。
     - `resume.notify`：三类形态（已配对 / `PENDING` / 旧行无 `PENDING`）各一条，未配对两种形态的工具调用次数为 0，回灌文本含「上次执行被中断：」且不含参数原文与绝对路径。
     - `resume.broken`：五类损坏 + 未知格式版本 + 序号缺口，各自落 `FAILED` 且文案固定。
     - `resume.terminal`：`PAUSED` 可取消、优雅关闭不动 `PAUSED`、恢复中二次强杀仍是一个 `PAUSED`。
     - `resume.checkpoint`：正常 Run 的检查点清单（phase / 序号 / 条数）、`PENDING` 残留、清理策略。（存储半段已在 Batch A 落地并通过，此处补运行时写入点与接口侧断言。）
     - `resume.history`：`api/run/history` 的正常 / 空 / 404 三种响应，`steps` 升序且不含 payload 原文。
     - `resume.metrics`：`resume` 段六字段复算。
     - `resume.tui` / `resume.web`：中断态展示与「发送消息可继续」提示、双 Run 事件按 `runId` 归类。
     - `persist.migration`：`0002` 旧库升级到 `0003` 后旧行可读。
  2. 数据采集只走接口响应、客户端事件与 `metrics` 快照；不读 `server.log`。
  3. 「未采集」必须显式记录（名称 + 原因），不得静默跳过。
  4. 用例注册进 `cases/mod.rs`，命名遵守 `<领域>.<场景>`。

  **验收标准**：
  - [x] `make test ARGS="--list"` 列出上述全部用例名。（结案：`resume` 域 11 条——`checkpoint` / `terminal` / `crash` / `trigger` / `idempotent` / `notify` / `broken` / `history` / `metrics` / `tui` / `web`；`persist.migration` 已在 Batch A 覆盖 `0003`。`resume.retry` 随 P6A-017 登记，本批次未新增。）
  - [x] 每个用例的断言只引用接口响应 / 事件 / 快照字段（静态核对无 `server.log` 读取）。（结案：新增的九个用例文件只读接口响应、SSE 事件、`metrics` 快照与库内行；`resume.broken` / `resume.notify` 的库内写入是「注入损坏」而不是数据来源。）
  - [x] 用例文件不落在 `crates/bin-tests` 之外的任何 Crate。（结案：全部在 `crates/bin-tests/src/cases/resume/`。）

  **测试要求**：本任务**就是**用例落地；执行需单独授权（`make test ARGS="resume"`）。未授权时只提交用例代码并保持任务未勾选。

---

## Batch E：Task Recovery 与收尾（P6A-017 ~ P6A-020）

> 依赖：Batch D。本批次把「失败之后怎么办」定死，并把门禁、生成物与阶段结论收口。

> **Batch E 结案（已执行）**：P6A-017 ~ P6A-020 全部落地并验收，阶段报告见 [`docs/reports/p6a-020-acceptance.md`](../../../docs/reports/p6a-020-acceptance.md)。
>
> 验收证据：`cargo build --workspace`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`make gen`（路由 / 接口各 33 条）、`make gen-check`、`make ui-check`、`make check` 全部 0 退出；`make test ARGS="--all"` **150 个用例全通过**（P5 基线 138 + 6A 新增 12）。
>
> 需要回填的口径差异：
>
> 1. `O-1` 的六个字段名沿用既有各段的 **snake_case**，且 P6A-018 追加了第七个键 `checkpoint_payload_bytes`（写放大口径）；`limit` 段另新增 `retries`。
> 2. `A-1` 的 `framework_` 检索命中 1（`use framework_proc_auto::auto_enum;`），它是 `AGENTS.md` 第 20 条要求的 API 标记宏，不是协议类型；`A-5` 的 `cargo tree` 按 `--depth 1` 判定（`lib-db` / `sqlx` 只经 `service-core` 传递出现）。
> 3. `S-3` 的第一条（对终态 Run 调 `api/run/resume`）与 D-P6-009 冲突，以决策为准；有效口径是第二条（恢复触发 + `api/run/cancel`）。
> 4. `I-3` 的「上下文重新组装」标记既没有字段也没有文案——D-P6-003 只要求「重新组装」，实现选择不记录上下文指纹（`KI-P6A-004`）。
> 5. `P-5` 的「`checkpointsPruned` 与删除条数相等」拆成两处：存储半段（同进程并行）断言「至少增长」，隔离服务端的 `resume.terminal` 断言「逐值相等」。
> 6. P6A-017 的 `[agent]` 两个新配置项**不在生成物里**：`AgentConfig` 是内部配置类型（不加 `auto_*` 宏），只在 `config.toml` 与 `lib-bin-cli` 的校验里出现。

- [x] **P6A-017 失败分类与重试策略**

  **目标**：失败有分类、重试有上限且可观测、不可重试的终态有固定原因；长任务的「分段」就是轮次边界，不引入新的调度层。

  **前置依赖**：P6A-007、P6A-014。

  **涉及文件 / Crate**：`crates/agent-runtime/src/error.rs`、`crates/agent-runtime/src/loop_.rs`、`crates/types-core/src/config/agent.rs`（`run_max_retries` / `run_retry_backoff_ms`）、`crates/lib-bin-cli/src/config.rs`（校验）。

  **实施步骤**：
  1. 分类表落成代码注释与单一函数：**可重试**（上游瞬时错误、`LlmError` 的传输类、工具 `Retryable`）与**不可重试**（认证、上下文超预算、轮次用尽、轨迹损坏、检查点不可用/写失败、取消）。
  2. Run 级重试：`run_max_retries` 缺省 `0`（**缺省不重试**，保持现有行为），配置为正整数时按次数重试同一次 LLM 请求（不重新入队、不重跑已提交轮次）；每次重试累加 `metrics` 计数。
  3. 重试用尽 → `FAILED` + 固定原因；不可重试 → 立即 `FAILED` + 固定原因，不消耗重试预算。
  4. 配置非法（负数、超上限）是启动期硬错误（退出码 2）。
  5. 不引入「Run 级自动重新入队」：需要重新执行时由用户显式再发消息（D-P6-012）。

  **验收标准**：
  - [x] 分类表的每个分支都有对应固定文案，且文案集中在 `error.rs`。（结案：`error::retryable` 上方就是分类表（可重试 / 不可重试逐条点名文案来源）；本任务新增的唯一文案是 `error::RETRY_EXHAUSTED`「模型调用重试次数已用尽」，其余分支复用各自既有产点——`agent-route` 的 `public_message()`、`agent-context` 的预算文案、`error.rs` 的 `TOOL_ROUNDS_EXCEEDED` / `TOOL_TRACE_BROKEN` / `CHECKPOINT*` / `RESUME_*` / `CANCEL_FALLBACK`。`resume.retry` 断言可重试分类落 `RETRY_EXHAUSTED`、不可重试分类落原始文案。）
  - [x] `run_max_retries = 0` 时行为与改动前逐字段一致（既有 `run.*` 用例不变）。（结案：`make test ARGS="run"` 10/10、`ARGS="runtime"` 18/18 全绿，三条 `run.failed_*` 的文案断言未放宽；实现上「一次都没重试过」时才用原始文案，见 `loop_.rs` 的 `attempt == 0` 分支。）
  - [x] 重试次数在 `metrics` 快照里可复算，且不超过配置上限。（结案：`agent-runtime::metrics` 新增 `retries` 原子量（`metrics` 特性下），经 `manager-agent` 的 `LimitMetrics::retries` 转写到 `api/probe/metrics`；`resume.retry` 断言 `limit.retries == 2 == 配置次数`，Mock 侧 `scenarios.upstream = 3` 说明「1 次失败 + 2 次重试」。）
  - [x] 不可重试路径的 `resume_count` 不增加。（结案：`resume.retry` 断言该 Run 的 `resumeCount == 0`；`resume.retry` 第二段再断言认证失败前后 `limit.retries` 不变。）

  **测试要求**：由 `resume.retry`（本轮新增，登记在 `crates/bin-tests/src/cases/resume/retry.rs`）与既有 `run.*` 回归覆盖，执行需授权。（结案：已授权执行 `make test ARGS="resume.retry"`、`ARGS="run"`、`ARGS="runtime"` 全绿；`llm.config` 另补两个配置项的缺省 / 合法 / 越界断言。）

- [x] **P6A-018 写放大与恢复开销核对**

  **目标**：确认检查点没有把落库路径变成瓶颈，并用数字（而不是感觉）给出结论。

  **前置依赖**：P6A-016。

  **涉及文件 / Crate**：`crates/agent-runtime/src/metrics.rs`、`crates/service-core/src/repository/checkpoint.rs`、`crates/bin-tests/src/cases/resume/`（只读核对与快照口径）。

  **实施步骤**：
  1. 定义三个可复算指标：单 Run 检查点条数、检查点 payload 平均字节、单次 Run 的落库次数（既有 `metrics` 的落盘计数 + 新增检查点计数）。
  2. 在 `resume.checkpoint` 里断言上界：payload ≤ 2 KiB、两轮工具 Run 的检查点条数 = 5。
  3. 与 P2.1 的落库指标对照：不引入新的同步阻塞点（检查点写入与既有事务同一连接池路径）。
  4. 结论写进验收报告，不写进本文档。

  **验收标准**：
  - [x] 三个指标都能从快照读到，且用例断言的是上界而不是「看起来还行」。（结案：单 Run 条数 = `api/run/history` 的 `steps` 条数；payload 平均字节 = 新增的 `resume.checkpoint_payload_bytes` / `checkpoints_written`；落库次数 = `journal.appends + resume.checkpoints_written`。`resume.checkpoint` 断言「单条 ≤ 2048 字节」「两轮工具 Run 的 5 条共 272 字节、平均 54 字节」，`resume.metrics` 断言「平均载荷 ≤ 单条上界」并给出落库次数 16。）
  - [x] 检查点写入没有引入 `spawn_blocking` 之外的新阻塞模式（静态核对调用点）。（结案：`service-core/src/repository/checkpoint.rs` 的 `append_checkpoint` 只有 `begin` → 两条语句 → `commit`，与既有 `with_xxx` 同一路径；`grep spawn_blocking|block_on|std::thread` 在 `checkpoint.rs` / `loop_/checkpoint.rs` / `observability.rs` 命中 0。）
  - [x] 单 Run 的检查点条数与「起点 + 每轮模型边界 + 每轮工具边界」之和相等。（结案：`resume.checkpoint` 断言两轮工具 + 正常完成 = 5 条、序号 1..5、phase 依次 `MODEL_DONE / MODEL_DONE / TOOLS_DONE / MODEL_DONE / TOOLS_DONE`。）

  **测试要求**：并入 `resume.checkpoint`；执行需授权。（结案：已授权执行，`resume.checkpoint` 的写放大段与 `resume.metrics` 的平均载荷断言均通过。）

- [x] **P6A-019 门禁、生成物与文档待同步清单**

  **目标**：新增枚举值、新增一个只读接口与两个配置项不造成契约漂移；把需要同步的文档写成清单交给人处理（本轮不改文档）。

  **前置依赖**：P6A-011、P6A-013、P6A-017。

  **涉及文件 / Crate**：`packages/sdk-rust` / `packages/sdk-ts`（生成物，不入库）、`docs/api/session.md`、`docs/api/probe.md`、`docs/workspace.md`、`docs/develop.md`（**只列清单，不修改**）。

  **实施步骤**：
  1. 依次执行 `make gen`、`make gen-check`、`make ui-check`、`make check`（均不执行测试）。
  2. 核对生成物的差异只有：`RunStatus` 新增 `PAUSED`、**一个**新接口（`api/run/history`）、`RunHistory` / `RunStep` 两个新 DTO、`[agent]` 两个新配置项（`run_max_retries` / `run_retry_backoff_ms`）。**不应出现** `api/run/resume`、`RunResumePO`、`resume_on_occupy`（按 D-P6-009 未引入）。
  3. 写出文档待同步清单（给需求方决定是否授权同步）：
     - `docs/api/session.md`：`RunStatus` 取值表（新增 `PAUSED`）、状态机说明（含「`PAUSED` 由自愈产生、由发消息触发恢复」）、`api/run/history` 章节、`api/message/create` 触发恢复的说明。
     - `docs/api/probe.md`：`metrics` 的 `resume` 段字段。
     - `docs/workspace.md`：`service-core` 的 `checkpoint` 模块、`agent-core` 的检查点类型、`agent-tool` 的 `ToolCallRecord` 新增字段（并注明**没有**新增可重放自述）。
     - `docs/develop.md`：`[agent]` 的两个新配置项（`run_max_retries` / `run_retry_backoff_ms`）与含义。
  4. 不修改 `README.md`、不改上面四份文档。

  **验收标准**：
  - [x] `make gen-check` / `make ui-check` / `make check` 退出码均为 0。（结案：三者均 0 退出；`make gen` 另输出「路由 33 条，接口 33 条」。）
  - [x] 生成物差异清单与 §实施步骤 2 逐项相等，没有多余改动。（结案：相对 P5 基线只有 `api/run/history` 一个新接口（路由 / 接口 32 → 33），`RunStatus` 的 `PAUSED`、`RunHistory` / `RunStep` 两个 DTO、`Run` 的三个新字段都是 Batch A / D 已落地产物；`api/run/resume` / `RunResumePO` / `resume_on_occupy` 命中 0。**两处需要回填**：（1）本任务的 `[agent]` 两个新配置项**不在生成物里**——`AgentConfig` 是内部配置类型（不加 `auto_*` 宏），只在 `config.toml` 与 `lib-bin-cli` 的校验里出现；（2）Batch B 登记的「`make ui-check` 既有失败」（UI 夹具缺 `Run` 的三个新字段）已由 Batch D 的客户端改动一并修掉，本轮复核仍是 0 退出、UI 文件零改动。）
  - [x] 待同步清单以任务结案说明的形式给出，`git status` 里没有文档改动。（结案：清单在 [报告](../../../docs/reports/p6a-020-acceptance.md) §七；本轮 `git status` 的 `docs/` 只有新增的报告文件，`README.md` / `docs/develop.md` / `docs/workspace.md` / `docs/api/*` 零改动。）

  **测试要求**：无（门禁命令不执行测试；`make check` 只跑 fmt + clippy）。

- [x] **P6A-020 阶段验收与报告**

  **目标**：判据逐条取证，未采集与结转风险显式记录，阶段结论可被下一子阶段（6B）直接引用。

  **前置依赖**：P6A-001 ~ P6A-019。

  **涉及文件 / Crate**：`docs/reports/p6a-020-acceptance.md`（新，报告）、`.steps/p6/6a/{README.md,tasks.md,acceptance.md,decisions.md}` 的状态行。

  **实施步骤**：
  1. 按 [acceptance.md](acceptance.md) 的十组逐条取证，每条给出证据来源（命令 / 用例名 / 静态核对 / 人工判定动作）。
  2. 「未采集」逐条写明名称与原因（如「无 `metrics` 构建下的快照」）；「未授权」逐条写明，不得算通过。
  3. 结转风险按 `KI-P6A-xxx` 登记（例：`PAUSED` 的契约影响面、事件文件与恢复的时间线差异）。
  4. 更新 `.steps/p6/6a/README.md` 与 `.steps/README.md` 的状态行（**这属于本轮授权范围之外，需另行确认**）。
  5. 报告不写日志路径作为数据来源；用例失败时只给出日志路径供人工排查。

  **验收标准**：
  - [x] 十组判据逐条有结论（成立 / 未授权 / 未采集），没有空缺。（结案：报告 §二 ~ §五 逐条列出 58 条；除 `O-3` 记为「未采集」外全部成立，没有「未授权」；另有 6 处判据原文与实现的偏差在 §6.2 逐条说明。）
  - [x] 报告里所有数字都能追到用例结果或 `metrics` 快照。（结案：报告引用的数字全部来自 `make test` 的用例报告条目与 `api/probe/metrics` 快照；用例目录以运行目录编号标注，未把日志当数据来源。）
  - [x] 结转风险条目至少覆盖：`PAUSED` 契约影响面、副作用不可回滚的两类未知、迁移不可回滚。（结案：`KI-P6A-001` / `KI-P6A-002` / `KI-P6A-003`，另加 `KI-P6A-004` ~ `KI-P6A-008`（上下文一致性、重试边界、事件时间线、用例设施偶发、检查点表增长）。）

  **测试要求**：无新增用例；报告取证依赖已授权执行的用例结果。（结案：本轮取证依赖的用例全部在授权下执行：`resume` 12/12、`llm.config` 1/1、`skill` 15/15、`tool` 28/28、`persist` 5/5、`run` 10/10、`runtime` 18/18，全量 150/150。）
