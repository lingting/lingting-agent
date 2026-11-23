# P2.1 任务清单

**状态：Batch A 已完成（P2.1-001 ~ 003，2026-11-21）；Batch B 已完成（P2.1-004 ~ 008，2026-11-21，逐任务「修复前 / 修复后」对照见 `docs/reports/p2.1-004 ~ 008`）；Batch C 已完成（P2.1-009 ~ 011，2026-11-22，`cases/` 只剩用例、Mock 与通用支持各自归位，`cargo check` / `clippy` 与两轮用例全绿）；Batch D 已完成（P2.1-012 ~ 013，2026-11-22，八领域分批全绿 + 默认并发三轮全绿，首轮的 `runtime.disconnect` 红灯已归因为 P2.1-006 遗留的用例时序缺口并修复，见 `docs/reports/p2.1-012-domain-regression.md`、`docs/reports/p2.1-013-default-concurrency.md`）；Batch E 已完成（P2.1-014 ~ 015，2026-11-22：`NC` 语法与 `3C` 默认值落地，显式 4 与默认 3C 两轮全量全绿，见 `docs/reports/p2.1-014-acceptance.md`）；Batch B 追加 P2.1-008.1 / 008.2（锁面与数据库读路径复核，审查型）：008.1 / 008.2 均已完成（锁面与数据库读路径逐项「不改」，见 `docs/reports/p2.1-008.1-lock-audit.md`、`docs/reports/p2.1-008.2-db-read-cache.md`）。** 阶段目标、边界、根因分析与判据见本目录 [README.md](README.md)；`acceptance.md` 不单独建（判据在 README §七），`decisions.md` 已随 Batch A 展开。关键结论：**K-1 未复现**，G-1 口径改为「结构修复 + 指标对照」（[decisions.md](decisions.md) D-P2.1-007）；Batch B 修掉四处结构缺陷（测试侧同步阻塞、全局断连链、锁内收尾、锁表只增），H-3 复核结论为「许可与 journal 都不是瓶颈」。

批次之间强序，批次内可并行。**每个任务的「测试要求」只允许在 `crates/bin-tests` 落地用例；其他 Crate 不得写 `#[cfg(test)]`**（仓库 `AGENTS.md` 第 28 条）。**所有需要执行用例的步骤都必须先取得明确授权**；未授权时只推进代码审查、静态检查与文档部分，相关验收项保持未勾选。

本阶段的核心约束：**不做测试侧限流**。`cli::default_concurrency()` 不得改为硬上限，`runner` 不得新增「共享分片许可」；降并发只用于定位对照。K-1 的结案口径是「修根因 + 默认并发下连续全绿 + 指标可解释」。

---

## Batch A：量测与可观测性（P2.1-001 ~ P2.1-003）

> 依赖：无。**本批次先不改修复性代码**：001 取基线，002 / 003 只加观测。003 完成后才能对 H-1 ~ H-4 定性。

- [x] **P2.1-001 复现基线与失败画像**（已完成：**K-1 未复现**，见 `docs/reports/p2.1-001-baseline.md`）

  **目标**：把 K-1 从「印象」变成可复算的数据：默认并发下的失败集合、耗时分布、失败是否压在用例内部上限，以及与低并发 / 单跑的对照。
  **前置依赖**：无。
  **涉及文件 / Crate**：不改代码；结论写进本阶段报告草稿（`docs/reports/` 下的 P2.1 报告，Batch D 定稿）。
  **实施步骤**：
  1. 默认并发（不传 `--concurrency`）跑 `make test ARGS="--all"` **3 次**，逐次记录：并发度、失败用例、失败耗时、总计耗时。
  2. 低并发对照：`make test ARGS="--all --concurrency 4"` **1 次**，记录同一组字段。
  3. 对每次失败用例单独跑一次，记录单独耗时与是否通过。
  4. 核对失败耗时是否恰好等于用例内部等待上限（`cases/tui.rs` 的 `STREAM_TIMEOUT`、`runner` 的用例总超时）。
  **验收标准**：
  - [x] 三次默认并发的失败集合、耗时、并发度成表；**实测三次全绿、失败集合恒为空**，「失败集合逐次变化」无法复现，改为记录「未复现」+ 五档加压对照表。
  - [x] 低并发与单跑的结果在同一张表里可对照（并发 4 与并发 18 墙钟相同：33.9s vs 34.0s）。
  - [x] 明确写出「失败耗时是否压在内部上限」的结论：无失败样本，只能给出「无压线」+「加压下从未出现精确压线」的形态对比。
  **测试要求**：`make test ARGS="--all"` × 3、`make test ARGS="--all --concurrency 4"`、失败用例单跑（无失败用例，改为五档加压实验）。**已授权并执行**。

- [x] **P2.1-002 测试进程阻塞点审计**（已完成，见 `docs/reports/p2.1-002-blocking-audit.md`）

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
  - [x] 阻塞点清单完整（3 处：`wait_until` 的同步轮询、`Harness` 收尾的 join、`rotation.rs` 的 10ms sleep），且每个点标注所属用例与最长阻塞时长。
  - [x] 能给出默认并发下单次运行的累计阻塞 worker 时长（27 次 / 4.8s，摊到 18 个 worker 人均 0.26s）。
  - [x] 用例行为与耗时特征与 P2.1-001 基线一致（加观测后预热墙钟 34.1s = 基线 34.0s / 34.1s）。
  **测试要求**：`make test ARGS="tui --all"` 与一次 `--all` 取累计值。**需授权**。

- [x] **P2.1-003 服务端并发可观测性**（已完成，见 `docs/reports/p2.1-003-observability.md`）

  **目标**：让「慢在哪一段」可观测：Run 排队 / 执行、断连清理链、`stop_session` 各阶段、事件落盘与转发、数据库取连接、下行通道满。
  **前置依赖**：无（可与 P2.1-002 并行）。
  **涉及文件 / Crate**：`crates/agent-runtime/src/{limit,loop_,port}.rs`、`crates/manager-agent/src/runtime/{handler,session,registry}.rs`、`crates/manager-agent/src/runner/queue.rs`、`crates/lib-db/src/pool.rs`、`crates/bin-tests/src/util/`（读取入口）。
  **实施步骤**：
  1. 在 `limit::acquire` 前后记录排队时长与当前持有数；排队超过阈值（建议 100ms）打 `debug`，超过 1s 打 `warn`，都带 `run_id`。
  2. 在 `RuntimeSession::stop` 的四个阶段（DB 取消 / `executor.stop` / 写者收尾 / 总时长）分别计时，超阈值（建议 500ms）打 `warn`，带 `session_id`。
  3. 在 `handler::process` 的进入 / 退出计时，记录本次处理的客户端数量与排队情况。
  4. 在 `journal::append` 记录单帧写入耗时（阈值触发），在 `write_event` 记录 `downlink.forward` 的返回（是否因通道满被丢弃）。
  5. 在 `Database::begin` / 取连接处记录等待时长（阈值触发）。
  6. 进程内指标：同时 `RUNNING` 数、排队数、通道满次数、`session_locks` 条目数、清理链并行度；只聚合，不新增 HTTP 接口、不新增配置项。**全部采集代码放在 `metrics` 特性下**（D-P2.1-008），运行结束后由测试报告的「观测数据」一节输出。
  **技术要求**：阈值先比较后构造日志参数，避免热路径格式化开销；日志级别默认 `debug`；指标只在 `metrics` 特性下采集，正式构建为空实现（D-P2.1-008）；`lib-db` 为此允许新增可选依赖 `tracing`（D-P2.1-009）；指标读取只用于测试与诊断，不加公开 API 的语义。
  **验收标准**：
  - [x] 默认并发下可读出：同时 `RUNNING` 数（`probe.health`）、Run 排队时长（O-1）、清理链耗时（O-3）、通道满次数（O-5）、`session_locks` 条目数（O-6）、DB 取连接等待（O-7）。**已知缺口**：共享服务端固定 `--log-level warn`，100ms 级排队分布只有「< 1s」的上界结论，具体分布需一次 `debug` 级诊断跑。
  - [x] 新增日志在无异常时不产生每帧一条的 `info` 噪声（实测整轮 500ms / 50ms 阈值日志零触发，唯一一条 `warn` 来自 `max_concurrent_runs = 1` 的人为排队）。
  - [x] 25 条接口、`RunEvent` 契约零变更（`make gen` 产出「内容无变化，未写盘」）。
  **测试要求**：一次 `--all` 采集指标快照（快照随测试报告输出）。**已授权并执行**。

---

## Batch B：结构性并发缺陷修复（P2.1-004 ~ P2.1-008.2）

> 依赖：P2.1-001 ~ P2.1-003。**一次只改一处，改完单独跑一轮全量**，禁止多处同改；每处修复在报告里保留「修复前 / 修复后」数据。

- [x] **P2.1-004 消除测试进程同步阻塞（H-1）**（已完成，见 [`docs/reports/p2.1-004-async-wait.md`](../../docs/reports/p2.1-004-async-wait.md)）

  **目标**：`async` 用例不再用同步 sleep 等结果，释放被占用的 worker。
  **前置依赖**：P2.1-002。
  **涉及文件 / Crate**：`crates/lib-bin-tui/src/testing.rs`、`crates/bin-tests/src/cases/tui.rs`、`crates/bin-tests/src/cases/tui/*.rs`。
  **实施步骤**：
  1. 在 `testing` feature 内新增异步等待入口 `wait_until_async`：内部 `pump()` + `tokio::time::sleep(POLL_INTERVAL)`，超时返回 `false`。
  2. 把 `cases/tui/*` 中 `async fn` 内的 `wait_until` 调用改为 `wait_until_async`；保留同步 `wait_until` 给非 `async` 场景，并在文档注释写明「禁止在 `async` 上下文调用」。
  3. 复查其它同步阻塞：`cases/tui.rs` 的公共等待、`lib-bin-tui` 的 `pump` 内部是否有阻塞 IO。
  4. 只改这一处后跑一轮默认并发 `--all`，与 P2.1-001 基线对比。
  **验收标准**：
  - [x] `cases/` 内不再有 `async` 上下文中的同步 sleep / 阻塞等待。实测「同步轮询等待 0 次 / 0ms、同步收尾 0 次 / 0ms」；`rotation.*` 的 `std::thread::sleep` 一并换成异步睡眠。
  - [x] 单独修复后的指标与基线对照成表：`wait_millis` / 收尾耗时显著下降（数字来自测试报告的「观测数据」一节），不是靠失败数下降证明。基线 27 次 / 4710ms + 10 次 / 42ms → 同步两行 0；同一批等待改记在异步行（27 次 / 4865~4993ms）。
  - [x] 用例断言与用例名零变更（只改等待入口与收尾调用，另新增 `Harness::close`）。
  **测试要求**：`make test ARGS="--all"`（默认并发）× 2。**已授权并执行**（两次 73/73，墙钟 35s / 33s）。

- [x] **P2.1-005 断连链按客户端并行（H-2）**（已完成，见 [`docs/reports/p2.1-005-chain-parallel.md`](../../docs/reports/p2.1-005-chain-parallel.md)）

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
  - [x] 多客户端同时断连时不互相排队（用 P2.1-003 的指标或日志证明）。单跑 `runtime.disconnect`（6 个客户端）读到并发水位 3；修复前该路径结构上不可能超过 1（全局锁把整条链串起来，`>1` 只代表等锁）。
  - [x] 同一客户端不会并发执行两次清理（每个 `client_id` 一把锁；`runtime.handler` 的并发入口断言仍通过）。
  - [x] `runtime.disconnect` / `runtime.handler` / `tui.reconnect` / `runtime.shutdown` 全通过。
  **测试要求**：`make test ARGS="runtime tui"`、`make test ARGS="--all"`。**已授权并执行**（32/32、73/73）。

- [x] **P2.1-006 收尾等待移出会话锁（H-2）**（已完成，见 [`docs/reports/p2.1-006-finalize-out-of-lock.md`](../../docs/reports/p2.1-006-finalize-out-of-lock.md)）

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
  - [x] 会话锁持有时间与收尾时长解耦（日志可证）。新增「会话锁持有」指标：22 次、最长 1ms、累计 14ms；同一轮单次会话停止最长 16~22ms，清理链最长 0ms。
  - [x] 占用 / 释放不被断连收尾拖住（`release_occupancy` / `clean` 锁内只摘会话并起后台任务）。
  - [x] 事件文件顺序与终态事件仍正确（`runtime.journal` / `run.journal_consistency` 通过；`runtime.journal` 的两处断言改为先等终态事件出现，内容与 seq 断言不变）。
  **测试要求**：`make test ARGS="runtime run"`、`make test ARGS="--all"`。**已授权并执行**（28/28、73/73 × 2）。

- [x] **P2.1-007 会话锁表治理（H-4）**（已完成，见 [`docs/reports/p2.1-007-lock-reclaim.md`](../../docs/reports/p2.1-007-lock-reclaim.md)）

  **目标**：`session_locks` 不再随历史会话数单调增长，取锁不再与所有读操作争写锁。
  **前置依赖**：P2.1-006（避免与收尾路径同时改）。
  **涉及文件 / Crate**：`crates/manager-agent/src/runtime/registry.rs`。
  **实施步骤**：
  1. 取锁先走读锁快路径（命中直接 clone），未命中再降级到写锁插入。
  2. 增加回收：会话停止 / 占用释放后，在没有其它持有者时移除条目（用 `Arc::strong_count` 判定或显式引用计数）；回收失败只记 `debug`，不得阻塞路径。
  3. 用 P2.1-003 的条目数指标验证长跑后的增长曲线。
  **技术要求**：不得引入新依赖；回收必须与「取锁快照」并发安全。
  **验收标准**：
  - [x] 长跑后 `session_locks` 条目数不随历史会话数单调增长。`runtime tui` 前 6 条 / 上限 6 → 后 0 条 / 上限 2；`--all` 0 条 / 上限 2。
  - [x] 同一会话的取锁仍唯一（读锁快路径与写锁插入都返回表里同一个 `Arc`）。
  - [x] `runtime.occupancy` / `runtime.handler` 通过（含在全量里）。
  **测试要求**：`make test ARGS="runtime"`、长跑一次 `--all` 后读条目数。**已授权并执行**（`runtime tui` 32/32、`--all` 73/73）。

- [x] **P2.1-008 许可与写路径复核（H-3）**（已完成，结论为「两条都不改」，见 [`docs/reports/p2.1-008-limit-and-journal.md`](../../docs/reports/p2.1-008-limit-and-journal.md)）

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
  - [x] 给出「许可是否瓶颈」「journal 是否瓶颈」两个明确结论与数据；本任务不改并发配置的默认值。许可：默认 4 个许可下 1s 级排队零条，唯一 3.1s 排队来自 `max_concurrent_runs = 1` 的用例；journal：100 帧单帧最长 0ms，50ms 阈值零触发。
  - [x] 如调整了 journal 写入方式，附修复前后对比，且 `git diff` 里没有测试侧限流改动（本任务无代码改动）。
  - [x] 事件顺序语义不变（先落盘 flush 再转发）。
  **测试要求**：`make test ARGS="--all"` 前后各一次。**已授权并执行**（复用 P2.1-007 的全量轮与 `run.concurrency` 单跑）。

- [x] **P2.1-008.1 同步锁与共享状态复核（锁面：ArcSwap / 无锁可替换性）**（已完成，结论为「逐项不改」，见 [`docs/reports/p2.1-008.1-lock-audit.md`](../../docs/reports/p2.1-008.1-lock-audit.md)）

  **目标**：逐处检阅进程内全部同步锁与共享状态，判定「能否换成 `ArcSwap` 或其他无锁方式」。判据不是「是否用了 ArcSwap」，而是「锁是否仍在热路径上制造排队」；**「不改」也是合格结论**，但必须附指标或结构理由。
  **前置依赖**：P2.1-008（先把许可与 journal 的结论落定，避免与写路径同改）。
  **涉及文件 / Crate**：`crates/manager-agent/src/runtime/{registry,handler,session}.rs`、`crates/manager-agent/src/runner/cancel.rs`、`crates/agent-runtime/src/{limit,port}.rs`、`crates/lib-bin-tui/src/worker.rs`、`crates/lib-core/src/directorys.rs`（现状参照）、`crates/{manager-agent,lib-bin-tui}/Cargo.toml`（仅当决定引入 `arc-swap` 时）。
  **实施步骤**：
  1. 建立锁面清单（文件 + 字段 + 锁类型 + 临界区是否跨 `await` + 每单位动作次数）。至少覆盖：
     - `registry.rs::RuntimeState::inner: Arc<RwLock<Registry>>`：整表一把 `RwLock`；`Downlink::forward` **每帧一条事件取一次写锁**（`allocate_seq` 自增与 `channels` 查找都在锁内），`mark_stream_closed` / `heartbeat` / `occupy` 也走写锁。
     - `registry.rs::Registry::session_locks: HashMap<String, Arc<AsyncMutex<()>>>`（读锁快路径 + 写锁插入 + P2.1-007 的回收）。
     - `registry.rs::Finalizer::pending: Mutex<HashMap<..>>`（短临界区，不跨 `await`）。
     - `handler.rs::CHAINS: OnceLock<RwLock<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>`（P2.1-005 的按客户端串行单位）。
     - `runtime/session.rs` 写者的 `journal: Arc<std::sync::Mutex<Journal>>`（`append` 在 `spawn_blocking` 内持锁）。
     - `runner/cancel.rs::RunCancels::inner: Mutex<HashMap<String, Entry>>`（锁内只做内存读写）。
     - `lib-bin-tui/src/worker.rs`：`Arc<Mutex<LinkSnapshot>>`（读多写少）与 `AsyncMutex<LinkState>`（跨 `await` 的连接状态机）。
     - `OnceLock` 类（`holder`、`probe`、`lifecycle`、`registry` 的 `events_root` / `driver`）：只写一次，直接排除并写明理由。
     - `agent-runtime` 的 `Semaphore` 许可：不是锁（排队行为已由 P2.1-008 定性），不在本任务改默认值。
  2. 按「锁的用途」分类后再判可替换性，避免把语义问题当性能问题：
     - **互斥原语**（`session_locks`、`CHAINS`、`LinkState`）：提供「同一会话 / 同一客户端串行」语义，**不得** ArcSwap 化；只允许把其所在索引表换成 `ArcSwap`，且收益必须由指标证明。
     - **读多写少快照**（`LinkSnapshot`、客户端状态与 `channels`）：ArcSwap 明确可行，读侧只 `load` + `Arc::clone`。
     - **整表状态**（`Registry`）：ArcSwap 结构上可行，但写侧要整表 clone；**必须先拆掉「每帧取写锁」**（序号改 `AtomicU64`、通道表独立快照），否则只是把写锁换成 clone 成本。
     - **阻塞线程内的短锁**（`journal`）：优先评估「单写者 + 频道」而非换锁类型。
  3. 补测量口径（只在 P2.1-003 的 `metrics` 特性下采集）：每帧取写锁次数与等待时长、锁表取锁次数与等待、临界区最长时长。没有数字不做改造。
  4. 依测量结果逐项定论；确需改的**一次只改一处**，改完单独跑一轮全量并留前后对照；未改项写明理由与数据。
  **技术要求**：
  - 语义零变化：同会话串行、断连顺序（`client_closed` → 反注册 → 停运行时会话 → 撤回占用）、先落盘 flush 再转发、锁内不跨 `await`。
  - 依赖：`arc-swap` 已在 workspace（`lib-core` 已用），向其他 Crate 引入仍需在 `decisions.md` 登记范围与理由。
  - 测试侧锁（`bin-tests/src/server.rs` 的共享服务端 `LOCK`、`support.rs` 的 `app_home` 锁、`cases/llm/mock/record.rs` 的 `Mutex`）是测试进程**故意串行**的夹具，不在本任务范围，只登记不改。
  - 锁中毒处理策略不变（保留现有 `unwrap_or_else(|e| e.into_inner())` 语义）。
  **验收标准**：
  - [x] 锁面清单完整，每项标出「锁类型 / 临界区是否跨 `await` / 每单位动作次数」。
  - [x] 每项都有明确结论（改 / 不改），「不改」附指标或结构理由，无沉默项。
  - [x] 若引入 `ArcSwap`（条件未触发：本轮无锁实现改动，见 decisions.md D-P2.1-011）：对应读路径不再取互斥锁，且改造前后有同一轮次的指标对照（不靠失败数）。
  - [x] `session_locks` / `CHAINS` 的「同一会话 / 同一客户端串行」语义仍成立（`runtime.occupancy` / `runtime.handler` / `runtime.disconnect` / `tui.reconnect` 全通过）。
  - [x] `git diff` 无测试侧限流改动、无用例名变化、无接口与 `RunEvent` 契约变化。
  **测试要求**：`make test ARGS="runtime tui"`、`make test ARGS="--all"`。**已授权并执行**（32/32 墙钟 18s、73/73 墙钟 33s）。
  **完成产物**：`docs/reports/p2.1-008.1-lock-audit.md`（锁面清单 + 逐项结论 + 首轮基线数据）；`decisions.md` 增加锁面口径（D-P2.1-010）与 `arc-swap` 引入范围（D-P2.1-011）。两轮实测的读写等待与单帧转发耗时全部 0ms、100ms 阈值零触发，因此**未做任何锁实现改动**。

- [x] **P2.1-008.2 数据库读路径复核（表级内存快照 + 失效事件）**（已完成，结论为「不改」，机制口径定稿但不落地，见 [`docs/reports/p2.1-008.2-db-read-cache.md`](../../docs/reports/p2.1-008.2-db-read-cache.md)）

  **目标**：逐处检阅执行期的 DB 读位置，判定能否改为「进程内持有一份表快照（`ArcSwap`），读时 `Arc::clone`；数据库写入后发布变更通知，由异步订阅方重读 DB 并替换快照」，用内存快照消除重复查询。目标是消除**读放大**，不是把数据库整体搬到内存，也不是让缓存参与业务判定。
  **前置依赖**：P2.1-008、P2.1-008.1（先定锁面口径，避免缓存自身引入新的锁热点）。
  **涉及文件 / Crate**：`crates/service-core/src/repository/{session,message,run}.rs`、`crates/service-core/src/session/{service.rs,mod.rs}`、`crates/service-core/Cargo.toml`、`crates/manager-agent/src/{session.rs,assemble.rs,holder.rs}`、`crates/manager-agent/src/runtime/{occupancy.rs,handler.rs}`、`crates/types-core/src/`（变更通知类型）、`crates/lib-web/src/{probe,session}/api.rs`、`crates/lib-db/src/pool.rs`（取连接口径）。
  **实施步骤**：
  1. 建立执行期读清单（调用点 + 每次请求的查询数 + 频率 + 结果是否被当成权威判定）。至少覆盖：
     - 探活 `api/probe/health` → `running_run_count()` → `SELECT COUNT(*) FROM runs WHERE status = 'RUNNING'`：每探活一次一条聚合查询（TUI 连接与重连各触发一次）。
     - `api/session/list` → `sessions()`：全表扫描 + 每行 `(SELECT COUNT(*) FROM messages ...)` 相关子查询，成本随会话数与消息数增长。
     - `api/message/list` → `require_session()` + `messages()`：同一请求两条查询。
     - `api/message/create` → `require_session()` + 事务写。
     - Run 执行期：`session_summary()` / `history_before_run()` / `user_prompt()` / `start_run()` / `complete_run()` / `fail_run()` / `run_error()`。
     - 占用建立 `has_session()`、会话停止 `cancel_session_runs()`（写）。
     - 已确认**不是**轮询：执行队列由 `notify_run` 唤醒后查一次 `next_queued_run()`；本次改造不得把它改成定时轮询。
  2. 分类并给出缓存资格判定：
     - **只读展示型**（`session/list`、`message/list`）：可缓存，允许有界陈旧。
     - **权威判定型**（`RunStatus` / `RunError` / 条件更新前的 `require_run`）：**不缓存**，否则会破坏 D-P2-029 / D-P2-030 的条件更新与取消语义；这类只允许把「仅用于展示的派生值」（如探活里的 `running_run_count`）改为内存计数。
     - **一次写一次读**（`user_prompt` / `history_before_run`）：不缓存（一次执行一次读）。
  3. 对取得缓存资格的表，先把设计写成决策，再落地：
     - 快照粒度与类型（整表 `ArcSwap<Vec<SessionSummary>>` / 按会话 `HashMap<session_id, Arc<Vec<Message>>>`）；读侧只 `load_full()` 后 `Arc::clone`，不在锁内做业务。
     - 失效通知：事务提交后 `bus.notify::<XxxChanged>()`；按 `framework-bus` 约定事件**纯通知、不带数据、边沿触发 + 合并**，订阅方被唤醒后自己重读 DB 并替换快照。
     - 归属：事件类型放 `types-core`（只放类型，不倒置依赖方向）；订阅与刷新任务放装配层 `manager-agent`；`repository` 只允许被 `service-core` 内部 Service 调用（`AGENTS.md` 第 7 条），因此快照与 `refresh_*` 放 `service-core`，`manager-agent` 只负责订阅并调用 `refresh_*`。
     - 启动顺序：先 `watch` 再全量加载，避免「先加载后订阅」之间丢通知；刷新失败保留旧快照并记 `warn`，等下一次通知重试。
     - 未命中与惊群：首次访问或刷新中回源 DB，并做「单飞」（同一表同一时刻最多一个回源任务）。
     - 有界陈旧：写明可接受上限（建议 ≤ 1 个刷新周期或 1 次通知往返），超出即回源。
  4. 分两步落地，每步单独跑一轮全量对照：先「探活计数改内存计数」，再「会话 / 消息只读快照」。写路径（`INSERT`、条件 `UPDATE`、事务）一律保持原样。
  5. 「设置」类读：当前**没有**配置 / 设置表，`config.toml` 由 `lib-bin-cli` 装载后经 `ProviderService` 常驻进程内，不存在重复 DB 查询。本任务只把该机制写成可直接套用的口径，不为尚不存在的表提前实现。
  **技术要求**：
  - 事件通路不变：先落盘 flush，再转发；缓存只影响「读」，不得影响事件顺序与终态写入。
  - 不新增 HTTP 接口、不新增配置项、不改 25 条接口与 `RunEvent` 契约、不改 `RunStatus` 状态机、不改表结构与迁移。
  - 缓存可观测项（命中 / 回源次数、刷新耗时、最近一次刷新成功时刻）只在 `metrics` 特性下采集（D-P2.1-008）。
  - 新增依赖：`arc-swap` 已在 workspace；向 `service-core` 增加该依赖需在 `decisions.md` 登记（含与 `AGENTS.md` 第 7 条的关系说明）。不得把缓存放进 `lib-db` 或 `types-core`。
  **验收标准**：
  - [x] 读清单完整，每条都有「可缓存 / 不可缓存」结论与理由，无沉默项（10 类访问，见报告 §一 / §三）。
  - [x] `probe.health` 的 `running_run_count` **不改**：单条 `COUNT(*)` 走 `idx_runs_status`，只在连接 / 重连各调用一次，`probe.health` 用例 1ms；无轮询调用方（报告 §二 / §四）。
  - [x] `session/list`（1 条带相关子查询）与 `message/list`（2 条）**不改**：前端无会话列表页、TUI 按需调用、整轮全量「取数据库连接」3 次；两条 SQL 读放大列入遗留候选（报告 §七）。
  - [x] 权威判定路径零改动：本任务无代码改动，取消 / 终态条件更新 / 同会话串行语义与用例口径不变。
  - [x] 失效正确性（条件未触发：未引入快照与通知，无陈旧窗口）。
  - [x] `git diff` 无测试侧限流、无接口 / 契约变更、无 SQL 与依赖改动、无缓存值参与业务判定。
  - [x] 以「不改」结案，附每请求语句数、调用方频率与耗时数据（报告 §一 ~ §四）。
  **测试要求**：`make test ARGS="probe session run runtime"`、`make test ARGS="--all"`。**已执行**：复用本轮默认并发全量（73/73，墙钟 33s，`probe` / `session` / `run` / `runtime` 全部覆盖在内）；本任务无代码改动，故未另跑分域轮。
  **完成产物**：`docs/reports/p2.1-008.2-db-read-cache.md`（读清单 + 分类判定 + 机制口径 + 未改项理由）；`decisions.md` 增加 D-P2.1-012（表级快照的形态与启用口径，当前不启用）。无代码改动，未引入 `arc-swap`。

---

## Batch C：测试基础设施整理（P2.1-009 ~ P2.1-011）**已完成（2026-11-22）**

> 依赖：无（可与 Batch A / B 并行）。**只做位移，不改行为**；本批次完成后才能跑 Batch D 的全量回归，保证归因唯一。

- [x] **P2.1-009 Mock 迁出 `cases`**（已完成）

  **目标**：`cases/` 只留用例，Mock 成为独立支持模块。
  **前置依赖**：无。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/llm/mock/*` → `crates/bin-tests/src/mock/llm/*`；`src/main.rs`、`src/cases/llm/mod.rs`、`src/util/server.rs`、`src/cases/{chat,run}.rs`、`src/cases/llm/canary.rs`。
  **实施步骤**：
  1. 新建 `src/mock/mod.rs` 与 `src/mock/llm/`，迁移 `mod` / `http` / `handler` / `record` / `reply` / `scenario` 六个文件。
  2. `MockServer` 的公开面（`start` / `base_url` / `url`）保持不变；引用改为 `crate::mock::llm::MockServer`。
  3. `cases/llm/mod.rs` 去掉 `pub(crate) mod mock`；`cases/mod.rs` 不再向外部暴露 Mock。
  4. 确认 `runner` / `util` / `mock` 不出现 `crate::cases`（R4）。
  **实施结果**：六件套与用例本体同时落位——用例文件改为 `cases/llm/mock.rs`（若在 009 保留 `cases/llm/mock/mod.rs` 会让模块树在 009 / 010 之间不成立）；`MODEL` 经 `crate::mock::llm` 再导出，`reply` 等子模块保持私有。R4 实测无 `crate::cases`（`runner` / `util` / `mock` 全部为 0 处）。
  **验收标准**：
  - [x] Mock 的 4 个端点、7 种场景、节奏参数、观测字段零变化。
  - [x] `cargo check -p bin-tests` 通过（`cargo clippy -p bin-tests` 无告警）。
  - [x] `llm.mock` / `chat.*` / `run.*` 通过。
  **测试要求**：`make test ARGS="llm chat run"`。**已授权并执行**（17/17 全绿）。

- [x] **P2.1-010 `llm.mock` 瘦身与观测去重**（已完成）

  **目标**：`llm.mock` 只写步骤与断言；Mock 的「请求构造 + SSE 读取」与「观测读取」各自只有一处实现。
  **前置依赖**：P2.1-009。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/llm/mock.rs`（由 `mock/case.rs` 迁移）、`src/mock/llm/{probe,observe}.rs`、`src/util/flow.rs`、`src/cases/chat.rs`。
  **实施步骤**：
  1. chat 请求体构造、非流式 / 流式请求、SSE 读取与汇总、期望回复计算 → `mock/llm/probe.rs`。
  2. `/mock/stats`、`/mock/last-request` 的读取与轮询等待 → `mock/llm/observe.rs`；`flow::get_json` / `flow::wait_aborted` 与 `cases/chat.rs::last_request_messages` 改为调用它。
  3. `cases/llm/mock.rs` 只保留用例步骤与断言；纠正模块文档中被引用的用例名（实际断言在 `run.execute` / `run.failed_auth` / `run.failed_upstream` / `run.failed_timeout`）。
  **实施结果**：`flow::get_json` / `flow::wait_aborted` 与 `cases/chat.rs::last_request_messages` 删除，改为 `mock::llm::observe`；轮询等待收敛为一条 `poll_aborted` 核心 + 两个入口（「至少一次中断」超时报错、「超过已知值」超时返值），保留原有超时与间隔口径，行为未变。断言条目逐条比对未减少（唯一挪动的是 `GET … 应返回 200`，随 `get_json` 进 `observe`）。
  **验收标准**：
  - [x] `llm.mock` 的断言条目不减少，覆盖的场景与之前一致。
  - [x] stats / last-request 的读取与等待只有一处实现。
  - [x] `cases/` 内不再直接实现 HTTP/SSE 细节（**本任务范围**：`llm.mock` 与 Mock 观测已全部走 `probe` / `observe`；其余领域用例自有的单点探测与 web / tui 领域内共用 helper 不在本任务清单，口径见 [decisions.md](decisions.md) D-P2.1-013）。
  **测试要求**：`make test ARGS="llm chat run"`。**已授权并执行**（17/17 全绿）。

- [x] **P2.1-011 通用支持归位与 import 收敛**（已完成）

  **目标**：通用支持平铺到 `src/util/`，用例只引 `crate::{util, mock}`。
  **前置依赖**：P2.1-009。
  **涉及文件 / Crate**：`crates/bin-tests/src/{context,server,flow,support,paths,prepare,frontend}.rs` → `src/util/`；`src/main.rs`；`src/cases/**`。
  **实施步骤**：
  1. 新建 `src/util/mod.rs`，迁移七个模块；`flow` 移出 Mock 观测后迁入（是否再拆 `client` / `events` 见 D-P2.1-003）。
  2. 全部用例文件的 `crate::context` / `crate::server` / `crate::flow` / `crate::support` / `crate::frontend` 引用改为 `crate::util::*`。
  3. 回收可见性：`cases/mod.rs` 去掉不再必要的 `pub(crate)`。
  4. 复查 R1：`cases/` 下不存在可被两个以上用例复用的动作实现。
  **实施结果**：七个模块按清单迁入，另按 D-P2.1-013 把 `observability.rs`（P2.1-003 新增、清单未列）一并迁入 `src/util/`；顶层只剩 `main` / `cli` / `runner` / `report` / `case`。全量 import 改为 `crate::{util, mock}`，`cases/mod.rs` 的 `chat` / `llm` / `run` 去掉 `pub(crate)`。
  **验收标准**：
  - [x] 目录结构符合 README §四，`cases/` 只剩用例。
  - [x] `cargo check -p bin-tests` 通过，无未使用 import（`cargo clippy -p bin-tests` 无告警）。
  - [x] 无用例名变化、无新增依赖（`Cargo.toml` 未改动）。
  **测试要求**：`make test ARGS="llm chat run session tui web"`。**已授权并执行**（44/44 全绿，含 `web.ui` / `web.e2e`）。

---

## Batch D：回归与验收（P2.1-012 ~ P2.1-013）

> 依赖：Batch A ~ C 全部完成。**全量回归前必须完成 Batch C 的 import 收敛**，否则红/绿无法归因。

- [x] **P2.1-012 分层回归**（已完成，见 `docs/reports/p2.1-012-domain-regression.md`）

  **目标**：按领域确认迁移与修复未改变行为。
  **前置依赖**：Batch B、Batch C。
  **涉及文件 / Crate**：无代码改动。
  **实施步骤**：按 `llm` / `chat` / `run` / `session` / `persist` / `rotation` / `tui` / `web` 分批跑；每批记录失败与耗时。
  **验收标准**：
  - [x] 八个领域分批全绿，或失败项有明确归因与处置记录（八个领域 54 条全绿，无失败项）。
  **测试要求**：`make test ARGS="<领域>"` 各一次。**已授权并执行**。
  **实施结果**：并发 18，逐批 `make test ARGS="<领域>"`：`llm` 4/4（5s）、`chat` 3/3（2s）、`run` 10/10（9s）、`session` 5/5（4s）、`persist` 5/5（3s）、`runtime.rotation` 5/5（3s）、`tui` 14/14（17s）、`web` 8/8（12s）。`runtime.rotation` 按 `runtime.rotation.` 点边界匹配五条轮转用例；本批未覆盖的 19 条（`runtime.*` 其余 13 条、`probe`、`client`、`sdk`）由 P2.1-013 的全量覆盖。`tui.reconnect` 15.1s 与 `web.ui` 6.8~9.9s 是两条长尾，均未贴近内部上限。


- [x] **P2.1-013 默认并发全量回归（G-1 回归判据）**（已完成，见 `docs/reports/p2.1-013-default-concurrency.md`）

  **目标**：在不限流、默认并发的前提下证明结构修复没有破坏现有行为（K-1 已在 P2.1-001 定性为未复现）。
  **前置依赖**：P2.1-012。
  **涉及文件 / Crate**：产品代码无改动；首轮红灯按 K-3 改了 `crates/bin-tests/src/cases/disconnect.rs` 的一处等待（断言语义不变）。
  **实施步骤**：
  1. 不传 `--concurrency` 跑 `make test ARGS="--all"` **连续 3 次**，逐次记录失败数与耗时分布。
  2. 记录同一轮的关键指标快照（Run 排队、worker 阻塞、清理链耗时、通道满次数）。
  3. 与 P2.1-001 基线做前后对照。
  **验收标准**：
  - [x] 3 次全绿，失败数 0（**回归**判据：K-1 已在 P2.1-001 定性为未复现，见 D-P2.1-007）。首轮曾有 1 条红（`runtime.disconnect`，81ms），归因为 P2.1-006 遗留的用例时序缺口（K-3，D-P2.1-014），修用例后三轮全绿、失败数 0。
  - [x] `default_concurrency()` 与 `runner` 的分片调度与修复前一致（无新增限流）：`default_concurrency()` 仍是 `available_parallelism()`，`runner` 未新增许可，`git diff` 只有 `cases/disconnect.rs` 的一处等待。
  - [x] 报告里给出 K-1 的最终口径（未复现 + 环境因素未排除 + 五档加压结果）与结构修复的「修复前 / 修复后」指标对照。**已在 P2.1-014 定稿**：口径与五档加压见 [`docs/reports/p2.1-014-acceptance.md`](../../docs/reports/p2.1-014-acceptance.md) §二 / §五。
  **测试要求**：`make test ARGS="--all"` × 3（默认并发）。**已授权并执行**（修复前 3 次 + 修复后 3 次，另加两次 `runtime` 分批定位）。
  **实施结果**：并发 18，修复前 72/73（35s）、73/73（35s）、73/73（35s）；修复后 73/73（36s）、73/73（36s）、73/73（35s）。指标快照：同步阻塞 0、异步轮询 27 次约 4.9s、会话锁持有 22 次最长 2ms、清理链最长 0ms、下行通道丢弃 1 次/轮。与 P2.1-001 相比墙钟与用例耗时之和同量级，**不宣告修复收益**；G-1-3 的「修复前 / 修复后」对照仍取自 P2.1-004 ~ 008 的报告。

---

## Batch E：并发配置增强与验收（P2.1-014 ~ P2.1-015）**已完成（2026-11-22）**

> 依赖：**P2.1-013 通过**。这是硬前置：必须先证明结构修复没有破坏现有行为（默认并发全量回归 + `max_concurrent_runs = 4`），才允许改默认值与语法，否则无法区分「修复生效」与「默认值变相放宽」。
>
> 本批次只改**服务端运行参数**的配置面，不动测试侧并发。
>
> 执行顺序：015（配置增强）→ 014（验收与报告，结论覆盖 015 的变更）。

- [x] **P2.1-015 `max_concurrent_runs` 支持核数倍数（`NC`）形式**（已完成，判据与数据见 `docs/reports/p2.1-014-acceptance.md` G-3 一节；决策见 `decisions.md` D-P2.1-015）

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
  - [x] `"2C"` 在注入 8 核时解析为 16；`"3C"` 为 24；整数 `4` 仍为 4（`llm.config` 的纯函数断言：缺省 3C → 24、`"2C"` → 16、`4` → 4）。
  - [x] `[agent]` 缺省时默认取 `3C`（注入 8 核时为 24）。`DEFAULT_MAX_CONCURRENT_RUNS` 的类型改为 `Concurrency`，值与解析点只有一处。
  - [x] 非法值（`0`、`0C`、`2X`、负数、溢出）各有明确错误文案且点名字段（`llm.config` 的 5 类断言，文案片段见 P2.1-014 报告）。
  - [x] 显式 `max_concurrent_runs = 4` 下 `--all` 全绿（与 P2.1-013 一致），默认 `3C` 下同样全绿（73/73 × 2，墙钟 33s；显式 4 一轮在跑前把共享服务端模板临时改为 4，跑完已还原，`git diff` 无残留）。
  - [x] `git diff` 中没有测试侧限流改动，无用例名变化（产品侧只动 `types-core` / `lib-bin-cli` / `agent-runtime`，用例侧只扩 `llm.config`）。
  **测试要求**：`make test ARGS="llm.config"`、`make test ARGS="run"`、`make test ARGS="--all"`（显式 4 与默认 3C 各一轮）。**已授权并执行**（1/1、10/10、73/73 × 2）。
  **完成产物**：核数倍数的并发配置；`decisions.md` D-P2.1-015（`Concurrency` 值类型与解析位置）；文档同步（`docs/develop.md` 的配置面、`.steps/README.md` 的 P2.1 状态）已在 P2.1-014 授权后一并处理。

---

- [x] **P2.1-014 验收与报告**（已完成，见 [`docs/reports/p2.1-014-acceptance.md`](../../docs/reports/p2.1-014-acceptance.md)）

  **目标**：按 README §七 判据逐条自查，产出阶段报告并更新阶段状态。
  **前置依赖**：P2.1-013、P2.1-015。
  **涉及文件 / Crate**：`docs/reports/p2.1-*.md`（新建）、`.steps/p2.1/`、`.steps/README.md`（需授权）。
  **实施步骤**：
  1. 报告结构沿用 P1.1 / P2：逐条判据 + 证据、假设验证结论（含被否决项）、偏差与遗留、下游（P3）输入清单。
  2. 记录每处修复的「修复前 / 修复后」数据；被否决的假设（如 ArcSwap）写明判断依据。
  3. 更新阶段状态与 P3 的开工前置说明（文档同步需授权）。
  **验收标准**：
  - [x] README §七 的 G-1 / G-2 / G-3 判据全部可核（报告 §二 ~ §四 逐条列表，G-3-4 由显式 4 与默认 3C 两轮 73/73 支撑）。
  - [x] 每个根因假设都有「成立 / 不成立 + 证据」的结论，无沉默项（H-1 / H-2 / H-3 × 2 / H-4 / 表级快照 / K-1 共 7 条，见报告 §五）。
  - [x] 偏差逐条写「处置 + 归属」（7 条，见报告 §六）。
  **测试要求**：无（证据来自 P2.1-012 / 013 / 015）。**已执行**：Batch E 的 `llm.config`、`run` 与两轮 `--all` 已授权执行，数据见报告 §四。
