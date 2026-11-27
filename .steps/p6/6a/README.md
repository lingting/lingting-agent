# 6A：可靠性（Checkpoint / Resume / Run Persistence / Task Recovery）

**状态：已完成（Batch A ~ E 全部结案）。** 20 个任务 / 5 个批次（[tasks.md](tasks.md)），判据 10 组 59 条（[acceptance.md](acceptance.md)），决策 13 项 `D-P6-001` ~ `D-P6-013`（[decisions.md](decisions.md)）。已定案：`D-P6-004` 新增非终态 `PAUSED`、`D-P6-007` 一律不重放并回灌中断原因、`D-P6-009` 由「占用 + 发消息」触发恢复。**阶段报告见 [`docs/reports/p6a-020-acceptance.md`](../../../docs/reports/p6a-020-acceptance.md)**：判据除 `O-3`（未采集：无 `metrics` 特性的构建不注册该探针）外全部成立；结转风险 `KI-P6A-001` ~ `KI-P6A-008`。
**复核轮补正（2026-11-28）**：三处代码补正与文档同步已按授权完成，明细见报告 §八 —— ① 检查点裁剪收敛到推进终态的同一事务（原实现只在取消时事后清理，`COMPLETED` / `FAILED` 会漏）；② 起点检查点补记 `assistant_message_id`，恢复重发的 `run_started` 与收尾复用它（`P6A-005` 第 1 条原为悬空项）；③ 文档（`docs/api/session.md` / `docs/api/probe.md` / `docs/workspace.md` / `docs/develop.md`）与 `.steps/` 状态行同步，`acceptance.md` 的重复编号修正并新增 `S-4`。
> 依据：阶段级规划 [`.steps/p6/README.md`](../README.md) 第二节「可靠性」与第六节验收方向 1、第八节风险 1 / 2 / 8；P5 结案结论与结转风险 [`docs/reports/p5-020-acceptance.md`](../../../docs/reports/p5-020-acceptance.md)（`KI-P5-001` ~ `KI-P5-004`）；结构权威 [`docs/workspace.md`](../../../docs/workspace.md)；开发规范 [`docs/develop.md`](../../../docs/develop.md)；仓库根 `AGENTS.md`。
>
> 凡文档与代码不一致之处，以代码为准并记入 [decisions.md](decisions.md)。

---

## 一、子阶段目标

把「一次 Run 只在进程活着的时候有意义」变成「一次 Run 有可恢复的进度与唯一的结局」。四个目标各自有独立结案证据：

| 目标 | 内容 | 结案证据 |
|---|---|---|
| **G-1 Checkpoint** | Run 在**已提交的边界**上留下检查点：模型调用完成、工具批次提交、工具确认挂起。检查点有格式版本、有单调序号、与已提交的消息/工具轨迹可交叉校验 | `C-1` ~ `C-7`：三种 phase 各有落点，`seq_no` 无缺口，未知版本判不可恢复 |
| **G-2 Resume** | 进程崩溃（含强杀）后，残留 Run 进入**可恢复的中间态**而不是直接判死；客户端重新占用会话并**向该会话发一条消息**即触发恢复：从最近检查点继续，不重跑已提交的轮次，**不重放任何工具调用**；同一条消息产生的新 Run 在恢复之后才执行 | `R-1` ~ `R-9`：`resume.crash` 在强杀 + 重启 + 占用 + 发消息后继续并完成，`resume.idempotent` 断言重复触发不产生第二次执行 |
| **G-3 Run Persistence** | Run 的状态机推进与步骤历史可读、可审计：状态集合明确、终态唯一、检查点可查；升级（`0003` 迁移）后旧数据可读 | `S-1` ~ `S-7`、`P-1` ~ `P-5`、`I-3`：终态不被二次改写，旧库升级后既有行可读 |
| **G-4 Task Recovery** | 失败被分类：可重试（上游瞬时错误、工具 `Retryable`）按上限重试并有计数；不可重试（结构性问题、轨迹损坏、检查点不可用）直接落终态并给出固定原因 | `E-1` ~ `E-5`、`C-7`、`R-6`：不可重试路径逐条给出固定文案，重试次数可在 `metrics` 快照复算 |

四者的关系：**G-1 是 G-2 的前置**（没有检查点就没有「从哪儿继续」）；**G-3 是 G-1 的落点**（检查点必须进 SQLite，且旧数据要能读）；**G-4 与 G-2 共用同一套「能不能恢复」的判定**（不可重试 = 不可恢复 = 落终态）。G-4 不引入新的事件类型，也不引入 Run 级自动重启。

本子阶段**不新增能力面**：不加记忆、不加检索、不加多 Agent、不加沙箱、不加 OTel。它只让已有的 Run 生命周期**可中断、可继续、可解释**。

---

## 二、边界

### 2.1 In Scope

| 层 | 改动面 |
|---|---|
| `crates/agent-core` | `RunStatus` 增加可恢复中间态；新增检查点领域类型（`RunCheckpoint` / `CheckpointPhase` / 格式版本常量） |
| `crates/lib-db` | 迁移 `0003`（检查点表、`runs` 恢复列、`tool_calls` 开始标记）；沿用「`ALTER TABLE` 由 `PRAGMA table_info` 预检」的幂等做法 |
| `crates/service-core` | 检查点的写/读/截断单点（`repository/checkpoint.rs`）；恢复相关的条件更新（`START` / `RESUME` / `CANCEL` / 自愈）；工具审计的「执行前写 `PENDING`、执行后有条件更新」 |
| `crates/agent-core` / `agent-runtime` | 轮次边界写检查点；恢复点解析（检查点 ↔ 消息 / 工具轨迹一致性）；中断通知（未配对调用一律不执行、回灌中断原因）；Run 级重试的分类与上限 |
| `crates/agent-tool` | 只给 `ToolCallRecord` 增加「开始时间」字段（缺省 `None`）；**不新增**可重放自述、不改执行器的既有重试与超时语义 |
| `crates/manager-agent` | 启动自愈从「一律判死」改为「落可恢复态」；会话被占用时拾起可恢复 Run；恢复的事件续写 |
| `crates/lib-web` | 新增步骤历史查询（恢复**不新增接口**：由发消息触发）；错误码与文案单一产点 |
| `crates/lib-bin-cli` / `lib-bin-tui` / UI | 客户端展示「中断 / 可恢复」并给出「发消息即可继续」的提示（**不新增恢复按钮 / 恢复接口**）；`[agent]` 重试配置项的装载与启动期校验 |
| `crates/bin-tests` | 新增 `resume.*` 用例域；改写 `persist.running_heal`（自愈语义变了）；`tool.audit` / `run.*` 的既有断言按新口径复核 |

### 2.2 Out of Scope（本子阶段明确不做）

1. **不拆微服务、不做分布式调度、不做多机集群**：恢复只在单进程、单 SQLite 文件内成立（`AGENTS.md` 第 18 条、P6 Non-goals）。
2. **不把协议类型引入 Agent Core**：`rig-core` / `rmcp` / `framework-*` 的类型不得出现在 `agent-core` 的检查点类型里（`AGENTS.md` 第 24 条）；检查点 payload 用领域自有结构 + `serde`，不落 Provider 原始响应。
3. **不提前做 6B ~ 6E**：Memory / 压缩 / RAG（6B）、Multi-Agent / Workflow（6C）、OTel / 评测（6D）、沙箱 / 资源隔离 / 发布（6E）一律不在本子阶段；本子阶段只落地「Run 中断后可继续」这一条。
4. **不改 P5 已定稿的 Skill 语义**：索引常驻系统提示、正文按需加载、`skill_load` 是唯一工具形态、三级披露与硬预算口径全部不变（D-P5-002 / D-P5-003 / D-P5-010 / D-P5-016）；恢复时**重新组装 Context**，不落 Skill 快照（见 §五 待讨论 4）。
5. **不改 Run 的触发规则**：Run 仍然只在「所属会话被占用」时执行（D-P1.1-004）；恢复不引入「无人值守 worker」。
6. **不把事件文件变成恢复的事实源**：恢复只读 SQLite；事件文件继续承担客户端重放与排障（见 §四）。
7. **不做副作用补偿事务**：不引入分布式事务、不引入「工具回滚」、不给第三方工具加幂等键协议；副作用只能做到「不重复执行 + 明确告知模型」。
8. **不改事件文件轮转的默认值**（`max_sessions = 200` / `max_bytes = 512 MiB`）：是否保护可恢复会话目录留作讨论（D-P6-013）。
9. **不做热升级 / 滚动重启 / 迁移回滚**：`0003` 只有前进迁移，升级前的备份要求写进人工判定项（`P-4`）。
10. **不修 P5 结转风险**：`KI-P5-001` ~ `KI-P5-004` 不在本子阶段处理；若其中某项与恢复有交集（`KI-P5-003` 的路径基准），只在相关判据里显式标注沿用现状。

---

## 三、对 P2 / P3 的具体改动面

### 3.1 P2 持久化：SQLite

现状（`crates/lib-db/migrations/0001_init.sql` + `0002_tool_calls.sql`）：

- `sessions` / `runs` / `messages` / `tool_calls` 四张表；枚举列存 serde wire 值、不加 `CHECK`。
- 终态写入一律是**条件更新**（`WHERE status = 'RUNNING'`），影响 0 行即说明已被取消或已终态（D-P2-030）。
- 启动自愈 `with_orphan_healing`：`RUNNING → FAILED`（`HEAL_RUNNING`），`QUEUED` / `WAITING → CANCELLED`（`HEAL_QUEUED`），同一事务完成。
- `messages` 的 `UNIQUE(session_id, seq)` 保证顺序；工具轨迹是「助手消息（带 `tool_calls`）+ 每条结果一条 TOOL 消息」。

本子阶段的改动：

| 对象 | 改动 | 目的 |
|---|---|---|
| `runs.status` | 新增非终态 `PAUSED`（D-P6-004） | 崩溃残留不再一律判死 |
| `runs` 新列 | `interrupted_reason` / `resume_count` / `last_checkpoint_seq` | 中断原因可读、恢复次数可计、检查点指针可从 Run 直接拿 |
| 新表 `run_checkpoints` | `(run_id, seq_no)` 唯一，`phase` / `tool_round` / `last_message_seq` / `payload` / `format_version` | 检查点有格式、有版本、有单调序号 |
| `tool_calls` 新列 | `started_at_ms` + 「执行前写 `PENDING`」 | 区分「从未开始」与「可能已执行」，是中断通知口径与审计完整性的唯一依据 |
| 迁移 | `0003_checkpoints.sql`，沿用 `PRAGMA table_info` 预检 | `ADD COLUMN` 幂等，旧库可升级 |
| 自愈 | `RUNNING → PAUSED`（携带中断原因）；`QUEUED` / `WAITING → CANCELLED` 保持不变 | 只有「非预期中断」才可恢复（D-P6-005） |
| 检查点保留 | Run 落终态时按策略删除该 Run 的检查点（保留最近 N 条的实现细节见 `C-4`） | 不让检查点表无限增长 |

**不改**：`sessions`、`messages` 的结构与 `seq` 语义；`api/session/*`、`api/message/*` 的既有字段；`tool_calls` 的既有列含义（只加列）。

### 3.2 P2 持久化：Run 状态机

现状：`QUEUED / RUNNING / WAITING / COMPLETED / CANCELLED / FAILED`（`crates/agent-core/src/session/dto.rs`），其中 `COMPLETED / CANCELLED / FAILED` 是终态，`WAITING` 是挂起等确认（不占并发许可、不阻塞同会话后续 Run）。

改动：加入 `PAUSED`，并把「合法前置状态」写成一张显式表（D-P6-004 / D-P6-005）：

| 迁移 | 允许的前置状态 | 触发者 |
|---|---|---|
| `QUEUED → RUNNING` | `QUEUED` | 执行队列拿到全局许可后 |
| `RUNNING → WAITING` | `RUNNING` | 命中工具确认点（D-P3-027） |
| `WAITING → RUNNING` | `WAITING` | 客户端回传确认决定 |
| `RUNNING → PAUSED` | `RUNNING` | **仅启动自愈**（进程消失后的第一次装配） |
| `PAUSED → RUNNING` | `PAUSED` | 会话被占用后，该会话收到一条新消息（`api/message/create`）时的队列拾取 |
| `QUEUED / RUNNING / WAITING / PAUSED → CANCELLED` | 四个非终态 | 取消 / 会话停止 / 优雅关闭 |
| `RUNNING → COMPLETED / FAILED` | `RUNNING` | 正常收尾 / 结构性失败 |

「终态唯一」不变：任何路径都不得改写已是 `COMPLETED / CANCELLED / FAILED` 的行（`S-3`）。恢复成功与正常完成走同一条收尾事务，因此不可能出现「恢复后多出一条助手消息」。

### 3.3 P2 持久化：事件文件（与 P1.1 的关系，见 §四）

改动：**无**。检查点不写事件文件；恢复产生新的运行时会话，事件写进新文件。

### 3.4 P3 工具执行

现状（`crates/agent-tool/src/executor.rs`、`crates/agent-runtime/src/loop_/tool.rs`）：

- 执行前发 `tool_call_started` 事件；执行后 `audit()` 写 `tool_calls`，再 `append()` 写消息，同一轮事务提交。
- 重试只对 `Tool::retryable(&error)` 为真的错误，上限 `limits.max_retries`；超时、权限、参数非法都是**工具结果**（P3-016）。
- 确认点：`ToolError::ConfirmRequired` → 写 `PENDING` 审计行 → `RUNNING → WAITING` → `drive` 返回 `Suspended`。

改动：

| 对象 | 改动 | 目的 |
|---|---|---|
| `Tool` 契约 | **不改**：不新增「可重放」自述（D-P6-007 定案「一律不重放」） | 恢复语义不依赖工具自述，工具契约保持 P3 / P4 原样 |
| `ToolExecutor` | **不改**：重试、超时、权限、参数校验零改动 | 不破坏 P3 已定稿的错误分类 |
| 审计写入时机 | 执行**前**写 `PENDING`（`started_at_ms` 有值），执行**后**按 `(run_id, tool_call_id)` 条件更新 | 崩溃时「有开始无结束」可见，中断通知的口径可区分 |
| 审计查询口径 | `PENDING` 行在 `api/tool/audit/list` 里是否可见由 D-P6-008 定 | 避免查询侧出现「永远不结束的行」 |
| 恢复回灌 | 未配对的调用**一律不执行**；每个未配对调用落一条 `ERROR` 审计 + 一条 TOOL 结果消息，文本为「上次执行被中断：<简短原因>」（原因取自 `runs.interrupted_reason` 的白名单映射，不拼参数、不含路径） | 不重复任何副作用，同时让模型知道「上次没跑完」（D-P6-006 / D-P6-007） |

**不改**：工具权限判定、`file_access` 推导的并发语义、`max_result_chars` 截断、确认点的挂起与唤醒机制、`ConfirmRequired` 的既有含义。

---

## 四、与 P1.1 事件文件轮转的关系

现状（`crates/manager-agent/src/runtime/journal.rs`）：

- 一条运行时会话一个文件：`<data>/events/<sessionId>/<runtimeSessionId>.jsonl`，一行一条 `RunEvent`。
- 文件是**事件**的唯一事实源：`seq` 由写者持有并覆写（不信任事件自带值），写入逐条 flush、不 `sync_all`。
- 轮转按会话目录计量：`max_sessions`（缺省 200）与 `max_bytes`（缺省 512 MiB）任一超限即删最旧；**占用中 / 写入中的目录只统计、不删**。

本子阶段对它的结论与约束：

1. **事件文件不是恢复事实源**（D-P6-002）。理由是本子阶段必须写进决策的三条硬事实：
   - 轮转**会删掉**整个会话目录，包括含「可恢复 Run」事件的文件——可恢复性不能依赖随时会消失的数据。
   - `seq` 由写者持有且**每次运行时会话从 0 重新计数**；同一 Run 跨进程的事件序号本就不连续，无法当作「进度指针」。
   - 事件文件不 `sync_all`，强杀后末尾可能缺行：它适合重放与排障，不适合作为「已提交边界」的判据。
2. **恢复不修改既有事件文件**。恢复产生新的运行时会话与新的文件；客户端通过 `run_id` 把两次运行的事件串起来，不假设 `seq` 连续（`R-8` 断言「删掉旧事件文件仍能恢复」，`I-4` / `I-5` 断言客户端按 `run_id` 归并）。
3. **检查点不写事件文件**。检查点只进 SQLite；事件里不新增「检查点」事件类型（`K-6`）。
4. **轮转默认值不变**。是否把「含 `PAUSED` Run 的会话目录」纳入 `protected` 留作议题（D-P6-013，本子阶段**不采纳**）：只影响事件保留时长，不影响可恢复性。
5. **轮转与恢复互不阻塞**：轮转在后台按占用表快照删除、不二次探测；恢复只读 SQLite，因此两者没有共享锁，也不需要新的协调点。

---

## 五、风险与待讨论问题

### 5.1 风险

| 编号 | 风险 | 说明与缓解方向 |
|---|---|---|
| **RK-6A-01** | 检查点写放大 | 每轮 2 次检查点写入，与 P2.1 优化过的落库路径相邻；缓解：检查点 payload 只存最小事实（不存上下文全文），`C-4` 断言其字节上界，`O-2` 用 `metrics` 快照复算写次数 |
| **RK-6A-02** | 副作用不可回滚 | 工具执行期间崩溃 = 副作用可能已发生且无审计；缓解：执行前写 `PENDING`（P6A-006）让「有开始无结束」可见，恢复时**一律不重放**并回灌「上次执行被中断：<简短原因>」（D-P6-007），把这一条写进阶段的显式风险而不是隐藏 |
| **RK-6A-03** | `PAUSED` 是契约破坏 | 新增枚举值会影响两份 SDK 的生成产物与 TUI / WebUI 的穷尽匹配；缓解：`B-4` / `I-4` / `I-5` 三处门禁 + `P6A-019` 的生成物重建，`S-1` 断言旧值集合不变 |
| **RK-6A-04** | 自愈语义变更会改既有用例 | `persist.running_heal` 直接断言 `FAILED` + `HEAL_RUNNING`；缓解：把改写放进 `P6A-009` 的验收标准，并在 `K-3` 里显式登记「这条用例的口径变了」 |
| **RK-6A-05** | 恢复与并发许可 / 占用语义叠加 | 恢复以「会话被占用」为前提，而占用不跨重启（`persist.occupancy_reset`）；缓解：D-P6-009 定死「占用 + 发消息」是唯一触发，D-P6-011 定死恢复走与正常执行相同的许可与队列路径；`R-2` 断言「只占用不发消息不恢复」 |
| **RK-6A-06** | 恢复后 Context 与当时不一致 | 重启后 `AGENTS.md`、Skill 集合、Provider 配置都可能变；缓解：检查点只记录「已提交的消息上界」，Context **重新组装**并在 `I-3` 的步骤历史里显示「本次恢复重新组装了上下文」，不承诺上下文逐字节一致 |
| **RK-6A-07** | 迁移不可回滚 | `lib-db` 只做前进迁移；缓解：`P-4` 改为人工判定项（升级前备份），把回滚策略登记为 6E 的输入 |
| **RK-6A-08** | 检查点表增长 | 长时间运行的 Run 每轮 2 条；缓解：Run 落终态时按策略清理（`C-4`），`O-2` 断言终态后剩余条数 |
| **RK-6A-09** | 用例的强杀窗口不稳定 | `resume.crash` 依赖「在工具执行中强杀」，窗口窄；缓解：用可复算的挂起点（确认点 `WAITING` 与进度事件）而不是 sleep，`R-1` 写明判定动作 |

### 5.2 待讨论问题（开工前必须定）

1. ~~**`PAUSED` 是否可接受为契约变更？**~~ **已定案（本轮）**：接受，新增非终态 `PAUSED`（D-P6-004）。
2. ~~**恢复由谁触发？**~~ **已定案（本轮）**：客户端重新占用会话后，**向该会话发一条消息**即触发恢复；不新增恢复接口、占用本身不触发（D-P6-009）。
3. ~~**崩溃时正在执行的工具怎么算？**~~ **已定案（本轮）**：**一律不重放**；直接通知「上次执行被中断」并附上可获得的简短原因（D-P6-007）。
4. **是否记录「上下文指纹」？** 记录可在恢复时发现上下文漂移，但会与 P5 的 Skill 集合、`AGENTS.md` 变更相互纠缠。本子阶段不记录（D-P6-003 的否决方案里保留）。
5. **检查点是否要保留历史（可查看每一次边界）？** 保留利于审计但会放大写与存储；`P6A-013` 的步骤历史接口当前口径是「按检查点列出，不保证长期保留」。
6. **`PENDING` 审计行是否出现在 `api/tool/audit/list`？** 定案见 D-P6-008。
7. **轮转是否保护含 `PAUSED` 的会话目录？** 本子阶段不采纳（D-P6-013），若后续要求「事件层也能追溯中断前的重放」，需要重新评估。
8. **Run 级重试归谁？** 运行时内的轮次重试（现状：只有工具级重试）还是装配层的「重新入队」？定案见 D-P6-012。

### 5.3 与 P5 结转风险的关系

| 编号 | 与本子阶段的关系 |
|---|---|
| `KI-P5-001` | 无关。`skill.get` 对 `INVALID` 的口径不变；6A 不触碰 Skill 接口 |
| `KI-P5-002` | 无关。索引极端预算下的空小节行为不变（`K-1` 回归） |
| `KI-P5-003` | 观察项。恢复时 Context 重新组装，引用的路径基准仍是「相对应用根目录」；不在本子阶段修，`K-1` 保持原断言 |
| `KI-P5-004` | 观察项。`resume.*` 用例同样使用隔离服务端与动态端口，若再次出现端口竞争，按「测试设施偶发风险」记录，不计入 6A 判据 |

---

## 六、批次与执行顺序

批次之间强序，批次内可并行。详见 [tasks.md](tasks.md)。

| 批次 | 主题 | 任务 | 批次出口 |
|---|---|---|---|
| **A** | 事实源、状态机与检查点存储 | P6A-001 ~ 004 | `PAUSED` 进状态机、`0003` 迁移可重复执行、检查点能写能读且带格式版本 |
| **B** | 循环内检查点与重复执行边界 | P6A-005 ~ 008 | 三种 phase 有落点；崩溃能区分「未开始 / 可能已执行」；损坏可判不可恢复 |
| **C** | 自愈、恢复触发与装配 | P6A-009 ~ 012 | 强杀后是 `PAUSED`；占用 + 发消息能从检查点继续并跑完、且发生在该消息的新 Run 之前；取消 / 关闭仍走终态 |
| **D** | Run Persistence 可观测与客户端 | P6A-013 ~ 016 | 步骤历史可查、`metrics` 段可复算、客户端能看到中断态并说明如何继续、`resume.*` 用例落地 |
| **E** | Task Recovery 与收尾 | P6A-017 ~ 020 | 失败分类与重试有上限与计数；门禁与生成物一致；验收报告出稿 |

---

## 七、执行纪律

- **测试只写在 `crates/bin-tests`**（`AGENTS.md` 第 28 条）；其他 Crate 不得出现 `#[cfg(test)]`。为测试提供的入口必须由 feature 隔离且只在 `bin-tests` 启用。
- **任何用例执行都必须先取得明确授权**；未授权时只推进代码、静态检查与文档部分，相关验收项保持未勾选并在报告里写「未授权」。
- **数据只来自接口响应、客户端事件与 `metrics` 快照**；不得读取、解析或检索 `server.log` 与日志目录（`AGENTS.md` 测试数据采集条款）。
- **本轮只改 `.steps/` 下的文档**：不动 `crates/`，不改 `README.md` / `docs/develop.md` / `docs/workspace.md`，不跑测试。
