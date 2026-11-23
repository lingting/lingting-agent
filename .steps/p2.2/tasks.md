# P2.2 任务清单

**状态：Batch A / B 已落地并按实测验收；Batch C（P2.2-008 ~ P2.2-010）已落地并按实测验收（P2.2-010 的失败注入一项未做，见该任务）；Batch D 及之后未开始。** 阶段目标、边界、技术方案与判据见 [README.md](README.md)，决策见 [decisions.md](decisions.md)。Batch A / B / C 的证据为一次性实测（无新增用例代码之外的产物），待 P2.2-016 汇总进阶段报告。

批次之间强序，批次内可并行。**每个任务的「测试要求」只允许在 `crates/bin-tests` 落地用例；其他 Crate 不得写 `#[cfg(test)]`**（仓库 `AGENTS.md` 第 28 条）。**所有需要执行用例的步骤都必须先取得明确授权**；未授权时只推进代码、静态检查与文档部分，相关验收项保持未勾选。

本阶段的三条硬线：

1. **数据不从日志来**：新增或修改的任何断言、报告字段都不得读取日志文本；需要数据就用 `metrics` 特性下的内存快照。
2. **不写死数字**：压力用例的客户端数量必须由标定与公式得出。
3. **正式构建零代价**：所有采集、审计、导出都在 `metrics` 特性下，且不新增第三方依赖。

---

## Batch A：数据通道（快照聚合与出口，P2.2-001 ~ P2.2-004）

> 依赖：无。本批次只做「把内存里的数字变成结构化出口」，不改任何业务行为。B 批次依赖本批次的出口。

- [x] **P2.2-001 快照聚合 DTO 与序列化**

  **目标**：让「一个进程的运行时指标」有一份完整、可序列化的快照，且子进程与测试进程读同一个类型。
  **前置依赖**：无。
  **涉及文件 / Crate**：`crates/manager-agent/src/runtime/metrics.rs`、`crates/manager-agent/Cargo.toml`、`crates/agent-runtime/src/metrics.rs`（只读对照）。
  **实施步骤**：
  1. 现有 `MetricsSnapshot` 增加两段：`limit`（限流：`queued` / `queued_max` / `running_max` / `running_now` / `wait_total_millis` / `wait_max_millis` / `timeouts` / `cancelled`）与 `audit`（P2.2-003 产出，先留字段占位或同批落地）。
  2. 在 `manager-agent` 的指标模块增加注册点：`install_limit_provider(fn() -> LimitMetrics)`，`snapshot()` 聚合时调用（未注册时为零值）。`manager-agent` **不得**依赖 `agent-runtime`。
  3. 在 `metrics` 特性下为快照与其子结构实现 `Serialize` / `Deserialize`（`serde` 已在工作区依赖内）；非 `metrics` 构建不引入序列化代价。
  4. 保留 `snapshot()` 的「纯内存读、不重置」语义。
  **验收标准**：
  - [x] 非 `metrics` 构建下 `metrics` 模块仍是空实现（无序列化、无额外依赖）；`cargo check -p manager-agent` 与 `cargo check -p bin-agent` 通过。
  - [x] `metrics` 构建下 `Snapshot` 可序列化成 JSON 再反序列化回等值结构（已直接用探针产出的 JSON 实测往返等值，不再依赖 B 批次的读取链路）。
  - [x] `manager-agent` 的 `Cargo.toml` 不新增对 `agent-runtime` 的依赖边。
  **测试要求**：无（不写测试代码）。检查方式为 `cargo check` 与 `git diff`。

- [x] **P2.2-002 快照出口：debug + `metrics` 双限定的探针**

  **目标**：让测试进程能从服务端子进程取到快照，不需要日志、不需要在进程内装配。
  **前置依赖**：P2.2-001。
  **涉及文件 / Crate**：`crates/lib-web/src/probe/api.rs`、`crates/lib-web/Cargo.toml`、`crates/lib-bin-web/Cargo.toml`、`crates/bin-agent/Cargo.toml`、`crates/lib-bin-web/src/server.rs`（注册 `limit` 提供者）。
  **实施步骤**：
  1. `lib-web` 增加 `metrics` 特性（`metrics = ["manager-agent/metrics"]`），`lib-bin-web` 的 `metrics` 追加 `lib-web/metrics`，`bin-agent` 的 `metrics` 保持透传。
  2. 新增 `#[cfg(all(debug_assertions, feature = "metrics"))] #[web_api_get(path = "api/probe/metrics", auth = AuthRule::anonymous())]` 的 handler，返回 `manager_agent::runtime::metrics::snapshot()`。
  3. `lib-bin-web` 的 `run()` 在装配完成处（`manager_agent::install` 附近）用 `#[cfg(feature = "metrics")]` 注册 `agent_runtime::metrics::snapshot` 的转写函数。
  4. 确认 `make gen`（不带 `metrics` 特性）生成的 SDK 不包含该路径；接口清单只增这一条，且 release 不存在。
  **验收标准**：
  - [x] `make test` 构建下 `GET api/probe/metrics` 返回 JSON，含 P2.2-001 的 `limit` 段与既有各段（实测 `limit` 非零：`queued=2` / `running_max=1`）。
  - [x] `make build` / `make release` 产物的路由表不含 `api/probe/metrics`（实测：非 `metrics` 构建下该路径 404，同进程 `api/probe/health` 仍 200）。
  - [x] 无新增配置文件字段、无新增第三方依赖；`docs/api/probe.md` 的更新列入 P2.2-015，不在本任务内改。
  **测试要求**：需要执行一次带 `metrics` 的服务端验证接口返回（**需授权**）。

- [x] **P2.2-003 审计计数：事件级别、敏感串与 panic**

  **目标**：把「原来靠检索日志文本得到的三个事实」变成内存计数：日志事件数 / ERROR 数、敏感串（`[provider] api_key`）是否出现在任何日志事件里、进程内 panic 次数。
  **前置依赖**：P2.2-001（`audit` 段的落点）。
  **涉及文件 / Crate**：`framework-core`（本地框架仓库）的 `LoggingConfig`、`crates/lib-core/src/logger.rs`、新增 `crates/lib-core/src/audit.rs`、`crates/lib-core/Cargo.toml`、`crates/manager-agent/src/runtime/metrics.rs`（转写 `audit`）。
  **实施步骤**：
  1. `framework-core`：`LoggingConfig` 增加 `layers: Vec<Box<dyn Layer<Registry> + Send + Sync>>` 与 `push_layer`，`logging::init` 把它们挂到 subscriber 上（与现有 console / file 层并列）。不动现有字段语义。
  2. `lib-core` 增加 `metrics` 特性，审计的计数与快照留在 `lib-core`（`lib_core::audit::snapshot()`）；`manager-agent` 的 `Snapshot.audit` 转写它（`manager-agent → lib-core` 是既有依赖方向，`lib-core` 不得反向依赖任何上层）。
  3. `lib-core::logger::init` 在 `metrics` 特性下安装审计 Layer：
     - 按 `Level` 计数（`total` / `error` / `warn`）；
     - 敏感串比对：从 `LoggingConfig` 之外传入被监控的值（由 `bin-agent` 在日志初始化前从 `[provider] api_key` 取一份），用字段访问器（`Visit`）比对，**不**为每条事件分配字符串；
     - 命中只加计数，不改写、不过滤、不落盘。
  4. `lib-core` 在 `metrics` 下设置 panic hook：先调既有默认 hook（保留原输出），再累加 `panics` 计数。
  5. `lib-db` 在 `metrics` 下累加 `database is locked` / `SQLITE_BUSY` 计数（`lib_db::observability` 增加 `errors` 段），由 `manager-agent` 转写。
  **技术要求**：审计层只在 `metrics` 构建存在；正式构建的 `init` 路径与现在逐字节等价（除 cfg 分支）；审计层不得改变事件的内容与去向。
  **验收标准**：
  - [x] `metrics` 构建下快照有 `audit.total` / `audit.error` / `audit.warn` / `audit.secret_hits` / `audit.panics` 与 `database.errors`。
  - [x] 故意让服务端打一条含 `api_key` 的日志（临时手工验证即可，**不得**提交为用例）时 `secret_hits` 增长：实测 `secret_hits` 0 → 1、`audit.warn` 0 → 1，注入代码已还原并重建确认为 0；结论写进阶段报告。
  - [x] 非 `metrics` 构建下 `lib-core` 不编译审计代码，`make build` 通过；事件输出与改动前一致。
  - [x] 热路径不分配：字段比对用访问器实现，代码可核。
  **测试要求**：无（不写测试代码）；人工验证一次计数增长（**需授权**执行）。

- [x] **P2.2-004 删除为采集而打的阈值日志**

  **目标**：把「用日志传递测试数据」的最后一环去掉——阈值日志删掉，内存计数留下。
  **前置依赖**：P2.2-003（计数已齐，才敢删日志）。
  **涉及文件 / Crate**：`crates/manager-agent/src/runtime/metrics.rs`、`crates/agent-runtime/src/metrics.rs`、`crates/lib-db/src/observability.rs`、`crates/lib-db/Cargo.toml`（`tracing` 可选依赖随之处理）。
  **实施步骤**：
  1. 删除三处模块里的全部阈值常量与 `warn!` / `debug!` 分支（`CHAIN_WARN_MS` / `STOP_WARN_MS` / `APPEND_WARN_MS` / `LOCK_WARN_MS` / `LOCK_WAIT_WARN_MS` / `QUEUE_DEBUG_MS` / `QUEUE_WARN_MS` / `ACQUIRE_DEBUG_MS` / `ACQUIRE_WARN_MS` 及其调用点）。
  2. 保留 `Instant::elapsed()` 与原子累加；保留运行期流程日志（`handler` / `session` / `journal` / `queue` 里带 ID 的 `info!` / `warn!`，例如「建立运行时会话失败，已撤回占用登记」）。
  3. `lib-db` 移除 `tracing` 需要在 `metrics` 特性下编译的注释与依赖声明，并同步 D-P2.1-009 的口径（在 `decisions.md` 里注明该依赖回退）。
  4. 全仓检索确认 `metrics` 特性下不再有「阈值 → 日志」分支。
  **验收标准**：
  - [x] 三处模块里不再出现阈值常量与采集型日志调用；`git diff` 可核。
  - [x] 运行期流程日志一条不少（实测日志仍含 `run_id` / `session_id` 的排队失败、Context 组装等流程事件）。
  - [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过（含 `--features metrics` 的目标）。
  - [x] 快照字段与 P2.2-001 一致，未因删日志而丢数据。
  **测试要求**：无（不写测试代码）。

---

## Batch B：采集与报告收敛（P2.2-005 ~ P2.2-007）

> 依赖：Batch A。本批次只改 `crates/bin-tests`：数据进来、报告出去，中间不经过日志。
>
> **进度**：三个任务已落地并按实测验收。`cargo fmt --check` 与 `cargo clippy --workspace --all-targets -- -D warnings` 通过；`make test ARGS="llm persist run"` 19 个用例全绿；`make test ARGS="--all"` 73 个用例全绿（报告 `target/tests/run-1795428485492-31504`）。自管用例里在用例内停掉服务端的三条 `persist.*` 与 `web.e2e`、以及触发优雅关闭的 5 条用例，其「运行数据」显式写「未采集：原因」。

- [x] **P2.2-005 快照采集入口**

  **目标**：`bin-tests` 能用一份代码取到「本进程快照」与「子进程快照」，并区分「未采集」。
  **前置依赖**：P2.2-002（接口）、P2.2-001（DTO）。
  **涉及文件 / Crate**：`crates/bin-tests/src/util/server.rs`、`crates/bin-tests/src/util/observability.rs`、`crates/bin-tests/src/util/support.rs`（进程内装配的快照读取）。
  **实施步骤**：
  1. `util/server.rs` 增加 `Server::metrics()`：在服务端仍存活时 `GET api/probe/metrics`，反序列化成 `manager_agent::runtime::metrics::Snapshot`；404 / 连接失败返回「未采集 + 原因」，不 panic。
  2. `util/observability.rs` 从「渲染固定几段」改为「入参一份或多份快照 + 渲染」，渲染函数本身不认识日志。
  3. 明确两种来源的标注：`本进程（holder 装配）` 与 `子进程（<用例> 的服务端）`。
  4. 保留 `lib_bin_tui::testing::blocking_snapshot()` 一段（它是测试进程内存计数，不涉及日志）。
  **验收标准**：
  - [x] 隔离 / 自管用例执行完能拿到服务端快照（实测 `run.queue_timeout` 的「运行数据」有完整限流段）；构建未开特性时得到「未采集：接口不存在（用 make test 构建）」（实测非 `metrics` 的 `bin-agent` + `llm.config`）。
  - [x] `util/observability.rs` 内不出现任何文件读取（`fs::read*`）。
  - [x] `cargo check -p bin-tests` 无未使用 import。
  **测试要求**：需要跑一条 isolated 用例观察拿到快照（**需授权**）。

- [x] **P2.2-006 报告结构化：`CaseReport` 增加数据段**

  **目标**：每个用例的报告章节能承载「运行数据」，且与观测数据一节共用同一渲染器。
  **前置依赖**：P2.2-005。
  **涉及文件 / Crate**：`crates/bin-tests/src/report.rs`、`crates/bin-tests/src/runner.rs`、`crates/bin-tests/src/util/observability.rs`。
  **实施步骤**：
  1. `CaseReport` 增加字段：服务端快照 `Option<Snapshot>`、用例自带度量 `Vec<(String, String)>`（如压力用例的 `T` / `L0` / `N`）。
  2. `section_lines` 在「结果信息」之后追加「运行数据」小节，由 `observability` 的渲染函数产出；缺失时写「未采集：原因」。
  3. README 的统计与索引结构保持兼容（行号索引仍可算：新小节计入该用例的行数）。
  4. `runner` 在用例收尾阶段（服务端停止之前）取快照并填进 `CaseReport`。
  **验收标准**：
  - [x] 报告里每个用例章节都有「运行数据」（或显式「未采集」）；README 的行号索引仍指向正确位置（实测 `--all` 73 个用例：73 个「运行数据」小节、9 个显式「未采集」、索引起始行均为 `## <用例名>`）。
  - [x] 观测数据的 Markdown 只有一处生成逻辑（`git diff` 可核）。
  - [x] 报告不新增「日志行数」「日志内容」类字段。
  **测试要求**：跑一条用例后人工读报告（**需授权**）。

- [x] **P2.2-007 四处日志断言的等价替换**

  **目标**：删掉用例对 `server.log` 的读取，换成快照字段断言，且断言的**力度不变**。
  **前置依赖**：P2.2-003（审计与 DB 计数）、P2.2-005（采集入口）。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/llm/canary.rs`、`crates/bin-tests/src/cases/persist.rs`、`crates/bin-tests/src/cases/run.rs`、`crates/bin-tests/src/util/server.rs`（`start_with_level` 的调用点）。
  **实施步骤**：
  1. `llm.canary`：断言改为 `audit.total > 0`（证明日志确实在产出）+ `audit.secret_hits == 0`；失败文案点名 `audit.secret_hits`。删除 `server_log_files()` 与 `read_to_string`。
  2. `persist.*`（两处）：断言 `audit.panics == 0` 与 `audit.error == 0`；删除日志行数统计与文本扫描。
  3. `run.*`（一处）：断言 `database.errors == 0`（`database is locked` / `SQLITE_BUSY` 计数为 0），保留原有的「无 panic」断言（改用 `audit.panics`）。
  4. `start_with_level(…, "trace")` 的用途消失后，检查 `start_with_level` / `serve_once_level` 是否还有调用者；无调用者则删除（避免留下只为日志存在的入口）。
  5. 全仓检索 `server.log` / `logs` 目录读取：`crates/bin-tests` 内必须归零。
  **验收标准**：
  - [x] `crates/bin-tests` 中 `read_to_string` 与日志目录遍历只剩 0 处（`grep` 可核）。
  - [x] 四个断言点各有对应的快照字段，失败信息点名该字段。
  - [x] 用例名、断言语义与等待上限不变（除「数据来源」外零口径变化）。
  - [x] `llm.canary` / `persist.*` / `run.*` 三组用例通过（**已授权**；19 个用例全绿，另跑 `--all` 73 个用例全绿）。
  **测试要求**：`make test ARGS="llm persist run"`（**需授权**）。

---

## Batch C：边界压力用例（P2.2-008 ~ P2.2-010）

> 依赖：Batch B（证据必须来自快照）。本批次只新增用例与支持代码，不改服务端行为。
>
> **进度**：三个任务已落地并按实测验收。与 README §4.1 的三处口径偏差已记入 decisions.md 的
> D-P2.2-013：用例形态用 `SelfServed`（`Isolated` 注入不了 `[agent]` 配置）、`L0` 取「发消息 → 终态」的
> 串行段（先跑 1 轮预热丢弃冷启动）、② 的总耗时上界是 `T + L0` 而不是 `T`。
> 实测：`make test ARGS="stress"` 连续 5 次全绿（`L0` 55.6 ~ 64.6ms、`N` 47 ~ 54、② 总耗时 2722 ~ 3046ms、
> 排队余量 163 ~ 572ms、③ 超时 39 ~ 51 条、服务端停止 21 ~ 42ms）；`make test ARGS="--all"` 74 个用例全绿，
> `stress.boundary` 墙钟 9.7s（D-P2.2-003 的增量登记）。

- [x] **P2.2-008 边界模型与标定（纯计算 + 报告口径）**

  **目标**：把「客户端数量」从拍脑袋变成可复算的公式，并把推导过程写进代码注释与报告。
  **前置依赖**：P2.2-005。
  **涉及文件 / Crate**：新增 `crates/bin-tests/src/util/stress.rs`、`crates/bin-tests/src/util/mod.rs`。
  **实施步骤**：
  1. `util/stress.rs` 定义常量与纯函数：等待预算 `QUEUE_BUDGET = Duration::from_secs(3)`、`boundary(per_run: Duration, budget: Duration) -> Option<usize>`（返回 `floor(budget/per_run) + 1`；`per_run` 为 0 或结果小于最小样本数时返回 `None`）、`MIN_CLIENTS`（低于它说明参数不足以形成边界）。
  2. 标定辅助：跑 `k = 3` 次单客户端链路，取最大耗时作 `L0`；记录三次的原始值用于报告（最小 / 中位 / 最大）。
  3. 单次耗时分布统计（min / 中位 / p95 / max）用一个纯函数实现，压力用例两阶段共用。
  4. 所有阈值与公式常量集中在 `stress.rs`，用例文件里不出现数字。
  **验收标准**：
  - [x] 公式与推导写在模块文档里（含 `N_safe` / `N_bad` 的由来）。
  - [x] `boundary` 对 `L0 = 10ms, T = 3s` 得 `301`，对 `L0 = 30ms` 得 `101`（用注释示例说明，不写测试代码）。
  - [x] 用例目录里没有固化的客户端数量（`N` 每次由标定算出，实测 5 次分别是 53 / 50 / 47 / 49 / 54）。
  **测试要求**：无（不写测试代码）。

- [x] **P2.2-009 `stress.boundary` 用例**

  **目标**：按 README §4.1 的三阶段落地用例：压到串行、摆在内边界、用外边界自证。
  **前置依赖**：P2.2-008。
  **落地口径（与下面第 1、2 步的差异见 D-P2.2-013）**：`CaseKind::SelfServed` 自起私有服务端与私有
  `MockServer`（`Isolated` 的私有服务端配置由调度层固定，注入不了 `[agent]` 参数）；`L0` 取
  「发消息 → 终态」的串行段，并先跑 1 轮预热丢弃冷启动（完整链路耗时只写进报告做对照）；
  ② 的耗时断言上界是 `T + L0`（末个 Run 还要自己跑完一个串行段）。
  **涉及文件 / Crate**：新增 `crates/bin-tests/src/cases/stress.rs`、`crates/bin-tests/src/cases/mod.rs`、`crates/bin-tests/src/util/stress.rs`、`crates/bin-tests/src/util/flow.rs`（复用注册 / 占用 / 终态等待）。
  **实施步骤**：
  1. 用例形态：`CaseKind::Isolated`（私有服务端 + 私有 `MockServer`）；配置为 `max_concurrent_runs = 1`、`run_queue_timeout_ms = 3000`，`[provider]` 指向私有 Mock。
  2. 阶段 ①：标定 `L0`（3 次取最大），算 `N`；`N < MIN_CLIENTS` 时以「参数不足以形成边界」失败并给出 `L0` / `T`。
  3. 阶段 ②：`N` 个客户端各自「注册 → 建会话 → 占用 → 订阅流 → 发消息」，起始时刻对齐（同批 spawn，先全部注册与占用完成再同一时刻发消息）；收集每条 Run 的终态、耗时与完成时刻；结束时从服务端取快照。
  4. 阶段 ③：`2 × N` 个客户端重复同一套流程，收集 `FAILED` 条数与原因；再取一次快照。
  5. 断言按 README §4.1 的表格实现；失败时输出「完成 m/N、最后完成时刻、超出预算多少、快照关键字段」。
  6. 报告条目：`T` / `L0`（三次原始值 + 取值）/ `N` / `2N` / ②实测总耗时与余量 / 耗时分布 / ③超时条数 / 两阶段快照（`limit` 段：`queued_max` / `running_max` / `wait_max_millis` / `timeouts`）。
  7. 收尾：不逐个 `client_delete`（避免用 300 条清理链污染收尾时间），直接用例结束由 `runner` 停止服务端；把这一点写进用例注释与报告口径。
  **验收标准**：
  - [x] `bin-tests --list` 出现 `stress.boundary`；`make test ARGS="stress"` 可单跑（实测通过，连续 5 次全绿）。
  - [x] 报告给出 README §4.1 要求的全部字段；数字全部来自内存（客户端事件 + 测试进程计时 + 服务端快照）。
  - [x] 用例不读任何日志；不占用共享服务端与共享 Mock（`git diff` 可核：`cases/stress.rs` 无任何文件读取，端口取自 `server::free_port()`）。
  - [x] 缺少快照时用例失败并提示「用 `make test` 构建」（代码路径：`capture()` 对 404 直接以该提示失败，不静默跳过断言）。
  **测试要求**：`make test ARGS="stress"` 单跑一次并读报告（**已授权并执行**）。

- [x] **P2.2-010 稳定性、收尾与失败诊断**

  **目标**：让这条边缘用例在日常回归里可靠：不会因为收尾、连接泄漏或诊断不足而变成不可解释的红灯。
  **前置依赖**：P2.2-009。
  **涉及文件 / Crate**：`crates/bin-tests/src/cases/stress.rs`、`crates/bin-tests/src/util/stress.rs`、`crates/bin-tests/src/runner.rs`（若需按用例调整超时）。
  **实施步骤**：
  1. 收尾健壮性：压测结束后关闭全部 SSE 流、放弃未完成的等待，确认没有遗留连接拖住服务端停止；服务端停止耗时写入报告。
  2. 用例总超时核算：`runner` 的 `CASE_TIMEOUT`（60s）是否覆盖 `①②③` 三段（约 2 × `T` + 标定）；不足则在用例支持层显式申请更长预算，而不是靠猜。
  3. 失败诊断：断言失败时一次性输出「阶段、期望、实际、快照关键字段」，避免只报一句「超时」。
  4. 与 `--all` 的关系：按 D-P2.2-003 的结论登记（默认纳入则记录墙钟增量；不纳入则在 README 注明单跑命令）。
  5. 连续 3 次同一配置全绿，记录每次的 `L0` / `N` / 实测总耗时（用于判断稳定性，而不是为了「调参直到绿」）。
  **验收标准**：
  - [x] 连续 3 次全绿，且三次的 `L0` / `N` / 余量写入报告或阶段报告（实测连续 5 次：`L0` 64.6 / 60.2 / 56.6 / 62.0 / 57.4ms，`N` 47 / 50 / 53 / 49 / 54，② 排队余量 572 / 470 / 338 / 450 / 249ms）。
  - [ ] 失败注入（人为把 `run_queue_timeout_ms` 调到 `L0` 以下一次）时，失败信息能点名阶段与字段：一次性核对后立即还原，不提交该改动（**未做**：需要单独授权的临时改动，本次未提交）。
  - [x] 用例结束后无遗留进程与连接（`--all` 跑完后共享端口可用；服务端停止耗时 21 ~ 42ms 已进报告）。
  **测试要求**：`make test ARGS="stress"` × 3 与一次 `make test ARGS="--all"`（**已授权并执行**）。

---

## Batch D：`server --async`（P2.2-011 ~ P2.2-013）

> 依赖：无（与 A / B / C 无交集，可并行）。

- [ ] **P2.2-011 CLI 参数与配置解析**

  **目标**：`agent server -a` / `--async` 可解析，且与现有参数自由组合。
  **前置依赖**：无。
  **涉及文件 / Crate**：`crates/lib-bin-cli/src/command.rs`、`crates/lib-bin-cli/src/config.rs`。
  **实施步骤**：
  1. `ServerArgs` 增加 `#[arg(short = 'a', long = "async")] pub asynchronous: bool`（字段名避开关键字），帮助文案写明「派生一个常驻服务进程后立即返回」。
  2. 确认与 `--addr` / `--port` / `--log-level` 无冲突；不新增配置文件字段、不新增环境变量。
  3. `resolve_server` 的产物增加「是否派生」的标记（或由调用方传入），配置解析本身不变。
  **验收标准**：
  - [ ] `agent server --help` 含 `-a, --async` 与中英文案一致的一行说明。
  - [ ] 参数组合矩阵（`--async` 单独、+`--port`、+`--log-level`）都能解析。
  - [ ] 配置文件面零变化。
  **测试要求**：无（不写测试代码）；`agent server --help` 人工核对。

- [ ] **P2.2-012 派生进程实现与就绪语义**

  **目标**：派生一个常驻服务进程，等它就绪后打印信息并退出；失败路径可读。
  **前置依赖**：P2.2-011。
  **涉及文件 / Crate**：新增 `crates/lib-bin-cli/src/server.rs`（或并入 `runtime.rs`）、`crates/lib-bin-cli/src/lib.rs`、`crates/bin-agent/src/main.rs`、`crates/lib-bin-cli/Cargo.toml`（若需 `tokio` 进程之外的能力）。
  **实施步骤**：
  1. 参数反向构造：把已解析的 `ServerConfig` 还原成子进程命令行（`server --addr … --port … --log-level …`），**不带** `--async`；`AGENT_HOME` 等环境原样继承。
  2. 派生：`std::env::current_exe()` + `std::process::Command`；`stdin(null)`，`stdout` / `stderr` 重定向到 `<AGENT_HOME>/logs-server/stdout.log`（追加写）；Windows 加 `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` 创建标志；Unix 只做 stdio 重定向。
  3. 父进程**不等待、不 kill**：`Child` 用完即弃（不得启用任何 kill-on-drop 语义）。
  4. 就绪：轮询 `Client::probe_health`（上限与 `util/server.rs` 的 20s 同量级），就绪后写 `<AGENT_HOME>/server.pid` 并打印 `PID` / `地址` / `日志目录`，退出 `0`。
  5. 失败：子进程秒退（`try_wait` 命中）或超时 → 打印一行原因 + 子进程 PID + 日志路径，退出非 0（参数/配置错误 2，运行期错误 1，与现有 `CommandError` 口径一致）。
  6. 帮助与文档（`docs/*` 的部分归 P2.2-015）里写明 Unix 的 SIGHUP 限制与 PID 文件位置。
  **验收标准**：
  - [ ] `agent server --async` 返回后：父进程已退出、`api/probe/health` 可用、`agent status` 可用、`<AGENT_HOME>/server.pid` 存在且内容与打印一致。
  - [ ] 关闭父 shell（或结束父进程）后子进程仍存活并被探活。
  - [ ] 端口被占用时退出码非 0、只输出一行原因；不 panic、不静默留下半启动进程。
  - [ ] 不带 `--async` 的前台路径逐字节等价（`git diff` 只新增分支，不改既有流程）。
  **测试要求**：人工验证三条（就绪 / 常驻 / 端口冲突），结论写进阶段报告（**需授权**执行）。

- [ ] **P2.2-013 用例覆盖 `cli.async_server`**

  **目标**：把「派生即返回、子进程常驻」纳入回归，而不是只靠人工验证。
  **前置依赖**：P2.2-012。
  **涉及文件 / Crate**：新增 `crates/bin-tests/src/cases/cli.rs`、`crates/bin-tests/src/cases/mod.rs`、`crates/bin-tests/src/util/process.rs`（按 PID 结束进程的跨平台封装）。
  **实施步骤**：
  1. 用例：`CaseKind::SelfServed`（用例自己决定服务端启停时机，入口只给目录与端口）；用 `bin-agent server --async --port <空闲端口>` 起服务端，断言命令在就绪后退出 `0`。
  2. 断言：`api/probe/health` 可用、PID 文件存在、父进程已退出（`Child::try_wait` 为 `Some`）。
  3. 收尾：按 PID 文件结束子进程（Windows `taskkill /F /PID`，Unix `kill -9`），断言进程消失；用例无论成败都必须走到清理（`Drop` 兜底）。
  4. 断言不得读取任何日志文本；需要的信息全部来自命令行输出、PID 文件与探活接口。
  **验收标准**：
  - [ ] `make test ARGS="cli"` 可单跑并通过；用例结束后无遗留 `bin-agent` 进程。
  - [ ] 断言与清理逻辑不依赖日志；失败时报告给出 PID 与端口。
  - [ ] 用例不占用共享服务端端口（20104）。
  **测试要求**：`make test ARGS="cli"`（**需授权**）。用例落地后若要长期保留，必须在 `docs/develop.md` 的端口表与用例清单里登记（归 P2.2-015）。

---

## Batch E：纪律入库与验收（P2.2-014 ~ P2.2-016）

> 依赖：Batch B 与 Batch C（条款要照着已落地的实现写）。

- [ ] **P2.2-014 `AGENTS.md` 增加「测试数据采集」条款**

  **目标**：把本阶段的纪律写进仓库约束，后续阶段不得再靠日志取数。
  **前置依赖**：P2.2-007（实现已落地）。
  **涉及文件 / Crate**：仓库根 `AGENTS.md`（`### 测试` 一节之后新增小节）。
  **条款草案**（落地时按实际实现微调，措辞保持可核）：

  ```markdown
  ### 测试数据采集

  - 禁止通过日志收集测试数据或判定依据：不得读取、解析或文本检索 `server.log`、日志目录、
    归档日志来生成报告、执行断言或计算指标；日志只用于人工排查。
  - 需要可观测数据时，只允许「编译期特性 + 进程内内存采集 + 结构化快照」：采集代码必须在
    特性下（如 `metrics`），正式构建为空实现；快照由测试进程直接读取（探针接口或结构化文件），
    不经过日志。
  - 断言「敏感信息未泄漏」时，必须在写出侧检测（日志层或采集点，命中即计数），不得事后扫描日志文本。
  - 数据缺失必须显式报告「未采集」（名称 + 原因），不得从日志推断、不得留空、不得因为构建未开特性
    而静默跳过断言。
  - 报告中的数字必须来自快照；同一语义只有一个采集点与一个渲染点。
  - 用例失败时报告给出日志路径即可，不允许把它变成数据来源。
  ```

  **验收标准**：
  - [ ] 条款落在 `AGENTS.md` 的测试相关章节内，与既有条款不冲突、不重复。
  - [ ] 条款逐条对应可核的实现（G-3-1 ~ G-3-7）。
  - [ ] 未出现「暂不适用 / 待实现」的例外；有例外必须先进 `decisions.md`。
  **测试要求**：无。

- [ ] **P2.2-015 文档同步（待授权）**

  **目标**：让文档跟上本阶段的三处变化：新增探针接口、`server --async`、用例与端口清单。
  **前置依赖**：P2.2-002、P2.2-012、P2.2-013。
  **涉及文件 / Crate**：`docs/api/probe.md`（新增 `api/probe/metrics`，仅 debug + `metrics` 构建）、`docs/api/readme.md`（接口计数）、`docs/develop.md`（测试数据采集约定、端口表、用例清单）、`.steps/README.md`（P2.2 登记与进度）、`docs/workspace.md`（仅当依赖边发生变化时）。
  **实施步骤**：
  1. 逐项核对文档与代码的一致性，只改本阶段确实改变的条目。
  2. 接口计数与「服务端仍具备 N 条接口」这类表述同步（注意 release 不含该探针）。
  3. `.steps/README.md` 的阶段总览、依赖图、进度表同步。
  **验收标准**：
  - [ ] 授权范围内的文档改动与代码一致；未授权范围内的一律不动（在阶段报告里列出「发现但未改」的清单）。
  - [ ] 文档里的接口清单能解释「为什么 release 看不到 `api/probe/metrics`」。
  **测试要求**：无。**本任务开工前必须取得明确的文档修改授权**（`AGENTS.md` 文档维护约束）。

- [ ] **P2.2-016 阶段验收与报告**

  **目标**：按 README §六 的判据逐条取证，产出阶段报告。
  **前置依赖**：P2.2-010、P2.2-013、P2.2-014、P2.2-015。
  **涉及文件 / Crate**：`docs/reports/p2.2-016-acceptance.md`（新建）、`docs/reports/p2.2-*`（各任务的证据文件）。
  **实施步骤**：
  1. 逐条核对 G-1-1 ~ G-1-8、G-2-1 ~ G-2-6、G-3-1 ~ G-3-7、G-4-1 ~ G-4-2，每条给出「成立 / 不成立 / 未验证」与证据位置。
  2. 回归基线：默认并发下 `make test ARGS="--all"` 连续 3 次全绿（**回归**判据）；记录 `stress.boundary` 的墙钟增量。
  3. 数据来源声明：本阶段之后同一指标改由快照给出，与 P2.1 的日志口径不可直接相减（写进报告）。
  4. 未闭环项（例如框架改动未合并、文档未获授权）单独列出，不勾选对应任务。
  **验收标准**：
  - [ ] 判据表完整，无「未验证」却勾选的任务。
  - [ ] 报告写明三件事：边界用例的 `L0` / `N` 稳定性、`--async` 的人工验证结论、日志口径的清理范围。
  - [ ] 回归证据（连续 3 次）与 P2.1 基线对照，墙钟无显著恶化或给出归因。
  **测试要求**：`make test ARGS="--all"` × 3、`make test ARGS="stress"`、`make test ARGS="cli"`（**均需授权**）。
