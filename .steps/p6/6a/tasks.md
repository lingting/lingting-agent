# 6A 任务清单

**状态：规划中（未开工）。** 20 个任务 / 5 个批次。子阶段目标与边界见 [README.md](README.md) §一 / §二，判据见 [acceptance.md](acceptance.md)，决策见 [decisions.md](decisions.md)。

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
  - [ ] 同一 Run 的 `seq_no` 从 1 开始连续递增；人为制造缺口后，读回结果能被上层判为损坏（不静默跳过）。（结案：序号连续已验收；「缺口判损坏」属 P6A-007 的解析，尚未落地。）
  - [x] 插入失败时 `runs.last_checkpoint_seq` 保持原值（同事务回滚）。
  - [x] `payload` 的序列化结果不含 `api_key`、不含系统提示词全文、不含 `AGENTS.md` 内容（静态核对序列化结构与调用点）。
  - [x] Run 落终态后 `list_of_run` 的条数 ≤ `keep_last`。

  **测试要求**：由 P6A-016 的 `resume.checkpoint` 覆盖（序号连续性、事务回滚、清理策略）。（结案：存储半段已由新增的 `resume.checkpoint` 覆盖并通过。）

- [ ] **P6A-004 检查点格式版本化与不识别版本的处置**（部分落地：格式版本与两类解码已落地，三条固定文案结转到 P6A-005 / P6A-007）

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
  - [ ] 三类错误各有独立文案，文案里不含绝对路径、不含 payload 内容。（未落地：按需求方决定结转到 P6A-005 / P6A-007，Batch A 没有使用点，提前定义会被 `dead_code` 判失败。）
  - [x] 静态核对：`Unsupported` 与 `Corrupt` 的处置路径相同且都不进入恢复执行。

  **测试要求**：由 P6A-016 的 `resume.broken` 覆盖（未知版本、缺字段、序号缺口三类）。（结案：未知版本与缺字段已由 `resume.checkpoint` 覆盖；序号缺口属 P6A-007。）

---

## Batch B：循环内检查点与重复执行边界（P6A-005 ~ P6A-008）

> 依赖：Batch A。本批次结束时，一次正常 Run 的每个已提交边界都有检查点；工具执行有「开始标记」；恢复前能判定「可恢复 / 损坏」；未配对调用一律不重放并产出中断通知。此时还**没有**恢复触发路径，也还没改自愈。

- [ ] **P6A-005 轮次边界的检查点写入**

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
  - [ ] 三种 phase 各只有一个构造函数（可被多次调用，但构造与追加只有一处）（静态核对调用点数量）。
  - [ ] 检查点写入点全部位于对应提交之后：先 `append` / 终态事务提交，再 `append_checkpoint`。
  - [ ] 一次「两轮工具 + 正常完成」的 Run，检查点条数与「起点 + 每轮模型边界 + 每轮工具边界」之和相等，且 `seq_no` 连续。
  - [ ] 检查点写失败时 Run 落 `FAILED`，`runs.error` 是固定文案，且**不产生**助手消息。

  **测试要求**：由 P6A-016 的 `resume.checkpoint` 覆盖（正常 Run 的检查点清单与条数、写失败路径由注入式失败构造）。

- [ ] **P6A-006 工具调用的「开始标记」前移**

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
  - [ ] 正常路径的 `tool_calls` 行在结果、时长、`tool_round` 上与改动前逐字段相等（旧用例断言不变）。
  - [ ] 模拟「写入后、更新前崩溃」：库里留下 `PENDING` 行且 `started_at_ms` 有值。
  - [ ] `WHERE status = 'PENDING'` 的条件更新是唯一收尾路径，全仓无第二处无条件 `UPDATE tool_calls SET status`。
  - [ ] `agent-tool` 的错误分类、重试与超时判定零改动（`git diff` 限定到该 crate 时只有 `audit.rs` 的字段与 `executor.rs` 的透传）。

  **测试要求**：由 P6A-016 的 `resume.checkpoint`（`PENDING` 残留）与既有 `tool.audit` 回归覆盖；回归执行需授权。

- [ ] **P6A-007 恢复点解析：检查点与工具轨迹的一致性校验**

  **目标**：恢复前先判定「能不能恢复」。可恢复 → 产出确定的恢复点；不可恢复 → 直接终态，绝不半恢复。

  **前置依赖**：P6A-005、P6A-006。

  **涉及文件 / Crate**：`crates/agent-runtime/src/loop_/checkpoint.rs`（新：解析与判定）、`crates/agent-runtime/src/loop_/trace.rs`（复用/扩展 `recover`）、`crates/agent-runtime/src/error.rs`（五类固定文案）、`crates/agent-core/src/session/checkpoint.rs`。

  **实施步骤**：
  1. 读最近检查点 + `messages` / `tool_calls`；产出 `ResumePoint { checkpoint_seq, phase, round, unpaired: Vec<LlmToolCall> }`。
  2. 五项校验（任一不满足即 `Unrecoverable`）：① `seq_no` 无缺口；② `last_message_seq` 上界内的消息齐全；③ 最后工具组之外的组不缺结果（沿用 D-P3-023）；④ `PENDING` 行只出现在最后检查组；⑤ 检查点 phase 与轨迹形态一致（`TOOLS_DONE` 之后不应还有未提交的助手调用）。
  3. 判定结果三态：`Recoverable(ResumePoint)` / `Unrecoverable(reason)` / `NoCheckpoint`。`NoCheckpoint` 只对「升级前的旧 Run」出现（`R-7`）。
  4. `Unrecoverable` 在装配层落 `FAILED` 并写固定原因；**不重试、不降级成从头执行**。
  5. 与 P3 既有 `trace::recover` 的关系：保留它对「挂起确认唤醒」的用法，恢复路径改成「先 `checkpoint::parse` 再 `trace::recover`」。

  **验收标准**：
  - [ ] 五类损坏各有独立固定文案，且都不进入执行路径。
  - [ ] 可恢复场景的 `ResumePoint` 字段与最近检查点逐字段相等（`phase` / `round` / `last_message_seq`）。
  - [ ] 全仓不存在「部分恢复」分支（先落一半状态再继续执行）。
  - [ ] `NoCheckpoint` 与 `Unrecoverable` 的处置不同：前者保留 Run 可用于「放弃恢复 → 终态」，后者直接终态。

  **测试要求**：由 P6A-016 的 `resume.broken` 覆盖（五类损坏 + `NoCheckpoint`）。

- [ ] **P6A-008 恢复不重放：中断通知与简短原因**

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
  - [ ] 全仓不存在「恢复时重放工具」的分支：静态核对 `loop_` 内调用 `ToolExecutor` 的路径只有正常执行路径与确认唤醒路径，恢复分支不产生新的执行调用。
  - [ ] 三类形态各有用例断言，且「未配对」两种形态的工具调用次数为 0（以 `tool_calls` 行数为准，不读日志）。
  - [ ] 通知文本里不含调用参数原文、不含 `AGENT_HOME` 绝对路径、不含工具名以外的可变内容。
  - [ ] `crates/agent-tool` 与 `crates/agent-mcp` 在本任务下零改动（`git diff` 为空）。

  **测试要求**：由 P6A-016 的 `resume.notify` 覆盖（三类形态 + 文本红线），执行需授权。

---

## Batch C：自愈、恢复触发与装配（P6A-009 ~ P6A-012）

> 依赖：Batch B。本批次结束时，强杀后的 Run 是 `PAUSED` 而不是 `FAILED`；客户端「占用 + 发一条消息」能从最近检查点继续并跑完、且恢复先于该消息的新 Run；取消与优雅关闭仍然只走终态。

- [ ] **P6A-009 启动自愈改造：`RUNNING → PAUSED`**

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
  - [ ] 强杀后重启：Run 为 `PAUSED`、`interrupted_reason` 有固定值、`finished_at_ms` 为空。
  - [ ] 二次重启：`PAUSED` 行逐字段不变（含 `updated_at_ms` 之外的全部列）。
  - [ ] `QUEUED` / `WAITING` 的自愈结果与改动前逐字段一致（仍 `CANCELLED` + `HEAL_QUEUED`）。
  - [ ] `api/probe/health` 的 `running_run_count` 不把 `PAUSED` 计入。

  **测试要求**：改写 `crates/bin-tests/src/cases/persist.rs` 的 `persist.running_heal`（断言从 `FAILED` 改为 `PAUSED` + `interrupted_reason`），并新增 `resume.crash` 于 P6A-016。**用例执行需授权**；未授权时任务不勾选。

- [ ] **P6A-010 恢复执行路径（运行时侧）**

  **目标**：`drive` 能从 `PAUSED + ResumePoint` 继续，且这一路径与正常路径共用收尾事务，不产生第二条助手消息、不重复已提交的轮次。

  **前置依赖**：P6A-007、P6A-008。

  **涉及文件 / Crate**：`crates/agent-runtime/src/loop_.rs`（恢复分支入口）、`crates/agent-runtime/src/loop_/checkpoint.rs`、`crates/agent-runtime/src/port.rs`（端口方法）、`crates/service-core/src/session/{service.rs,driver.rs}`（`RunInput` 携带 `ResumePoint`、`RESUME` 条件更新）。

  **实施步骤**：
  1. `RunInput` 增加 `resume: Option<ResumePoint>`；`drive` 在「取许可 → `PAUSED → RUNNING`（条件更新）→ 解析恢复点 → 组装 Context」之后与正常路径合流。
  2. `PAUSED → RUNNING` 是条件更新（`WHERE status = 'PAUSED'`），影响 0 行即「已被别人恢复或已取消」→ 直接返回，不产出事件（幂等的基础）。
  3. 推进成功时 `resume_count += 1`，与状态推进同事务。
  4. 事件：恢复的 Run 重新发 `run_started`（携带同一 `assistant_message_id` 语义由 payload 决定），**不补发**历史 `message_delta`（客户端按 `run_id` + 步骤历史补齐，见 `I-4` / `I-5`）。
  5. 检查点：恢复后继续按 P6A-005 的规则追加，`seq_no` 接着上一次继续（不重置）。

  **验收标准**：
  - [ ] 恢复路径与正常路径的收尾调用是同一函数（静态核对调用点）。
  - [ ] 一次「两轮工具后崩溃 → 恢复」的 Run，最终 `messages` 里助手消息只有一条，工具轮次计数与改动前一致。
  - [ ] 同一个 `PAUSED` Run 被并发恢复两次：只有一次推进成功，另一次不产出任何事件、不改任何行。
  - [ ] 恢复后的检查点 `seq_no` 与崩溃前连续（无重置回 1）。

  **测试要求**：由 P6A-016 的 `resume.crash` / `resume.idempotent` 覆盖，执行需授权。

- [ ] **P6A-011 恢复触发：占用 + 向该会话发一条消息**

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
  - [ ] 「先恢复后 `QUEUED`」的顺序有可复算证据：同一会话同时存在 1 个 `PAUSED` 与本次消息产生的 1 个 `QUEUED` 时，`PAUSED` 那个的 `run_completed` 事件序号早于新 Run 的 `run_started`。
  - [ ] 只占用不发消息：`PAUSED` Run 的 `status`、`updatedAtMs`、`resumeCount` 逐字段不变。
  - [ ] `api/run/resume` 与 `resume_on_occupy` 在全仓检索命中数为 0（未引入）。
  - [ ] `make gen` 的路由数与接口数与改动前相同（恢复不新增接口）；`api/message/create` 的响应结构逐字段不变。

  **测试要求**：由 P6A-016 的 `resume.trigger`（顺序、只占用不恢复、响应结构不变）覆盖，执行需授权。

- [ ] **P6A-012 终态唯一收口：取消 / 关闭 / 恢复中再次中断**

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
  - [ ] `PAUSED` 可取消，结果与普通取消逐字段同形（除 `error` 文案来源）。
  - [ ] 优雅关闭路径对 `PAUSED` 行零改动。
  - [ ] 「恢复中再次强杀」后状态仍是单个 `PAUSED` 行，`resume_count` 单调不减。
  - [ ] 全仓不存在 `UPDATE runs SET status = 'PAUSED'` 以外的 `PAUSED` 写入点，且它带 `WHERE status = 'RUNNING'`。

  **测试要求**：由 P6A-016 的 `resume.terminal` 覆盖（取消、关闭、二次中断、竞态），执行需授权。

---

## Batch D：Run Persistence 可观测与客户端（P6A-013 ~ P6A-016）

> 依赖：Batch C。本批次把「恢复」从内部能力变成可查、可看、可操作的对外能力，并把用例落地。

- [ ] **P6A-013 步骤历史只读接口 `api/run/history`**

  **目标**：一个 Run 的步骤历史（检查点序列 + 恢复次数 + 中断原因）可查，客户端据此重建「中断 → 恢复」的时间线。

  **前置依赖**：P6A-003、P6A-012。

  **涉及文件 / Crate**：`crates/lib-web/src/session/api.rs`、`crates/service-core/src/session/service.rs`（只读查询）、`crates/agent-core/src/session/dto.rs`（`RunHistory` / `RunStep` DTO）、生成物 SDK。

  **实施步骤**：
  1. `GET api/run/history?runId=…` 返回 `RunHistory { run, resumeCount, interruptedReason, steps: Vec<RunStep> }`；`RunStep { seqNo, phase, toolRound, createdAtMs }`。
  2. 只读：不触发恢复、不改状态；`payload` 不外泄（只返回派生字段）。
  3. 未知 Run → 404；`steps` 为空是合法响应（旧 Run 无检查点）。
  4. 与 `api/run/get` 的分工写进接口文档待同步清单：`get` 是状态，`history` 是过程。

  **验收标准**：
  - [ ] 返回结构里没有 `payload` 原文字段；`steps` 按 `seqNo` 升序。
  - [ ] 旧库 Run（无检查点）返回 `steps = []` 且不报错。
  - [ ] 接口总数 +1；`make gen` 门禁通过。

  **测试要求**：由 P6A-016 的 `resume.history` 覆盖（正常、空、404），执行需授权。

- [ ] **P6A-014 `metrics` 增加 run / checkpoint / resume 段**

  **目标**：恢复相关的数字可复算，不靠日志。

  **前置依赖**：P6A-012。

  **涉及文件 / Crate**：`crates/manager-agent/src/runtime/metrics.rs`、`crates/agent-runtime/src/metrics.rs`、`crates/service-core/src/repository/checkpoint.rs`（计数点）、`docs/api/probe.md` 的字段清单（待同步）。

  **实施步骤**：
  1. 新增段 `resume`：`checkpoints_written`、`checkpoints_pruned`、`resume_attempts`、`resume_succeeded`、`resume_rejected`（按拒绝原因分桶）、`resume_unrecoverable`。
  2. 全部在 `metrics` 特性下累加；未开特性时是空实现（与既有采集点同一口径）。
  3. 采集点各自在本层（写检查点、清理、推进、判定），只有一个渲染点（`bin-tests` 的 `util::observability`）。
  4. 不新增阈值日志；不为采集新增 `warn!` / `debug!`。

  **验收标准**：
  - [ ] 六个字段都能在一次「崩溃 → 恢复 → 完成」里被快照读到，且与接口侧事实逐项相等（检查点条数、`resume_count`）。
  - [ ] 未开 `metrics` 的构建里这些字段不存在，且不引入静态内存（`Cargo.toml` 特性链核对）。
  - [ ] 全仓无新增的采集型日志调用（静态核对 `git diff`）。

  **测试要求**：由 P6A-016 的 `resume.metrics` 覆盖，执行需授权。

- [ ] **P6A-015 客户端：展示中断态并说明如何继续**

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
  - [ ] 两端都能展示 `PAUSED` + `interruptedReason`，并展示「发送消息可继续」的提示文案。
  - [ ] 客户端源码里检索不到 `api/run/resume`（未引入恢复动作）；中断态下输入框仍可用。
  - [ ] 单次发消息可能导致两条 Run 的事件（恢复的旧 Run + 新的 Run），两端都按事件里的 `runId` 归类，不按「时间顺序」猜。
  - [ ] `pnpm exec tsc --noEmit` 通过（既有 `make ui-check` 口径）。

  **测试要求**：由 P6A-016 的 `resume.tui` / `resume.web` 覆盖（复用 `skill.tui` / `skill.web` 的驱动方式），执行需授权。

- [ ] **P6A-016 用例落地：`resume.*` 用例域**

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
  - [ ] `make test ARGS="--list"` 列出上述全部用例名。
  - [ ] 每个用例的断言只引用接口响应 / 事件 / 快照字段（静态核对无 `server.log` 读取）。
  - [ ] 用例文件不落在 `crates/bin-tests` 之外的任何 Crate。

  **测试要求**：本任务**就是**用例落地；执行需单独授权（`make test ARGS="resume"`）。未授权时只提交用例代码并保持任务未勾选。

---

## Batch E：Task Recovery 与收尾（P6A-017 ~ P6A-020）

> 依赖：Batch D。本批次把「失败之后怎么办」定死，并把门禁、生成物与阶段结论收口。

- [ ] **P6A-017 失败分类与重试策略**

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
  - [ ] 分类表的每个分支都有对应固定文案，且文案集中在 `error.rs`。
  - [ ] `run_max_retries = 0` 时行为与改动前逐字段一致（既有 `run.*` 用例不变）。
  - [ ] 重试次数在 `metrics` 快照里可复算，且不超过配置上限。
  - [ ] 不可重试路径的 `resume_count` 不增加。

  **测试要求**：由 `resume.retry`（新增，随 P6A-016 一起登记）与既有 `run.*` 回归覆盖，执行需授权。

- [ ] **P6A-018 写放大与恢复开销核对**

  **目标**：确认检查点没有把落库路径变成瓶颈，并用数字（而不是感觉）给出结论。

  **前置依赖**：P6A-016。

  **涉及文件 / Crate**：`crates/agent-runtime/src/metrics.rs`、`crates/service-core/src/repository/checkpoint.rs`、`crates/bin-tests/src/cases/resume/`（只读核对与快照口径）。

  **实施步骤**：
  1. 定义三个可复算指标：单 Run 检查点条数、检查点 payload 平均字节、单次 Run 的落库次数（既有 `metrics` 的落盘计数 + 新增检查点计数）。
  2. 在 `resume.checkpoint` 里断言上界：payload ≤ 2 KiB、两轮工具 Run 的检查点条数 = 5。
  3. 与 P2.1 的落库指标对照：不引入新的同步阻塞点（检查点写入与既有事务同一连接池路径）。
  4. 结论写进验收报告，不写进本文档。

  **验收标准**：
  - [ ] 三个指标都能从快照读到，且用例断言的是上界而不是「看起来还行」。
  - [ ] 检查点写入没有引入 `spawn_blocking` 之外的新阻塞模式（静态核对调用点）。
  - [ ] 单 Run 的检查点条数与「起点 + 每轮模型边界 + 每轮工具边界」之和相等。

  **测试要求**：并入 `resume.checkpoint`；执行需授权。

- [ ] **P6A-019 门禁、生成物与文档待同步清单**

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
  - [ ] `make gen-check` / `make ui-check` / `make check` 退出码均为 0。
  - [ ] 生成物差异清单与 §实施步骤 2 逐项相等，没有多余改动。
  - [ ] 待同步清单以任务结案说明的形式给出，`git status` 里没有文档改动。

  **测试要求**：无（门禁命令不执行测试；`make check` 只跑 fmt + clippy）。

- [ ] **P6A-020 阶段验收与报告**

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
  - [ ] 十组判据逐条有结论（成立 / 未授权 / 未采集），没有空缺。
  - [ ] 报告里所有数字都能追到用例结果或 `metrics` 快照。
  - [ ] 结转风险条目至少覆盖：`PAUSED` 契约影响面、副作用不可回滚的两类未知、迁移不可回滚。

  **测试要求**：无新增用例；报告取证依赖已授权执行的用例结果。
