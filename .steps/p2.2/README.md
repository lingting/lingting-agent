# P2.2：边界压力用例、后台服务端与测试数据通道收敛

**状态：已规划，未开工。** 任务与批次见 [tasks.md](tasks.md)，决策见 [decisions.md](decisions.md)，判据见本文 §六。

> 依据：P2.1 验收结论（[`docs/reports/p2.1-014-acceptance.md`](../../docs/reports/p2.1-014-acceptance.md)）、`docs/develop.md` 的测试约定、仓库根 `AGENTS.md` 的现有条款，以及仓库实际代码（`crates/bin-tests`、`crates/manager-agent`、`crates/agent-runtime`、`crates/lib-db`、`crates/lib-bin-cli`、`crates/lib-core`、`crates/lib-web`）。凡文档与代码不一致之处，以代码为准并记入 `decisions.md`。
>
> 本阶段**不修并发缺陷、不改业务语义、不改接口契约**。它做四件事：给并发能力补一条「卡在门槛上」的用例、给服务端补一个后台常驻启动入口、把测试数据的来源从日志换成进程内快照、把这条纪律写进 `AGENTS.md`。

---

## 一、阶段目标

| 目标 | 内容 | 结案证据 |
|---|---|---|
| **G-1** 边界压力用例 | 新增 `stress.boundary`：把服务端并行参数压到最小（`max_concurrent_runs = 1`），按「等待预算 ÷ 单次串行耗时」算出**理论上刚好不会失败**的客户端数并按其测试；同时用 2 倍客户端证明该门槛真实存在 | 用例连续 3 次全绿；报告给出 `T` / `L0` / `N` / 实测总耗时 / 余量 / 服务端快照；「外边界」阶段稳定出现排队超时 |
| **G-2** `server --async` | `agent server` 支持 `-a` / `--async`：内部派生一个常驻服务进程后父进程退出，打印 PID、地址与日志路径 | 前台行为零变化；`--async` 返回后服务端可访问、父进程已退出、子进程常驻；失败路径有一行原因与非 0 退出码 |
| **G-3** 测试数据通道收敛 | 测试数据只来自「编译期特性 + 进程内内存采集 + 结构化快照」：删掉为采集而打的阈值日志，删掉用例对 `server.log` 的文本检索，报告统一由快照渲染 | 全仓测试代码 `read_to_string(…server.log)` 归零；服务端子进程数据经由结构化出口获取；非 `metrics` 构建无采集、无出口 |
| **G-4** 纪律入库 | `AGENTS.md` 增加「测试数据采集」条款：禁止用日志收集数据或标识，只允许特性 + 内存采集 + 直接报告 | 条款可逐条对照代码实现核查 |

四者的关系：**G-3 是 G-1 的前置**（边界用例的全部证据都必须来自快照，否则又会绕回日志）；G-2 与 G-1 / G-3 无依赖，可并行；G-4 在 G-3 落地后写，避免写成「与实现不一致的条款」。

---

## 二、阶段边界

### In Scope

- `crates/bin-tests`：新增 `cases/stress.rs`，观察与报告入口收敛到一份快照 DTO，三个读日志的用例改断言口径。
- `crates/manager-agent`：快照聚合（新增限流段与审计段）、序列化、子进程出口所需的公开读接口。
- `crates/agent-runtime`、`crates/lib-db`：删除阈值型采集日志（保留内存累加）。
- `crates/lib-core`：日志审计层（事件计数、敏感串计数、panic 计数）挂在 `metrics` 特性下。
- `crates/lib-web`、`crates/lib-bin-web`、`crates/bin-agent`：`metrics` 特性下的快照出口与特性链。
- `crates/lib-bin-cli`：`server --async` 的参数解析、配置解析与派生进程实现。
- `framework-core`（本地框架仓库）：`LoggingConfig` 支持挂自定义 Layer（仅为审计层，见 D-P2.2-002）。
- 仓库根 `AGENTS.md`：新增测试数据采集条款（G-4）。

### Out of Scope

| 不做的事 | 说明 |
|---|---|
| 并发语义与接口变更 | 25 条接口、`RunEvent` 契约、`RunStatus` 状态机、断连顺序、同会话串行一律不动；新增的只有 `metrics` 构建下的探针 |
| `max_concurrent_runs` 之外的并行度改造 | DB 池（5）、下行通道（64）当前不可配置，本阶段不改造成可配置；若边界被它们决定，只在报告里写明 |
| Mock 契约变更 | 4 个端点、7 种场景、`chunk_ms` / `stall_ms`、观测字段保持；不为「凑 300」新增节奏参数 |
| 测试侧限流 | `cli::default_concurrency()` 不得改成硬上限；压力用例不得占用共享服务端 |
| 新增业务接口 / 新配置项 | 快照出口是 debug + `metrics` 双条件限定的探针，不进 release，不新增配置文件字段 |
| 新 Crate、`#[cfg(test)]` | 沿用 `AGENTS.md` 第 28 条：用例只写在 `bin-tests` |
| 把日志整体废掉 | 日志仍然是人工排查手段；只是**不再作为数据来源** |
| `agent server --stop` 等运维命令 | 需求只要求「新进程常驻后父进程退出」；停服能力见 D-P2.2-004 |

### 与前序阶段的硬约束

1. **事件通路不变**：先落盘 flush，再转发到 `api/client/stream`；文件是事实源。
2. **同会话串行不变**：同一会话同一时刻最多一个 `RUNNING` Run。
3. **正式构建零代价**：非 `metrics` 构建不付任何采集、审计、导出代价（沿用 D-P2.1-008 的口径）。
4. **不引入新的环境变量**：测试仍通过 `AGENT_HOME` 下的 `config.toml` 注入配置；`--async` 只读已有配置面。
5. **依赖不扩散**：不新增第三方依赖（含 `libc`，见 D-P2.2-005）。

---

## 三、现状与缺口（代码事实）

| 编号 | 现状 | 缺口 |
|---|---|---|
| C-1 | 服务端唯一可配置的并行度参数是 `[agent] max_concurrent_runs`（`types-core`，支持 `"3C"` 默认） | 没有任何用例把它压到 1 并按「容量边界」设计；并发能力只在「默认并发下全绿」这一条上被间接保护 |
| C-2 | `[agent] run_queue_timeout_ms`（默认 120s）是排队超时：等全局许可超过即落 `FAILED`「执行排队超时」（`agent-runtime/src/limit.rs`） | 没有用例把「等待预算」当成容量门槛来用；默认 120s 让任何排队都不可能超时 |
| C-3 | `agent server` 前台常驻，父 shell 结束即随会话退出 | 没有「派生即返回」的入口，本地起服务必须占一个终端 |
| C-4 | 服务端子进程的指标只存在于 `metrics` 特性的**阈值日志**里（`manager-agent`、`agent-runtime`、`lib-db` 三处 `warn!` / `debug!`） | `bin-tests` 读不到子进程的内存指标；想要数据只能翻 `server.log` |
| C-5 | `util/observability.rs` 的「观测数据」一节只覆盖**测试进程内**装配的运行时（`manager_agent` holder + `lib-bin-tui` 的阻塞计数） | 共享 / 隔离用例的服务端在子进程里，报告里没有它们的数字 |
| C-6 | `cases/llm/canary.rs`、`cases/persist.rs`（2 处）、`cases/run.rs`（1 处）用 `read_to_string(server.log)` 做断言 | 断言依赖日志文本与行数：日志格式一改就失效，且违反「数据不从日志来」 |
| C-7 | `agent-runtime` 的限流快照（`queued` / `queued_max` / `running_max` / `wait_max_millis` / `timeouts`）已实现 | 没有任何出口把它读出去；`manager-agent` 的快照里没有这一段 |
| C-8 | `lib-web` 已有 debug-only 探针先例（`api/probe/shutdown`，`#[cfg(debug_assertions)]`） | 没有「快照探针」，也没有 `metrics` 与接口层的特性链 |

---

## 四、技术方案

### 4.1 边界压力用例（G-1）

**核心思想**：把服务端压成「一次只跑一个 Run」的串行机器，让排队时长变成可由单次耗时线性推算的量；再用「排队超时」当作容量门槛，把客户端数量精确地摆在不失败的那一格上。多于这个数字必然失败，少于它必然安全——用例因此既是压力测试，也是并发语义的回归哨兵。

**用例自带配置**（写进用例自己的 `AGENT_HOME/config.toml`）：

```toml
[agent]
# 唯一可配置的并行度参数：压到 1，使 Run 执行完全串行、排队时长可线性推算。
max_concurrent_runs = 1
# 用例的等待预算 T：等许可超过它就落 FAILED「执行排队超时」。
run_queue_timeout_ms = 3000
```

`[provider]` 指向**用例私有**的 `MockServer`（不复用进程级共享实例：节奏与 `/mock/stats` 观测必须私有）。DB 池与下行通道保持默认，不可配置。

**三个阶段**：

| 阶段 | 动作 | 用途 |
|---|---|---|
| ① 标定 | 1 个客户端跑 1 次完整链路（注册 → 会话 → 占用 → 流 → 发消息 → 终态），重复 3 次，取**最大**值记为 `L0` | 得到「串行执行一个 Run 的端到端耗时」；取最大值是保守估计，不是平均值 |
| ② 内边界 | `N = floor(T / L0) + 1` 个客户端（各自 1 会话 1 占用 1 Run）同时发起 | 该 `N` 是「理论上不会失败的最大客户端数」，见下方推导 |
| ③ 外边界 | `2N` 个客户端同时发起 | 证明门槛真实存在：必然有 Run 等不到许可而落 `FAILED`「执行排队超时」 |

**内边界公式的推导**（写进用例注释与报告，避免「魔法数字」）：第 `k` 个 Run 在全局许可上等待约 `(k-1) × L0`；排队超时只看**等许可**这一段，因此它超时当且仅当 `(k-1) × L0 > T`，即 `k > T / L0 + 1`。所以：

```text
不会失败的最大客户端数  N_safe = floor(T / L0) + 1
第一个必然失败的客户端数 N_bad  = floor(T / L0) + 2
```

`N` 由标定得出，**代码里不出现任何固化的客户端数量**（示例值只允许出现在注释与文档里）。若 `L0` 大到 `N < 8`，说明本机参数不足以形成有意义的边界，用例直接失败并点明原因（缺参数，不是缺功能）。

**断言**（全部来自内存数据，不读日志）：

| 断言 | 来源 |
|---|---|
| ②全部 `RUN_COMPLETED`，无 `RUN_FAILED` | 客户端收的 `RunEvent` |
| ②从首个 `message/create` 到最后一个终态 ≤ `T` | 测试进程内计时 |
| ②`running_max == 1` 且 `queued_max >= N - 1` | 服务端快照（`limit` 段） |
| ②`limit.timeouts == 0` | 服务端快照 |
| ③至少一条 `FAILED` 且原因为「执行排队超时」 | 客户端收到的 `RunEvent.error` / `run/get` 状态 |

**报告内容**：`T`、`L0`、`N`、`2N`、②实测总耗时与余量（`T - 实测`）、单次耗时分布（min / 中位 / p95 / max）、③的超时条数与首批超时时刻、服务端快照（第二、三阶段各取一次）。

### 4.2 `agent server --async`（G-2）

```bash
agent server --async            # 或 -a
# → PID 12345  地址 http://127.0.0.1:20102
# → 日志 <AGENT_HOME>/logs-server
```

- **参数**：`ServerArgs` 增加 `#[arg(short = 'a', long = "async")]`，与 `--addr` / `--port` / `--log-level` 任意组合；不新增配置文件字段。
- **实现位置**：`lib-bin-cli`（配置与派生逻辑都归它，`bin-agent` 只调用并决定退出码）。
- **派生方式**：重新执行当前可执行文件，参数由已解析的 `ServerConfig` 反向构造（**不带** `--async`）；`stdin` 置空，`stdout` / `stderr` 重定向到 `<AGENT_HOME>/logs-server/stdout.log`；父进程不等待、不 `kill_on_drop`。
- **脱钩标志**：Windows 用 `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`（`std::os::windows::process::CommandExt`）；Unix 只做 stdio 重定向并在帮助文案与文档里写明「终端关闭可能收到 SIGHUP」。
- **就绪语义**：派生后轮询已有探活接口（复用 `lib_bin_cli::client::Client::probe_health`），就绪即打印 `PID` / 地址 / 日志目录并退出 `0`；超时或子进程立即退出则打印一行原因并退出非 0，同时打印子进程 PID 便于人工回收。
- **PID 文件**：写 `<AGENT_HOME>/server.pid`（含 pid 与地址），供人工与 `cli.async_server` 用例结束进程；不提供 `--stop` 子命令（D-P2.2-004）。
- **前台零变化**：不带 `--async` 时行为与现在完全一致（日志初始化、装配、优雅关闭链路都不动）。

### 4.3 测试数据通道：特性 + 内存快照 + 结构化出口（G-3）

```text
服务端子进程（metrics 构建）
  manager-agent / agent-runtime / lib-db / lib-core(audit)  →  各自 Atomic 累加
                                        ↓ snapshot()（纯内存读）
        manager-agent::runtime::metrics::Snapshot（唯一聚合 DTO，serde）
                                        ↓
        GET api/probe/metrics（debug + metrics 双限定，release 与普通构建不存在）
                                        ↓ JSON
bin-tests（manager-agent/metrics）→ 反序列化成同一个 Snapshot 类型 → 报告
```

要点：

1. **一个 DTO**：`Snapshot` 聚合 `chain` / `stop` / `journal` / 下行 / 锁 / 注册表 / `database`（已有）+ `limit`（新增，转写 `agent-runtime`）+ `audit`（新增，转写 `lib-core`）。子进程与测试进程读的是同一个类型，`bin-tests` **不重复定义 DTO**（`AGENTS.md` 第 19~22 条）。
2. **不新增跨层依赖**：`manager-agent` 不依赖 `agent-runtime`；由装配层（`lib-bin-web`，已同时依赖两者）在启动时把 `agent_runtime::metrics::snapshot` 作为函数指针注册进 `manager-agent` 的指标模块。`lib-web` 只依赖 `manager-agent`。
3. **出口只在一个地方**：`GET api/probe/metrics`，`#[cfg(all(debug_assertions, feature = "metrics"))]`，匿名可访问（与 `api/probe/shutdown` 同规格）。特性链：`bin-agent/metrics → lib-bin-web/metrics → {lib-web/metrics, manager-agent/metrics, agent-runtime/metrics, lib-db/metrics} → lib-core/metrics`。
4. **审计层**（替换三处日志检索）：`lib-core` 在 `metrics` 特性下往 subscriber 挂一个 Layer，做三件事——按级别计数、统计「事件中是否出现 `[provider] api_key`」（敏感串命中数）、统计 panic（`std::panic::set_hook`）。热路径要求：先比较、命中才构造；字段值用访问器比对，不为每条事件分配字符串。挂载点需要 `framework-core` 的 `LoggingConfig` 暴露 `layers`（D-P2.2-002）。
5. **DB 错误计数**：`lib-db` 在 `metrics` 下累加「`database is locked` / `SQLITE_BUSY`」出现次数，供 `run.*` 的断言替换日志检索。
6. **缺失即显式**：拿不到快照（子进程崩溃、构建未开特性、接口 404）时，报告写「未采集：原因」，用例断言本身**失败**而不是静默放过。

### 4.4 报告收敛

- 观测数据只有一个渲染点（`crates/bin-tests/src/util/observability.rs`）：入参是本进程快照 + 各用例的服务端快照，出参是 Markdown。
- `CaseReport` 增加结构化字段（服务端快照 + 用例自带的度量条目），用例章节里的「运行数据」由它渲染。
- 每个用例结束前（服务端仍存活时）取一次快照；取不到就记「未采集」，不猜、不翻日志。
- 报告里的日志路径保留，但只用于人工排查。

### 4.5 日志与数据的界线

| 类别 | 处置 |
|---|---|
| 为采集而打的阈值日志（`metrics` 特性下 `warn!` / `debug!`） | **全部删除**：`manager-agent/src/runtime/metrics.rs`、`agent-runtime/src/metrics.rs`、`lib-db/src/observability.rs` |
| 运行期流程日志（带 `run_id` / `session_id` 的装配、关闭、失败） | 保留；它们不进报告，也不是数据来源 |
| 用例读取日志文本 | **全部删除**（`read_to_string(server.log)` 三处用例、四处断言点） |
| 日志文件本身 | 保留；失败时报告给出路径供人工排查 |

---

## 五、批次与任务

批次之间按序；同批次内可并行。每批结束时必须保持可编译、可运行。

| 批次 | 主题 | 任务 | 出口 |
|---|---|---|---|
| A | 数据通道：快照聚合与出口 | P2.2-001 ~ P2.2-004 | 子进程指标可从结构化出口取到；为采集而打的阈值日志清零 |
| B | 数据通道：采集与报告收敛 | P2.2-005 ~ P2.2-007 | `bin-tests` 不读日志文本；报告由快照渲染；四个断言点等价替换 |
| C | 边界压力用例 | P2.2-008 ~ P2.2-010 | `stress.boundary` 三阶段可跑，连续 3 次全绿，数据全来自快照 |
| D | `server --async` | P2.2-011 ~ P2.2-013 | 派生即返回、子进程常驻、失败路径可读；前台零变化 |
| E | 纪律入库与验收 | P2.2-014 ~ P2.2-016 | `AGENTS.md` 条款落地；判据有证据；阶段报告出稿 |

```text
A ──► B ──► C ──┐
                ├──► E
D ──────────────┘
```

- 强序：A → B（B 依赖 A 的出口）→ C（C 的证据全部来自快照）。
- 可并行：D 与 A / B / C 无交集。
- E 的硬前置是 B 与 C：条款要照着已落地的实现写。

任务清单见 [tasks.md](tasks.md)。

---

## 六、判据

### G-1 边界压力用例

| 编号 | 判据 |
|---|---|
| **G-1-1** | `bin-tests --list` 可查 `stress.boundary`；`make test ARGS="stress"` 可单跑 |
| **G-1-2** | 用例自带服务端配置里 `max_concurrent_runs = 1`、`run_queue_timeout_ms = 3000`，且报告打印生效配置 |
| **G-1-3** | 客户端数量由标定得出：报告给出 `L0`、`T`、`N = floor(T / L0) + 1` 与推导过程；代码中不存在固化的客户端数量（可核） |
| **G-1-4** | ②阶段全部成功且总耗时 ≤ `T`；`N < 8` 时用例以「参数不足以形成边界」失败并点明原因 |
| **G-1-5** | ③阶段（`2N`）稳定出现 `FAILED`「执行排队超时」，超时条数写入报告 |
| **G-1-6** | 证据全部来自内存：客户端事件 + 测试进程计时 + 服务端快照；用例不读日志、不读 `server.log` |
| **G-1-7** | 用例是 `Isolated`（私有服务端 + 私有 Mock），不占用共享服务端；连续 3 次同一配置全绿 |
| **G-1-8** | 默认并发下 `--all` 仍全绿（回归判据，不是压力收益的证据） |

### G-2 `server --async`

| 编号 | 判据 |
|---|---|
| **G-2-1** | `agent server --help` 含 `-a, --async`；与 `--addr` / `--port` / `--log-level` 可组合 |
| **G-2-2** | `agent server --async` 在就绪后退出 `0`，输出含 `PID`、服务地址、日志目录；此后 `api/probe/health` 与 `agent status` 均可用 |
| **G-2-3** | 父进程退出后子进程仍在运行（父 shell 结束、父进程退出后各验一次），且父进程不残留 |
| **G-2-4** | 失败路径可读：端口被占用 / 子进程秒退时退出码非 0，输出一行原因并给出子进程 PID |
| **G-2-5** | 不带 `--async` 时行为零变化（前台常驻、优雅关闭、日志与装配链路不动） |
| **G-2-6** | 无新增配置文件字段、无新增接口、无新增第三方依赖；`git diff` 可核 |

### G-3 测试数据通道收敛

| 编号 | 判据 |
|---|---|
| **G-3-1** | 测试代码中 `read_to_string(…server.log)` / 日志目录扫描 / 行数统计归零（`crates/bin-tests` 全量检索可核） |
| **G-3-2** | 服务端子进程数据只经 `GET api/probe/metrics` 获取；返回 DTO 与 `bin-tests` 反序列化用的是同一个类型 |
| **G-3-3** | 为采集而打的阈值日志全部删除；`metrics` 特性下不再有 `warn!` / `debug!` 采集分支 |
| **G-3-4** | 非 `metrics` 构建无采集、无接口、无额外原子操作；`cargo check -p bin-agent` 与 `make build` 通过 |
| **G-3-5** | 原四处日志断言等价替换：`llm.canary`（敏感串命中 0）、`persist.*`（panic 0、ERROR 0）、`run.*`（`SQLITE_BUSY` 0），失败信息点名字段 |
| **G-3-6** | 报告收敛：观测数据只有一个渲染点；每个用例的服务端快照出现在该用例章节；缺失时显式「未采集」 |
| **G-3-7** | 正式构建零代价：`make release` 产物的接口清单不含 `api/probe/metrics`，日志中不出现采集相关事件 |

### G-4 纪律入库

| 编号 | 判据 |
|---|---|
| **G-4-1** | `AGENTS.md` 的「测试」相关章节新增「测试数据采集」条款，含：禁止日志采集、只允许特性 + 内存采集 + 结构化快照、敏感检测在写出侧、缺失显式报「未采集」、日志仅用于人工排查 |
| **G-4-2** | 条款可逐条对照实现：G-3-1 ~ G-3-7 的实现方式与条款无冲突；条款里不出现「目前不做」的例外（有例外就写进 `decisions.md`） |

---

## 七、已知限制与风险

1. **边界用例与机器性能强相关**：`L0` 随 CPU、磁盘、杀软、SQLite 页缓存漂移，`N` 必须每次标定；把 `N` 写死成常数会立刻变成「今天过、明天红」。报告必须同时给出 `L0` 与余量，读者才知道这条用例当时有多紧。
2. **它是边缘用例，天生比别的用例脆**：`N` 恰好等于「不会失败的最大值」，任何一次抖动都可能让最后一位排队超时。缓解手段只有三条——`L0` 取 3 次最大值、`N` 由公式算而非估计、报告给出余量；**不允许**用「减一减二」的方式偷偷放水（那会让用例失去门槛意义）。
3. **`2N` 阶段会制造真实失败**：服务端会写 `FAILED` 与终态事件。用例必须断言「失败原因就是排队超时」，否则无法区分「边界真实」与「服务端有其他缺陷」。
4. **300 级并发会同时压到三处**：测试进程（300 条 SSE + 300 条 HTTP 连接）、SQLite（单写者）、事件文件（300 个目录各若干帧）。边界可能被 DB 而不是 Run 许可决定——报告必须给出快照里的取连接等待与事件落盘耗时，便于归因；本阶段**不为此改任何默认值**。
5. **快照出口依赖 debug + `metrics` 双条件**：用 `make build` 的产物跑 `stress` 会拿不到数据。用例必须在缺失时明确失败并提示用 `make test`，不能静默「跳过断言」。
6. **审计层的开销**：即使只在 `metrics` 构建下，它也在每条事件上做一次字段比对。必须做到「先比较、命中才构造」，并验证 `--all` 墙钟与 P2.1 基线同阶。
7. **框架改动是外部依赖**：`LoggingConfig::layers` 要落在本地框架仓库（`lingting-rust-framework`），与主仓库的合并顺序需要一起确认；如果暂不允许改框架，canary 用例只能退化为「删除该断言」（见 D-P2.2-002 的备选，需重新决策）。
8. **`--async` 的孤儿进程**：没有 `--stop` 时，用户只能靠 PID 文件或任务管理器收尾。本阶段只保证「不乱杀、不留守护进程」，不承诺进程治理。
9. **`--async` 的 Unix 语义弱于 Windows**：没有 `setsid` 时终端关闭可能带回 SIGHUP。已在帮助文案与文档写明；需要更强语义时再引入依赖（D-P2.2-005）。
10. **报告口径变化会改变历史可比性**：P2.1 的报告数字来自日志阈值行，本阶段之后同一指标改由快照给出。跨阶段对比时必须注明来源不同，不能直接相减。
11. **测试执行受授权约束**：G-1-7 / G-1-8 / G-2 的实测条目、任何 `make test` 都需要明确授权；未授权时本阶段只推进代码与文档部分，相关判据保持未勾选。

---

## 八、待确认项

| 编号 | 事项 | 状态 |
|---|---|---|
| D-P2.2-001 | 快照出口：debug 探针端点 / 关闭时写 JSON / 两者并存 | 倾向端点，见 [decisions.md](decisions.md) |
| D-P2.2-002 | 是否允许为审计层给 `framework-core` 的 `LoggingConfig` 增加 `layers` | 倾向允许 |
| D-P2.2-003 | `stress.boundary` 是否默认纳入 `--all` | 倾向纳入 |
| D-P2.2-004 | `--async` 是否提供 `--stop` / PID 文件语义 | 倾向只写 PID 文件 |
| D-P2.2-005 | Unix 是否为 `setsid` 引入 `libc` | 倾向不引入 |
| D-P2.2-006 | canary 用例的保留口径（内存审计 / 删除） | 倾向内存审计 |
| D-P2.2-007 | 边界外阶段（`2N`）是否默认启用 | 倾向默认启用 |
| D-P2.2-008 | `docs/*` 与 `.steps/README.md` 的同步授权与时机 | 待确认 |

各项的候选方案与理由见 [decisions.md](decisions.md)。
