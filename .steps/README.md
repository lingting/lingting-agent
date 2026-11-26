# Personal Agent 开发路线图
本目录是本项目的分阶段开发路线图。**P1 已完成，P1.1 已完成，P2 已完成（Batch A ~ H；15 类验收全部通过，报告见 [`reports/p2-032-acceptance.md`](../docs/reports/p2-032-acceptance.md)），P2.1 已完成（Batch A ~ E；报告见 [`reports/p2.1-014-acceptance.md`](../docs/reports/p2.1-014-acceptance.md)），P2.2 已完成（16 个任务 / 5 个批次，报告见 [`reports/p2.2-016-acceptance.md`](../docs/reports/p2.2-016-acceptance.md)），P3 已完成（32 个任务 / 8 个批次，Batch A ~ H 全部结案，报告见 [`reports/p3-032-acceptance.md`](../docs/reports/p3-032-acceptance.md)；唯一「部分取证」项是 C-3 的批量确认点用例），P4 已细化（20 个任务 / 5 个批次，23 项决策全部定案，待开工），P5–P6 只做阶段级规划。**

本次路线图不包含任何代码实现。Cargo Workspace 与 Crate 结构以 `docs/workspace.md` 为准，本文档不重复记录；前端工程结构以 `docs/develop.md` 的「前端工程」一节为准。

---

## 一、项目目标

开发一个用 Rust 实现的 **Personal Agent**，不是 ChatGPT Wrapper，而是一个可独立安装、可单体部署、可扩展的 **Agent Runtime**。

核心设计原则：

1. **Native Binary 分发**：最终产物是一到多个可执行文件，不要求用户预装 JVM / Python Runtime。
2. **Modular Monolith**：单进程服务端，不提前拆微服务。
3. **Server 是唯一业务逻辑执行方**：Session、Message、Run、Context、Tool Execution 只由 Server 管理。
4. **CLI / TUI / WebUI 都是客户端**：不含 Agent Runtime 逻辑，不直接调用 LLM Provider，不执行 Tool。
5. **统一 API 契约**：前端与 CLI 使用由服务端 API 定义生成的 SDK，客户端只允许自实现 HTTP Transport，不允许自维护 API 定义。
6. **Agent Runtime 自研**：Rig 只承担 LLM Provider / Completion 等底层能力，Agent Loop、Context、Run 生命周期由项目实现。
7. **第三方协议通过 Adapter 接入**：Rig、MCP、LLM 供应商协议不得污染 Agent Core。
8. **渐进演进**：Tool → MCP → Skills → Memory / Workflow / 生产级可靠性。

---

## 二、技术方向

| 领域 | 选型 | 阶段 |
|------|------|------|
| 语言 | Rust（edition 2024，当前工具链 rustc 1.98.0） | P1 |
| 异步运行时 | Tokio | P1 |
| HTTP Server | Axum（经 `framework-web-axum` 适配层） | P1 |
| 服务端 API 契约 | `framework-proc-web` + `framework-proc-ts` + `framework-proc-core` | P1 |
| TS SDK 生成 | `framework-proc-core` 的 `TypescriptApiBuilder` / `PackageBuilder` | P1 |
| Rust SDK 生成 | 新增 `framework-proc-rust`（本路线图确定的方向） | P1 |
| HTTP Client | Reqwest | P1 |
| CLI | Clap | P1 |
| TUI | Ratatui | P1 |
| WebUI | TypeScript + Vite（框架待定，见 `p1/decisions.md`） | P1 |
| Serialize | Serde | P1 |
| 日志与追踪 | logging 经 `framework-core`，后续接 tracing / OpenTelemetry | P1 / P6 |
| LLM Provider 抽象 | `rig-core`（不用 `rig` facade 的 `agent` 特性，见 `p2/decisions.md` D-P2-001） | P2 |
| 持久化 | SQLite（`sqlx`，见 `p2/decisions.md` D-P2-002 / D-P2-003） | P2 |
| MCP | rmcp | P4 |

> 说明：`framework-*` 系列 crate（`framework-proc-web` / `framework-proc-ts` / `framework-proc-core` / `framework-web` / `framework-web-axum`）是作者自有框架，源码位于本地仓库 `D:/code/lingting/lingting-rust-framework`，workspace 版本 26.11.0。P1 中允许为满足 SDK 生成与优雅关闭需求向该框架提交改动。

---

## 三、阶段总览

| 阶段 | 目标 | 核心产出 | 文档 |
|------|------|----------|------|
| **P1** | 基础工程架构 | 单 Binary 双运行模式、Server 基础能力、统一 API 契约、TS/Rust 双 SDK 生成、TUI/WebUI 最小闭环（Mock） | [p1/README.md](p1/README.md) |
| **P1.1** | 客户端连接与会话运行时 | 客户端注册/心跳/问询、断连处理链、显式会话占用、原始事件文件与轮转、移除 `api/run/stream`、TUI/WebUI 接入 | [p1.1/README.md](p1.1/README.md)（已细化：`tasks.md` / `acceptance.md` / `decisions.md`） |
| **P2** | 基础 Agent Runtime | Rig Provider Adapter、Agent Loop、Context、Streaming（经 P1.1 事件通路）、SQLite 持久化、Run 生命周期与取消 | [p2/README.md](p2/README.md)（32 个任务 / 8 个批次，已完成） |
| **P2.1** | 共享服务端并发根因修复与测试基础设施整理 | 结构性并发缺陷修复（测试侧同步阻塞、全局断连链、锁内收尾、锁表回收）、`bin-tests` 的 `cases` / `mock` / `util` 分层、`max_concurrent_runs` 支持核数倍数 | [p2.1/README.md](p2.1/README.md)（15 个任务 / 5 个批次，已完成） |
| **P2.2** | 边界压力用例、后台服务端与测试数据通道收敛 | `stress.boundary`（按 `max_concurrent_runs = 1` 与排队超时算出的容量边界）、`agent server -a/--async` 派生常驻进程、测试数据从日志改为 `metrics` 特性下的进程内快照与结构化出口、`AGENTS.md` 测试数据采集条款 | [p2.2/README.md](p2.2/README.md)（16 个任务 / 5 个批次，已完成） |
| **P3** | Tool System | Tool Trait / Registry / Executor、Tool Calling Loop、权限与审计、并行调用、5 个内置工具 | [p3/README.md](p3/README.md)（32 个任务 / 8 个批次，已完成；报告见 [p3-032-acceptance.md](../docs/reports/p3-032-acceptance.md)） |
| **P4** | MCP Integration | 基于 rmcp 的 MCP Client Manager、Server Registry、工具发现与适配 | [p4/README.md](p4/README.md)（已细化：`tasks.md` / `acceptance.md` / `decisions.md`） |
| **P5** | Skills | Skill 发现与加载、SKILL.md、渐进披露、Skill 与 Tool 协作 | [p5/README.md](p5/README.md) |
| **P6** | Production Readiness | Checkpoint/Resume、Memory、压缩与 RAG、Multi-Agent、Workflow、Otel、评测、安全沙箱、跨平台发布 | [p6/README.md](p6/README.md) |

### 阶段依赖

```text
P1 基础工程架构
   |  （单 Binary、API 契约、SDK、客户端闭环）
   v
P1.1 客户端连接与会话运行时
   |  （客户端注册/心跳、会话占用、事件文件为唯一事实源）
   v
P2 基础 Agent Runtime
   |  （可真实调用 LLM、Streaming、持久化）
   v
P2.2 边界压力用例与服务端启动入口
   |  （不改业务语义与接口：补压力用例、后台启动、测试数据通道）
   v
P3 Tool System
   |  （统一工具抽象与执行）
   v
P4 MCP Integration
   |  （MCP 工具接入统一 Tool System）
   v
P5 Skills
   |  （任务知识与流程，组合 Tool 使用）
   v
P6 Production Readiness
      （可靠性、记忆、多 Agent、评测、发布）
```

依赖关系是**强序**的：P1.1 插入在 P1 与 P2 之间（改动 Run 触发点与事件通道契约）；P2 依赖 P1.1 的客户端连接与会话运行时；P2.1 插入在 P2 与 P3 之间（共享服务端并发根因修复与测试基础设施整理，不改业务语义与接口）；P2.2 紧随 P2.1（边界压力用例、`server --async`、测试数据通道收敛，同样不改业务语义与接口），其 G-1 的数据证据依赖自身的快照通道；P3 依赖 P2 的 Agent Loop；P4 依赖 P3 的 Tool Registry；P5 依赖 P3/P4 的能力目录；P6 依赖前五阶段的全部可观察点。阶段内部允许并行，阶段之间不并行。

---

## 四、当前开发进度

| 项 | 状态 |
|----|------|
| 仓库状态 | Cargo Workspace + 17 个成员 crate（`bin-agent` + `bin-tests` + `lib-*` + `types-core` + `service-core` + `manager-agent` + `lib-db` + `agent-core` / `agent-context-core` / `agent-context` / `agent-route` / `agent-runtime`，见 `docs/workspace.md` §8.1）；服务端仍具备 25 条接口（探针 12 条，其中关闭探针仅 debug 构建；客户端 4 条、会话占用 3 条；会话 / 消息 / Run 6 条），服务端推送只有 `api/client/stream` 一条 SSE 通道，另有统一错误契约 `ApiError`、trace id、CORS、静态资源托管与优雅关闭；会话数据落 SQLite（`<AGENT_HOME>/data/agent.db`），执行由 `agent-runtime` 经 `agent-route` 真实调用 OpenAI 兼容 Provider；TUI 与 WebUI 的对话闭环已切到真实执行并展示排队 / 失败 / 取消态，WebUI 产物嵌入 release binary；前端工程在 `lingting-agent-ui/`（pnpm workspace 根） |
| Rust 工具链 | rustc 1.98.0 / cargo 1.98.0 |
| Node 工具链 | node v22.22.3 / pnpm 11.24.0 / npm 10.9.8 |
| framework 版本 | 本地源码 26.11.0（含未发布的 Rust 导出改动）；**P1-003 定稿：全部走 `path` 引入，暂不带 `version` 约束**；本地框架已删除 `framework-proc-ts`，TS/Rust 导出并入 `framework-proc-core` 的 `ts` / `rust` 特性，见 `p1/decisions.md` D-015 回填 |
| SDK 生成 | `make gen` 一条命令产出两份 SDK，产物落 `packages/sdk-rust`（`#[path]` 引入 `lib-bin-cli`）与 `packages/sdk-ts`（由 `lingting-agent-ui/` 内的 pnpm workspace 消费），均不入库（D-006 已修订）；**生成必须在 `make build` / `make ui-install` 之前执行** |
| P1 | **已完成**：P1-001 ~ P1-020 全部完成并通过验收（九类条目全部通过，结论与证据见 [`reports/p1-020-acceptance.md`](../docs/reports/p1-020-acceptance.md)）；判据修订（A-1、E-3、E-4）见 `p1/decisions.md` D-032。下一步：P2 细化前先读该报告的「P2 输入清单」 |
| P1.1 | **已完成**：22 个任务（7 个批次）全部完成，21 条判据全部通过，结论与证据见 [`reports/p1.1-022-acceptance.md`](../docs/reports/p1.1-022-acceptance.md)；本阶段移除了 `api/run/stream`，服务端推送只剩 `api/client/stream`，Run 触发点改为「会话被占用 + 消息创建」，并新增客户端注册 / 心跳 / 问询、会话占用、事件文件与轮转 |
| P2 | **已完成（2026-11-21）**：`p2/`（32 个任务、8 个批次、15 类验收共 92 条、32 项决策 + 5 项 OPEN，其中 O-1 / O-3 / O-4 已结案）按 P1.1 的客户端连接与会话运行时模型重写——Run 触发点继承 P1.1，P2 只把「产出 Mock 事件」换成真实 Agent Loop（D-P2-028）。Batch A ~ H 全部完成，15 类判据全部通过（A-4 / P-7、Q-3、S-1 三条按实现口径修订），结论与证据见 [`reports/p2-032-acceptance.md`](../docs/reports/p2-032-acceptance.md)；本轮补齐了 `run.execute` / `persist.running_heal` / `llm.canary` 三个此前缺失的用例 |
| P2.1 | **已完成**：15 个任务 / 5 个批次。K-1 在默认并发与五档加压下均未复现，G-1 口径改为「结构修复 + 指标对照」（D-P2.1-007）；Batch B 修掉四处结构缺陷（测试侧同步阻塞 4752ms → 0ms、全局断连链收窄为按客户端、收尾等待移出会话锁、会话锁表 6 条 → 0 条），H-3 复核结论为「全局 Run 许可与 journal 都不是瓶颈」；Batch C 把 Mock 替身与通用支持移出 `cases/`（`mock/llm` + `util/`，D-P2.1-013）；Batch D 八领域分批全绿 + 默认并发三轮全绿；Batch E 让 `max_concurrent_runs` 支持核数倍数（默认 `"3C"`），显式 4 与默认 3C 两轮全量全绿（D-P2.1-015，报告见 [`p2.1-014-acceptance.md`](../docs/reports/p2.1-014-acceptance.md)）。任务与判据见 [`p2.1/`](p2.1/README.md) |
| P2.2 | **已完成（2026-11-24）**：16 个任务 / 5 个批次，判据 4 组 23 条（G-1 边界压力用例 8 条、G-2 `server --async` 6 条、G-3 测试数据通道 7 条、G-4 纪律入库 2 条）全部成立，报告见 [`p2.2-016-acceptance.md`](../docs/reports/p2.2-016-acceptance.md)。落地要点：`stress.boundary` 按 `max_concurrent_runs = 1` 与 `run_queue_timeout_ms = 3000` 每次标定 `L0` 并算出 `N`（本轮 51 ~ 54），② 全绿、③`2N` 稳定出现 44 ~ 52 条排队超时；`agent server -a/--async` 派生常驻进程后父进程退出（PID 文件 + `logs/stdout.log`）；测试数据改由 `metrics` 特性下的进程内快照经 `GET api/probe/metrics` 输出，阈值日志与日志检索全部移除；`AGENTS.md` 新增「测试数据采集」条款。`--all` 连续 5 次 75/75（36s，P2.1 基线 35s 同阶）。决策 16 项全部定案（D-P2.2-008 在本轮由需求方授权后定案）；遗留 KI-P2.2-003（`chat.multiturn` 的 Mock 观测偶发 404，与本阶段无关） |
| P3 | **已完成（Batch A ~ H 全部结案）**：32 个任务 / 8 个批次，判据 11 组（B / A / S / L / X / C / P / F / T / W / R），决策 28 项**全部定案**，验收报告见 [p3-032-acceptance.md](../docs/reports/p3-032-acceptance.md)（唯一「部分取证」项是 C-3 的批量确认点用例）。落地要点：新增 `agent-tool` Crate（九件套 + `builtin` 五个内置工具 —— `calculator` / `current_time` / `file_read` / `file_search` / `http_request`，由 `builtin::register_all` 在 `manager-agent` 装配期一次注册，Tool Trait 带「本地文件操作」声明并据此推导并发默认值）、`agent-runtime` 多轮工具循环 + **每轮边界落库**、工具轨迹进 `messages` 表并按「工具调用组」成对裁剪、Run 事件追加三个工具事件、`api/client/update` **改名** `api/client/post` 并承载工具确认、`confirm` 工具**挂起 Run**（按 D-P3-027 停在 `WAITING`、让出并发位、不阻塞同会话后续 Run、不设超时、重连补发）、迁移 `0002` 与 1 条查询接口 `GET api/tool/audit/list`（接口总数 25 → 26，不含 debug + `metrics` 限定的 `api/probe/metrics`）。本阶段有两处**破坏性变更**：`messages` 表与 `Message` 契约扩字段、上行接口改名。任务与批次见 [`p3/tasks.md`](p3/tasks.md)，判据见 [`p3/acceptance.md`](p3/acceptance.md)，决策见 [`p3/decisions.md`](p3/decisions.md) |
| P4 | **已细化，待开工**：20 个任务 / 5 个批次，23 项决策（D-P4-001 ~ D-P4-023）全部定案。开工前提已核实（`rmcp` 3.11.0 可解析，client / child-process / streamable-http 特性齐全）。落地要点预告：新增 `agent-mcp`（`rmcp` 唯一落点）、`Tool` 契约放宽为 `&str` + 外部 schema、`ToolRegistry` 运行期可变（含条目默认权限）、`[mcp]` 配置段（含 `file_access`：精确名 + `*` 通配，缺省 `none`，写声明恒串行）、3 条管理接口（`api/mcp/server/list` / `api/mcp/tool/list` / `api/mcp/server/update`）、`metrics` 的 `mcp` 段、关闭时终止子进程；不新增 Run 事件、不加迁移、`agent-runtime` 零改动。任务见 [`p4/tasks.md`](p4/tasks.md)，判据见 [`p4/acceptance.md`](p4/acceptance.md)，决策见 [`p4/decisions.md`](p4/decisions.md)。风险：文件访问声明是自述而非强制（不是沙箱）、Windows 孙进程不保证回收、工具数量推高 token 成本 |
| P5–P6 | 未开始，仅有阶段级规划 |

---

## 五、如何使用路线图

1. **开始 P1 前**：先读 `p1/README.md` 的阶段边界与执行顺序，再读 `p1/decisions.md` 中所有 `OPEN` 项，确认哪些必须在动手前讨论。
2. **执行任务**：按 `p1/tasks.md` 的依赖顺序取任务。任务分为若干批次（Batch），同一批次内可以并行，批次之间必须按顺序。
3. **验收**：每个阶段结束时对照该阶段的判据逐条确认（P1 / P1.1 / P2 已有 `acceptance.md`；P2.1 的判据在 `p2.1/README.md` §七，P2.2 的判据在 `p2.2/README.md` §六并已按报告 [p2.2-016-acceptance.md](../docs/reports/p2.2-016-acceptance.md) 逐条取证；P3 的判据在 `p3/acceptance.md` 并已按报告 [p3-032-acceptance.md](../docs/reports/p3-032-acceptance.md) 逐条取证；P4 的判据在 `p4/acceptance.md`（待开工）；P5–P6 暂以各自 README 中的“阶段验收方向”为准）。
4. **细化后续阶段**：P1、P1.1、P2、P2.1、P2.2、P3 已完成，P4 已按同一规则展开（`p4/tasks.md` / `acceptance.md` / `decisions.md`，待开工）；P5–P6 在前一阶段验收通过后、正式开工前才展开成 `tasks.md` / `acceptance.md` / `decisions.md`，禁止提前细化。
5. **结构设计**：Cargo Workspace、Crate 划分与依赖方向以 `docs/workspace.md` 为准；前端工程结构在 P1-016 前确认。

---

## 六、如何更新任务状态

任务状态只用 Markdown Checklist 表达，定义如下：

| 写法 | 含义 |
|------|------|
| `- [ ] P1-0xx 任务名` | 未开始 |
| `- [ ] P1-0xx 任务名（进行中）` | 进行中 |
| `- [ ] P1-0xx 任务名（阻塞：原因）` | 阻塞，原因与待决策项写在 `decisions.md` |
| `- [x] P1-0xx 任务名` | 已完成，并通过该任务“验收标准” |

规则：

- **只允许把任务标记为 `[x]` 时，其“验收标准”逐条可客观验证且已通过。** 不允许因为“代码写完了”而勾选。
- 一项任务拆分或新增任务时，直接在 `tasks.md` 插入，ID 使用 `P1-0xx` 中尚未占用的编号，并在“依赖”中回写。
- 状态变化外还需要记录的结论（选型确认、框架能力变更、踩坑限制）写入 `p1/decisions.md`，不要写进 `tasks.md`。
- `decisions.md` 中的 `OPEN` 项一旦确定，改为 `DECIDED` 并补全“决策理由”；被否决的方案保留记录，不删除。
