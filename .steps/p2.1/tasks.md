# P2.1 任务清单

**状态：未开始（15 个任务 / 5 个批次）。** 阶段目标、边界、根因分析与判据见本目录 [README.md](README.md)；`acceptance.md` / `decisions.md` 在 Batch A 结束、假设定性后展开。

批次之间强序，批次内可并行。**每个任务的「测试要求」只允许在 `crates/bin-tests` 落地用例；其他 Crate 不得写 `#[cfg(test)]`**（仓库 `AGENTS.md` 第 28 条）。**所有需要执行用例的步骤都必须先取得明确授权**；未授权时只推进代码审查、静态检查与文档部分，相关验收项保持未勾选。

本阶段的核心约束：**不做测试侧限流**。`cli::default_concurrency()` 不得改为硬上限，`runner` 不得新增「共享分片许可」；降并发只用于定位对照。K-1 的结案口径是「修根因 + 默认并发下连续全绿 + 指标可解释」。

---

## Batch A：量测与可观测性（P2.1-001 ~ P2.1-003）

> 依赖：无。**本批次先不改修复性代码**：001 取基线，002 / 003 只加观测。003 完成后才能对 H-1 ~ H-4 定性。

- [ ] **P2.1-001 复现基线与失败画像**

  **目标**：把 K-1 从「印象」变成可复算的数据：默认并发下的失败集合、耗时分布、失败是否压在用例内部上限，以及与低并发 / 单跑的对照。
  **前置依赖**：无。
  **涉及文件 / Crate**：不改代码；结论写进本阶段报告草稿（`docs/reports/` 下的 P2.1 报告，Batch D 定稿）。
  **实施步骤**：
  1. 默认并发（不传 `--concurrency`）跑 `make test ARGS="--all"` **3 次**，逐次记录：并发度、失败用例、失败耗时、总计耗时。
  2. 低并发对照：`make test ARGS="--all --concurrency 4"` **1 次**，记录同一组字段。
  3. 对每次失败用例单独跑一次，记录单独耗时与是否通过。
  4. 核对失败耗时是否恰好等于用例内部等待上限（`cases/tui.rs` 的 `STREAM_TIMEOUT`、`runner` 的用例总超时）。
  **验收标准**：
  - [ ] 三次默认并发的失败集合、耗时、并发度成表，且能看出「失败集合逐次变化」。
  - [ ] 低并发与单跑的结果在同一张表里可对照。
  - [ ] 明确写出「失败耗时是否压在内部上限」的结论。
  **测试要求**：`make test ARGS="--all"` × 3、`make test ARGS="--all --concurrency 4"`、失败用例单跑。**需授权**。

- [ ] **P2.1-002 测试进程阻塞点审计**

  **目标**：确认 H-1（`async` 上下文里的同步等待阻塞 Tokio worker）是否成立，并量化其规模。
  **前置依赖**：P2.1-001（有基线才能对比）。
  **涉及文件 / Crate**：`crates/lib-bin-tui/src/testing.rs`、`crates/bin-tests/src/cases/tui.rs`、`crates/bin-tests/src/cases/tui/*.rs`。
  **实施步骤**：
  1. 在 `cases/` 与 `lib-bin-tui/src` 内检索 `async` 上下文中的 `std::thread::sleep`、阻塞 `recv`、`block_on`、长时间同步循环，形成清单（文件 + 调用点 + 最长阻塞时长）。
  2. 重点核对 `Harness::wait_until`：确认它是同步轮询，且被 `cases/tui/*` 的 `async fn` 直接调用；列出全部调用点。
  3. 在 `wait_until` 入口加「本次等待时长」累计（进程内 `AtomicU64`），并暴露只读快照给 `bin-tests` 读取（`testing` feature 内）。
  4. 在默认并发下读一次累计值，换算「单次运行累计阻塞 worker 秒数」。
  **技术要求**：只加观测，不改等待实现（改实现归 P2.1-004），保证本轮基线可比。
  **验收标准**：
  - [ ] 阻塞点清单完整，且每个点标注所属用例与最长阻塞时长。
  - [ ] 能给出默认并发下单次运行的累计阻塞 worker 时长。
  - [ ] 用例行为与耗时特征与 P2.1-001 基线一致（加观测未改变行为）。
  **测试要求**：`make test ARGS="tui --all"` 与一次 `--all` 取累计值。**需授权**。

- [ ] **P2.1-003 服务端并发可观测性**

  **目标**：让「慢在哪一段」可观测：Run 排队 / 执行、断连清理链、`stop_session` 各阶段、事件落盘与转发、数据库取连接、下行通道满。
  **前置依赖**：无（可与 P2.1-002 并行）。
  **涉及文件 / Crate**：`crates/agent-runtime/src/{limit,loop_,port}.rs`、`crates/manager-agent/src/runtime/{handler,session,registry}.rs`、`crates/manager-agent/src/runner/queue.rs`、`crates/lib-db/src/pool.rs`、`crates/bin-tests/src/util/`（读取入口）。
  **实施步骤**：
  1. 在 `limit::acquire` 前后记录排队时长与当前持有数；排队超过阈值（建议 100ms）打 `debug`，超过 1s 打 `warn`，都带 `run_id`。
  2. 在 `RuntimeSession::stop` 的四个阶段（DB 取消 / `executor.stop` / 写者收尾 / 总时长）分别计时，超阈值（建议 500ms）打 `warn`，带 `session_id`。
  3. 在 `handler::process` 的进入 / 退出计时，记录本次处理的客户端数量与排队情况。
  4. 在 `journal::append` 记录单帧写入耗时（阈值触发），在 `write_event` 记录 `downlink.forward` 的返回（是否因通道满被丢弃）。
  5. 在 `Database::begin` / 取连接处记录等待时长（阈值触发）。
  6. 进程内指标：同时 `RUNNING` 数、排队数、通道满次数、`session_locks` 条目数、清理链并行度；只聚合，不新增 HTTP 接口、不新增配置项。
  **技术要求**：阈值先比较后构造日志参数，避免热路径格式化开销；日志级别默认 `debug`；指标读取只用于测试与诊断，不加公开 API 的语义。
  **验收标准**：
  - [ ] 默认并发下可读出：同时 `RUNNING` 数、Run 排队时长分布、清理链耗时、通道满次数、`session_locks` 条目数、DB 取连接等待。
  - [ ] 新增日志在无异常时不产生每帧一条的 `info` 噪声。
  - [ ] 25 条接口、`RunEvent` 契约零变更。
  **测试要求**：一次 `--all` 采集指标快照。**需授权**。

---

## Batch B：并发根因修复（P2.1-004 ~ P2.1-008）

> 依赖：P2.1-001 ~ P2.1-003。**一次只改一处，改完单独跑一轮全量**，禁止多处同改；每处修复在报告里保留「修复前 / 修复后」数据。

- [ ] **P2.1-004 消除测试进程同步阻塞（H-1）**

  **目标**：`async` 用例不再用同步 sleep 等结果，释放被占用的 worker。
  **前置依赖**：P2.1-002。
  **涉及文件 / Crate**：`crates/lib-bin-tui/src/testing.rs`、`crates/bin-tests/src/cases/tui.rs`、`crates/bin-tests/src/cases/tui/*.rs`。
  **实施步骤**：
  1. 在 `testing` feature 内新增异步等待入口 `wait_until_async`：内部 `pump()` + `tokio::time::sleep(POLL_INTERVAL)`，超时返回 `false`。
  2. 把 `cases/tui/*` 中 `async fn` 内的 `wait_until` 调用改为 `wait_until_async`；保留同步 `wait_until` 给非 `async` 场景，并在文档注释写明「禁止在 `async` 上下文调用」。
  3. 复查其它同步阻塞：`cases/tui.rs` 的公共等待、`lib-bin-tui` 的 `pump` 内部是否有阻塞 IO。
  4. 只改这一处后跑一轮默认并发 `--all`，与 P2.1-001 基线对比。
  **验收标准**：
  - [ ] `cases/` 内不再有 `async` 上下文中的同步 sleep / 阻塞等待。
  - [ ] 单独修复后的全量结果与基线对比成表；失败数下降或剩余失败有明确归因（指向 H-2 / H-3）。
  - [ ] 用例断言与用例名零变更。
  **测试要求**：`make test ARGS="--all"`（默认并发）× 2。**需授权**。

- [ ] **P2.1-005 断连链按客户端并行（H-2）**

  **目标**：不同客户端的断连清理互不排队；同一客户端的清理顺序不变。
  **前置依赖**：P2.1-003（需要清理链耗时数据）。
  **涉及文件 / Crate**：`crates/manager-agent/src/runtime/handler.rs`。
  **实施步骤**：
  1. 把全局 `CHAIN: tokio::sync::Mutex<()>` 的语义收窄为「同一 `client_id` 串行」：`take_marks()` 一次取出全部标记后，按客户端并行清理（例如每客户端一把 `Arc<Mutex<()>>`，或一次只由一个任务处理某客户端的标记）。
  2. 保持顺序语义不变：`client_closed` 帧 → 反注册 → 停运行时会话 → 撤回占用；任一步失败继续后续步骤。
  3. 保留 `process` 的「合并通知」语义：一次唤醒处理当前全部标记，清理中新增的标记由下一次唤醒处理。
  4. `shutdown` 路径保持「先给全部客户端打 `ServerShutdown` 标记，再跑清理链」。
  **技术要求**：只改并发范围，不改步骤顺序与失败处理；并行任务的错误必须逐条记录，不得因单点失败中断其它客户端。
  **验收标准**：
  - [ ] 多客户端同时断连时不互相排队（用 P2.1-003 的指标或日志证明）。
  - [ ] 同一客户端不会并发执行两次清理。
  - [ ] `runtime.disconnect` / `runtime.handler` / `tui.reconnect` / `runtime.shutdown` 全通过。
  **测试要求**：`make test ARGS="runtime tui"`、`make test ARGS="--all"`。**需授权**。

- [ ] **P2.1-006 收尾等待移出会话锁（H-2）**

  **目标**：会话锁只保护状态切换，不再等「执行队列收尾（≤5s）」与「写者 flush」。
  **前置依赖**：P2.1-005。
  **涉及文件 / Crate**：`crates/manager-agent/src/runtime/{handler,session,registry}.rs`、`crates/manager-agent/src/runner/queue.rs`。
  **实施步骤**：
  1. 明确 `clean` 的关键路径：锁内只做「从注册表摘下运行时会话 + 标记停止」，写库取消与收尾等待移出锁。
  2. 收尾改为后台任务：`executor.stop` + 写者 `task.await` 在后台执行，失败 / 超时打 `warn`；关闭（`shutdown`）路径仍要求「等全部收尾完成」再返回。
  3. 检查「摘下会话」与「新占用建立会话」的竞态：新占用不得与旧运行时会话共用同一事件文件（这是原有顺序语义的根因，不能破）。
  4. 用 P2.1-003 的日志对比修复前后的「会话锁持有时间」与「清理链耗时」。
  **技术要求**：进程内只有一处可以「等收尾」——关闭流程；常规断连 / 释放路径一律异步收尾 + 超时兜底。
  **验收标准**：
  - [ ] 会话锁持有时间与收尾时长解耦（日志可证）。
  - [ ] 占用 / 释放不被断连收尾拖住。
  - [ ] 事件文件顺序与终态事件仍正确（`runtime.journal` / `run.journal_consistency` 通过）。
  **测试要求**：`make test ARGS="runtime run"`、`make test ARGS="--all"`。**需授权**。

- [ ] **P2.1-007 会话锁表治理（H-4）**

  **目标**：`session_locks` 不再随历史会话数单调增长，取锁不再与所有读操作争写锁。
  **前置依赖**：P2.1-006（避免与收尾路径同时改）。
  **涉及文件 / Crate**：`crates/manager-agent/src/runtime/registry.rs`。
  **实施步骤**：
  1. 取锁先走读锁快路径（命中直接 clone），未命中再降级到写锁插入。
  2. 增加回收：会话停止 / 占用释放后，在没有其它持有者时移除条目（用 `Arc::strong_count` 判定或显式引用计数）；回收失败只记 `debug`，不得阻塞路径。
  3. 用 P2.1-003 的条目数指标验证长跑后的增长曲线。
  **技术要求**：不得引入新依赖；回收必须与「取锁快照」并发安全。
  **验收标准**：
  - [ ] 长跑后 `session_locks` 条目数不随历史会话数单调增长。
  - [ ] 同一会话的取锁仍唯一（建立 / 停止 / 断连三处共用同一把锁）。
  - [ ] `runtime.occupancy` / `runtime.handler` 通过。
  **测试要求**：`make test ARGS="runtime"`、长跑一次 `--all` 后读条目数。**需授权**。

- [ ] **P2.1-008 许可与写路径复核（H-3）**

  **目标**：用数据回答「全局 Run 许可 4 与 journal 逐帧 flush 是不是瓶颈」，而不是猜。
  **前置依赖**：P2.1-003、P2.1-004 ~ P2.1-007（先排除已修项）。
  **涉及文件 / Crate**：`crates/agent-runtime/src/{limit,port,loop_}.rs`、`crates/manager-agent/src/runtime/{session,journal}.rs`。
  **实施步骤**：
  1. 读默认并发下的排队时长分布：若长期为 0 → 许可不是瓶颈，出「不改」结论；若有明显排队且此时 CPU 空闲 → 许可可能是瓶颈，进入第 2 步。
  2. 若许可被判定为瓶颈：**只记录结论与数据**（当前值、排队分布、CPU 利用率），不在本任务调整默认值；并发配置的数值与语法调整统一归 P2.1-015，其硬前置是 P2.1-013。不得改测试侧并发。
  3. 读 journal 单帧写入耗时：若单帧耗时叠加显著，改为有界批量 flush（保持「先落盘再转发」的顺序与 flush 语义），否则出「不改」结论。
  4. 任何调整都附前后对照数据。
  **技术要求**：结论必须能从 P2.1-003 的指标复算；「不改」也是合格结论，需写明理由与数据。
  **验收标准**：
  - [ ] 给出「许可是否瓶颈」「journal 是否瓶颈」两个明确结论与数据；本任务不改并发配置的默认值。
  - [ ] 如调整了 journal 写入方式，附修复前后对比，且 `git diff` 里没有测试侧限流改动。
  - [ ] 事件顺序语义不变（先落盘 flush 再转发）。
  **测试要求**：`make test ARGS="--all"` 前后各一次。**需授权**。

---

## Batch C：测试基础设施整理（P2.1-009 ~ P2.1-011）

> 依赖：无（可与 Batch A / B 并行）。**只做位移，不改行为**；本批次完成后才能跑 Batch D 的全量回归，保证归因唯一。

- [ ] **P2.1-009 Mock 迁出 `cases`**

  **目标**：`cases/` 只留用例，Mock 成为独立支持模块。
  **前置依赖**：无。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/llm/mock/*` → `crates/bin-tests/src/mock/llm/*`；`src/main.rs`、`src/cases/llm/mod.rs`、`src/util/server.rs`、`src/cases/{chat,run}.rs`、`src/cases/llm/canary.rs`。
  **实施步骤**：
  1. 新建 `src/mock/mod.rs` 与 `src/mock/llm/`，迁移 `mod` / `http` / `handler` / `record` / `reply` / `scenario` 六个文件。
  2. `MockServer` 的公开面（`start` / `base_url` / `url`）保持不变；引用改为 `crate::mock::llm::MockServer`。
  3. `cases/llm/mod.rs` 去掉 `pub(crate) mod mock`；`cases/mod.rs` 不再向外部暴露 Mock。
  4. 确认 `runner` / `util` / `mock` 不出现 `crate::cases`（R4）。
  **验收标准**：
  - [ ] Mock 的 4 个端点、7 种场景、节奏参数、观测字段零变化。
  - [ ] `cargo check -p bin-tests` 通过。
  - [ ] `llm.mock` / `chat.*` / `run.*` 通过。
  **测试要求**：`make test ARGS="llm chat run"`。**需授权**。

- [ ] **P2.1-010 `llm.mock` 瘦身与观测去重**

  **目标**：`llm.mock` 只写步骤与断言；Mock 的「请求构造 + SSE 读取」与「观测读取」各自只有一处实现。
  **前置依赖**：P2.1-009。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/llm/mock.rs`（由 `mock/case.rs` 迁移）、`src/mock/llm/{probe,observe}.rs`、`src/util/flow.rs`、`src/cases/chat.rs`。
  **实施步骤**：
  1. chat 请求体构造、非流式 / 流式请求、SSE 读取与汇总、期望回复计算 → `mock/llm/probe.rs`。
  2. `/mock/stats`、`/mock/last-request` 的读取与轮询等待 → `mock/llm/observe.rs`；`flow::get_json` / `flow::wait_aborted` 与 `cases/chat.rs::last_request_messages` 改为调用它。
  3. `cases/llm/mock.rs` 只保留用例步骤与断言；纠正模块文档中被引用的用例名（实际断言在 `run.execute` / `run.failed_auth` / `run.failed_upstream` / `run.failed_timeout`）。
  **验收标准**：
  - [ ] `llm.mock` 的断言条目不减少，覆盖的场景与之前一致。
  - [ ] stats / last-request 的读取与等待只有一处实现。
  - [ ] `cases/` 内不再直接实现 HTTP/SSE 细节。
  **测试要求**：`make test ARGS="llm chat run"`。**需授权**。

- [ ] **P2.1-011 通用支持归位与 import 收敛**

  **目标**：通用支持平铺到 `src/util/`，用例只引 `crate::{util, mock}`。
  **前置依赖**：P2.1-009。
  **涉及文件 / Crate**：`crates/bin-tests/src/{context,server,flow,support,paths,prepare,frontend}.rs` → `src/util/`；`src/main.rs`；`src/cases/**`。
  **实施步骤**：
  1. 新建 `src/util/mod.rs`，迁移七个模块；`flow` 移出 Mock 观测后迁入（是否再拆 `client` / `events` 见 D-P2.1-003）。
  2. 全部用例文件的 `crate::context` / `crate::server` / `crate::flow` / `crate::support` / `crate::frontend` 引用改为 `crate::util::*`。
  3. 回收可见性：`cases/mod.rs` 去掉不再必要的 `pub(crate)`。
  4. 复查 R1：`cases/` 下不存在可被两个以上用例复用的动作实现。
  **验收标准**：
  - [ ] 目录结构符合 README §四，`cases/` 只剩用例。
  - [ ] `cargo check -p bin-tests` 通过，无未使用 import。
  - [ ] 无用例名变化、无新增依赖。
  **测试要求**：`make test ARGS="llm chat run session tui web"`。**需授权**。

---

## Batch D：回归与验收（P2.1-012 ~ P2.1-013）

> 依赖：Batch A ~ C 全部完成。**全量回归前必须完成 Batch C 的 import 收敛**，否则红/绿无法归因。

- [ ] **P2.1-012 分层回归**

  **目标**：按领域确认迁移与修复未改变行为。
  **前置依赖**：Batch B、Batch C。
  **涉及文件 / Crate**：无代码改动。
  **实施步骤**：按 `llm` / `chat` / `run` / `session` / `persist` / `rotation` / `tui` / `web` 分批跑；每批记录失败与耗时。
  **验收标准**：
  - [ ] 八个领域分批全绿，或失败项有明确归因与处置记录。
  **测试要求**：`make test ARGS="<领域>"` 各一次。**需授权**。

- [ ] **P2.1-013 默认并发全量回归（K-1 结案）**

  **目标**：在不限流、默认并发的前提下证明 K-1 已解决。
  **前置依赖**：P2.1-012。
  **涉及文件 / Crate**：无代码改动。
  **实施步骤**：
  1. 不传 `--concurrency` 跑 `make test ARGS="--all"` **连续 3 次**，逐次记录失败数与耗时分布。
  2. 记录同一轮的关键指标快照（Run 排队、worker 阻塞、清理链耗时、通道满次数）。
  3. 与 P2.1-001 基线做前后对照。
  **验收标准**：
  - [ ] 3 次全绿，失败数 0，且失败集合不再随并发变化。
  - [ ] `default_concurrency()` 与 `runner` 的分片调度与修复前一致（无新增限流）。
  - [ ] 指标能解释「为什么以前会超时」。
  **测试要求**：`make test ARGS="--all"` × 3（默认并发）。**需授权**。

---

## Batch E：并发配置增强与验收（P2.1-014 ~ P2.1-015）

> 依赖：**P2.1-013 通过**。这是硬前置：必须先在 `max_concurrent_runs = 4` 下把并发异常解决掉，才允许改默认值与语法，否则无法区分「修复生效」与「默认值变相放宽」。
>
> 本批次只改**服务端运行参数**的配置面，不动测试侧并发。
>
> 执行顺序：015（配置增强）→ 014（验收与报告，结论覆盖 015 的变更）。

- [ ] **P2.1-015 `max_concurrent_runs` 支持核数倍数（`NC`）形式**

  **目标**：`[agent] max_concurrent_runs` 支持 `"2C"`（2 × CPU 核数）这类写法，默认值为 `3C`，仍支持配置文件覆盖为绝对值或倍数。
  **前置依赖**：P2.1-013（硬前置）、P2.1-008（先有「许可是否瓶颈」的结论与数据）。
  **涉及文件 / Crate**：`crates/types-core/src/config/agent.rs`、`crates/lib-bin-cli/src/config.rs`、`crates/agent-runtime/src/port.rs`、`crates/manager-agent/src/{assemble,holder}.rs`、`crates/bin-tests/src/util/server.rs`（配置模板不变，仅核对）、`crates/bin-tests/src/cases/llm/config.rs`。
  **实施步骤**：
  1. **定稿值类型**（任务内决策，写进 `decisions.md`）：新增 `Concurrency`，取值为 `Absolute(usize)` 或 `Cores(usize)`；`Default = Cores(3)`；提供纯函数 `resolve(cores: usize) -> usize`；`Deserialize` 同时接受整数（`4`）与字符串（`"2C"`）。
  2. **解析与校验**：`lib-bin-cli::config` 的 `validate_agent` 改为按值类型校验；`0` / `0C` / `""` / 非法后缀（`2X`） / 负数 / 溢出（超过 `Semaphore` 可用上限）都要给出点名字段的错误文案；整数写法语义不变（向后兼容）。
  3. **装配**：`agent-runtime/src/port.rs` 的 `Semaphore::new(...)` 改为 `resolve(available_parallelism())`；核数只在装配层取一次，运行期不变。
  4. **默认值**：`[agent]` 整段缺省或只缺该字段时取 `3C`；`DEFAULT_MAX_CONCURRENT_RUNS` 的旧常量语义按 `Cores(3)` 重写，避免两处默认值不一致。
  5. **用例**：扩展 `llm.config`，覆盖 `2C` 解析、整数兼容、非法值文案、默认值；`resolve` 用注入核数的纯函数断言（不依赖真实机器核数）；`run.concurrency` 的显式 `max_concurrent_runs = 1` 必须继续通过。
  6. **回归**：分别验证「显式 4」与「默认 3C」两种配置下默认并发 `--all` 全绿；记录两者的并发度与耗时。
  **技术要求**：
  - 只扩展配置语法与默认值，不改 `RunEvent` 契约、不改接口、不改同会话串行与断连顺序语义。
  - `resolve` 必须是纯函数且可注入核数，便于用例断言。
  - 不得在测试侧做任何限流或并发折算；`bin-tests` 的共享服务端仍走「不写 `[agent]` → 默认值」的路径。
  - 上限保护：解析结果必须落在 `Semaphore` 可接受范围内，溢出按配置错误处理而非 panic。
  **验收标准**：
  - [ ] `"2C"` 在注入 8 核时解析为 16；`"3C"` 为 24；整数 `4` 仍为 4。
  - [ ] `[agent]` 缺省时默认取 `3C`（注入 8 核时为 24）。
  - [ ] 非法值（`0`、`0C`、`2X`、负数、溢出）各有明确错误文案且点名字段。
  - [ ] 显式 `max_concurrent_runs = 4` 下 `--all` 全绿（与 P2.1-013 一致），默认 `3C` 下同样全绿。
  - [ ] `git diff` 中没有测试侧限流改动，无用例名变化。
  **测试要求**：`make test ARGS="llm.config"`、`make test ARGS="run"`、`make test ARGS="--all"`（显式 4 与默认 3C 各一轮）。**需授权**。
  **完成产物**：核数倍数的并发配置；`decisions.md` 增加 `Concurrency` 值类型与解析位置的决策记录；文档同步（`docs/develop.md` 的配置面、`.steps/README.md` 的 P2.1 状态）在 P2.1-014 授权后一并处理。

---

- [ ] **P2.1-014 验收与报告**

  **目标**：按 README §七 判据逐条自查，产出阶段报告并更新阶段状态。
  **前置依赖**：P2.1-013、P2.1-015。
  **涉及文件 / Crate**：`docs/reports/p2.1-*.md`（新建）、`.steps/p2.1/`、`.steps/README.md`（需授权）。
  **实施步骤**：
  1. 报告结构沿用 P1.1 / P2：逐条判据 + 证据、假设验证结论（含被否决项）、偏差与遗留、下游（P3）输入清单。
  2. 记录每处修复的「修复前 / 修复后」数据；被否决的假设（如 ArcSwap）写明判断依据。
  3. 更新阶段状态与 P3 的开工前置说明（文档同步需授权）。
  **验收标准**：
  - [ ] README §七 的 G-1 / G-2 / G-3 判据全部可核。
  - [ ] 每个根因假设都有「成立 / 不成立 + 证据」的结论，无沉默项。
  - [ ] 偏差逐条写「处置 + 归属」。
  **测试要求**：无（证据来自 P2.1-012 / 013 / 015）。
