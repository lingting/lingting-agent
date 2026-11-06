# Personal Agent 开发路线图

本目录是本项目的分阶段开发路线图。**P1 详细到可直接执行，P2–P6 只做阶段级规划。**

本次路线图不包含任何代码实现。Cargo Workspace 与 Crate 结构以 `docs/workspace.md` 为准，本文档不重复记录；前端工程结构仍待 P1-016 前确认。

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
| LLM Provider 抽象 | Rig | P2 |
| 持久化 | SQLite | P2 |
| MCP | rmcp | P4 |

> 说明：`framework-*` 系列 crate（`framework-proc-web` / `framework-proc-ts` / `framework-proc-core` / `framework-web` / `framework-web-axum`）是作者自有框架，源码位于本地仓库 `D:/code/lingting/lingting-rust-framework`，workspace 版本 26.11.0。P1 中允许为满足 SDK 生成与优雅关闭需求向该框架提交改动。

---

## 三、阶段总览

| 阶段 | 目标 | 核心产出 | 文档 |
|------|------|----------|------|
| **P1** | 基础工程架构 | 单 Binary 双运行模式、Server 基础能力、统一 API 契约、TS/Rust 双 SDK 生成、TUI/WebUI 最小闭环（Mock） | [p1/README.md](p1/README.md) |
| **P2** | 基础 Agent Runtime | Rig Provider Adapter、Agent Loop、Session/Message/Context、Streaming、SQLite 持久化、Run 生命周期与取消 | [p2/README.md](p2/README.md) |
| **P3** | Tool System | Tool Trait / Registry / Executor、Tool Calling Loop、权限与审计、并行调用、5 个内置工具 | [p3/README.md](p3/README.md) |
| **P4** | MCP Integration | 基于 rmcp 的 MCP Client Manager、Server Registry、工具发现与适配 | [p4/README.md](p4/README.md) |
| **P5** | Skills | Skill 发现与加载、SKILL.md、渐进披露、Skill 与 Tool 协作 | [p5/README.md](p5/README.md) |
| **P6** | Production Readiness | Checkpoint/Resume、Memory、压缩与 RAG、Multi-Agent、Workflow、Otel、评测、安全沙箱、跨平台发布 | [p6/README.md](p6/README.md) |

### 阶段依赖

```text
P1 基础工程架构
   |  （单 Binary、API 契约、SDK、客户端闭环）
   v
P2 基础 Agent Runtime
   |  （可真实调用 LLM、Streaming、持久化）
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

依赖关系是**强序**的：P2 依赖 P1 的 Run / Streaming API 契约；P3 依赖 P2 的 Agent Loop；P4 依赖 P3 的 Tool Registry；P5 依赖 P3/P4 的能力目录；P6 依赖前五阶段的全部可观察点。阶段内部允许并行，阶段之间不并行。

---

## 四、当前开发进度

| 项 | 状态 |
|----|------|
| 仓库状态 | Cargo Workspace + 9 个成员 crate 骨架（`bin-agent` + `lib-*` + `types-core` + `service-core`）；已有探针 API 服务与 TS / Rust 两份 SDK 生成链路；前端工程与业务代码尚未开始 |
| Rust 工具链 | rustc 1.98.0 / cargo 1.98.0 |
| Node 工具链 | node v22.22.3 / pnpm 11.24.0 / npm 10.9.8 |
| framework 版本 | 本地源码 26.11.0（含未发布的 Rust 导出改动）；**P1-003 定稿：全部走 `path` 引入，暂不带 `version` 约束**；本地框架已删除 `framework-proc-ts`，TS/Rust 导出并入 `framework-proc-core` 的 `ts` / `rust` 特性，见 `p1/decisions.md` D-015 回填 |
| SDK 生成 | `scripts/gen.sh` 一条命令产出两份 SDK，产物落 `packages/sdk-rust`（`#[path]` 引入 `lib-bin-cli`）与 `packages/sdk-ts`（pnpm workspace 消费），均不入库（D-006 已修订）；**生成必须在 `cargo build` / `pnpm install` 之前执行** |
| P1 | 进行中：P1-001 ~ P1-004、P1-006 已完成；P1-005 进行中（仅差 `tsc --noEmit` 校验）；下一个 P1-007（API 契约单一来源与生成流程固化） |
| P2–P6 | 未开始，仅有阶段级规划 |

---

## 五、如何使用路线图

1. **开始 P1 前**：先读 `p1/README.md` 的阶段边界与执行顺序，再读 `p1/decisions.md` 中所有 `OPEN` 项，确认哪些必须在动手前讨论。
2. **执行任务**：按 `p1/tasks.md` 的依赖顺序取任务。任务分为若干批次（Batch），同一批次内可以并行，批次之间必须按顺序。
3. **验收**：每个阶段结束时对照该阶段的 `acceptance.md`（P2–P6 为 README 中的“阶段验收方向”）逐条确认。
4. **细化后续阶段**：P2 在 P1 验收通过后、正式开工前才展开成 `tasks.md` / `acceptance.md` / `decisions.md`。P3–P6 同理，禁止提前细化。
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
