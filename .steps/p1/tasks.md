# P1 任务清单

状态维护规则见 `.steps/README.md` 第六节。**批次内可并行，批次间必须按序。**

执行前请先阅读 `decisions.md` 中的 `OPEN` 项；凡任务依赖未决决策的，先完成决策再动手。

---

## Batch A：工程基线与契约可行性

- [x] **P1-001 仓库与工具链勘察**

  - **目标**：在写任何代码之前，把“当前仓库里到底有什么”和“工具链能做什么”变成书面结论，避免盲目覆盖或误判版本。
  - **具体工作**：
    1. 确认仓库状态（当前仅文档：`docs/`、`.steps/`、根目录 `README.md` / `AGENTS.md`；无源码、无 Cargo 文件、无 `.gitignore`），记录远程、分支与需要保留的历史信息。
    2. 记录工具链实测版本：rustc / cargo / node / pnpm / npm；确认 edition 2024 与 `framework-*` 的 `rust-version = 1.98.0` 相匹配。
    3. 核实 `framework-proc-web`、`framework-proc-ts`、`framework-proc-core`、`framework-web`、`framework-web-axum` 的可用版本：本地源码仓库 `D:/code/lingting/lingting-rust-framework` 为 26.11.0 全链路；registry 上同样已发布 26.11.0（本机 cargo 缓存仅到 26.8.150，缓存滞后不等于未发布）。
    4. 确认各平台构建前置（Windows 本机、是否需要交叉编译工具、`cargo-zigbuild` 是否可用）。
    5. 把结论写入 `docs/reports/p1-001-environment.md`（完整勘察报告），版本矩阵的版本事实同步到 `decisions.md`，并更新 `.steps/README.md` 的“当前开发进度”。**获取方式的最终取舍与依赖锁定不在本任务内，由 P1-003 定稿。**
  - **前置依赖**：无
  - **交付物**：`docs/reports/p1-001-environment.md`；`decisions.md` 中新增/更新的版本矩阵记录；`.steps/README.md` 进度表更新。
  - **验收标准**：
    - [x] 版本矩阵包含全部 `framework-*` crate 的确定版本（registry 发布版本与本地源码版本一致）。**获取方式（registry / path）的取舍不属于本任务验收范围，归入 P1-003 定稿。**
    - [x] 明确写出 rustc/cargo/node/pnpm 的实测版本，并指出与 `rust-version = 1.98.0` 的匹配结论。
    - [x] 无任何源码或 Cargo 文件被提前创建。

- [x] **P1-002 工程骨架与工具链固定**

  - **目标**：建立可构建、可 lint、可格式化的最小工程骨架，并把工具链固定下来，保证后续所有人在同一套环境上工作。
  - **具体工作**：
    1. 创建 `rust-toolchain.toml`（固定 channel，声明 `rustfmt`、`clippy` 组件）。
    2. 创建 `.gitignore`（覆盖 `target`、`node_modules`、`.venv`、前端构建产物、本地配置文件）。
    3. 创建 `rustfmt.toml` 与 Clippy 基线配置（workspace lints）。
    4. 建立最小 `Cargo.toml`（仅到“能 `cargo build` 通过”的程度；workspace/crate 结构以 `docs/workspace.md` 为准，本任务只要求骨架可构建）。**该 `Cargo.toml` 不引入任何 `framework-*` 依赖，依赖锁定由 P1-003 负责。**
    5. 定义本地开发脚本入口（构建 / 生成 / 检查三类命令，脚本内容与命名保持一致）。
  - **前置依赖**：P1-001（已完成）。**不依赖 P1-003，也不依赖 D-015（`framework-*` 获取方式）——本任务不引入 `framework-*` 依赖，OPEN 项不阻塞开工。**
  - **交付物**：`rust-toolchain.toml`、`.gitignore`、`rustfmt.toml`、最小 `Cargo.toml`、开发脚本入口。
  - **验收标准**：
    - [x] `cargo build` 与 `cargo fmt --check` 在干净环境可执行且通过。
    - [x] `cargo clippy`（不触发测试）无 warning 基线。
    - [x] 工具链版本由文件固定，而非依赖开发者本机默认值。
    - [x] 未创建任何业务逻辑代码。

- [x] **P1-003 依赖与版本矩阵对齐**

  - **目标**：一次性锁定 P1 所需依赖版本，消除 `framework-*` 内部版本错位带来的不可编译风险。
  - **具体工作**：
    1. 定稿 `framework-*` 获取方式：registry 版本 vs `path` 依赖本地源码仓库（**该决策由 P1-001 归入本任务**）。P1-001 已核实 registry 上 26.11.0 全量已发布、本地源码同为 26.11.0，两条路径都成立；若走 path，记录后续切回 registry 的条件。
    2. 锁定版本：`axum 0.8`、`tokio 1.52`、`clap 4.6`、`reqwest`、`ratatui`、`serde`、`serde_json`、`specta =2.0.0-rc.25`（`framework-proc-core` 硬约束）、`specta-typescript 0.0.12`。
    3. 校验 feature 传递：`framework-web/collect` → `framework-proc-web/collect`、`framework-proc-ts/collect` → `framework-proc-core/collect`，明确哪一侧（服务端 / 生成器）需要显式开启。
    4. 记录 `inventory` 注册机制的副作用（trait object 静态注册、仅 `collect` 下生效）对构建方式的影响。
    5. 写入 `decisions.md`（D-015 及相关项）。
  - **前置依赖**：P1-002
  - **交付物**：确定后的 `Cargo.toml` 依赖段；`decisions.md` 版本对齐结论。
    - **依赖落位**：`lib-core` → `framework-core`；`lib-web` → `framework-web` / `framework-web-axum` / `framework-proc-ts` / `framework-proc-core`（后两者是宏展开的绝对路径依赖，必须直接声明并开 `collect`）；`lib-bin-cli` → `clap` / `reqwest`；`lib-bin-tui` → `ratatui`。`types-core` 的 `framework-proc-auto/collect` 与 `lib-web-core` 的 `framework-web` 随 P1-010 / P1-009 落地。
  - **验收标准**：
    - [x] `cargo build` 在锁定版本下通过，`Cargo.lock` 已生成。
    - [x] 不存在同一 `framework-*` crate 的多个版本被同时解析进依赖图（以 `cargo tree -d` 输出为证据）。
    - [x] 每个锁定版本在 `decisions.md` 中都有“为什么是它”的一句理由。
    - [x] `framework-*` 获取方式（registry / path）已在 `decisions.md` 中定稿，D-015 由 `OPEN` 改为 `DECIDED`；若选 path，切回 registry 的条件已写明。

- [x] **P1-004 技术验证：framework-proc-web 服务端 API 定义**

  - **目标**：用最小代价确认 `web_api` 宏在真实工程里的行为边界，特别是路径、参数、返回值、路由收集四件事。
  - **具体工作**：
    1. 用 `web_api_*` 定义 3 个最小接口（1 个无参 GET、1 个 `Json<T>` POST、1 个 `Query<T>` GET），返回值分别覆盖 `Void` / `Type` / `Result<T, E>`。
    2. 验证 `collect` 开启后路由被 `WebRouter` 自动收集；验证关闭 `collect` 时的手动挂载路径。
    3. 验证已知限制的实际表现并记录：path 不支持 `{id}` 占位符、GET 只能有一个参数对象、`WebResponse` 被标记为 `Blob`。
    4. 验证参数提取失败（缺字段、类型不符）时的错误响应形态与状态码。
    5. 验证 `auth` 参数默认值（`AuthRule::login()`）对本地客户端的影响，确定 P1 是否必须显式声明 `AuthRule::anonymous()`。
    6. 把限制与结论写入 `decisions.md`（D-008、D-014）。
  - **前置依赖**：P1-003
  - **交付物**：验证用临时 API 定义（验证完成后保留为契约基线，或按结构讨论结果迁移）；`decisions.md` 结论条目。
  - **验收标准**：
    - [x] 3 个接口都能被实际 HTTP 调用并返回预期响应（探针共 7 个，见 `docs/reports/p1-004-api-probe.md`）。
    - [x] 明确指出 REST 风格路径（如 `/api/sessions/{id}`）是否可用，并给出 P1 的替代方案（query 参数 / body 字段）。
    - [x] 参数错误时的响应结构被记录成一份可复制的样例（含状态码与 `R{code,message}` 取值）。
    - **实测结论**：`collect` 必须开启（关闭后无手动挂载路径）；消费路由/元数据的二进制必须引用 `lib_web::web_routes()` 作为链接锚点；`auth` 默认值不触发鉴权（D-014 已定案）；参数与错误响应约束见 D-021。探针接口保留在 `crates/lib-web/src/probe`，探针服务为 `cargo run -p lib-web --example api_probe_server`。

- [ ] **P1-005 技术验证：TypeScript SDK 生成链路**

  - **目标**：确认从服务端 API 定义到可用的 npm 包 TS SDK 全链路成立，包括类型、抽象类与包结构。
  - **具体工作**：
    1. 建一个可执行生成器（临时形态即可，最终形态在 P1-007 固化），link 开启了 `collect` 的 API crate，调用 `api_metadata_iter()`。
    2. 用 `TypescriptTypeBuilder` 导出 DTO 类型、`TypescriptEnumBuilder` 导出枚举、`TypescriptApiBuilder` 导出 `ApiDefinitions` 与抽象类，最后由 `PackageBuilder` 写出 npm ESM 包。
    3. 用 `tsc --noEmit`（或项目已有的等静态检查）验证生成物类型正确、可被消费。
    4. 验证错误分支：方法名重复、`method + path` 重复、`class_name` 非法时的报错信息是否足以定位。
    5. 验证 `i64/u64 → string` 映射对 DTO 的实际影响，确定 P1 的 id 字段写法。
    6. 记录生成物目录、包名、版本号约定。
  - **前置依赖**：P1-004
  - **交付物**：可重复运行的生成流程；生成出的 npm 包；`decisions.md` 中 D-002、D-005、D-018 结论。
  - **验收标准**：
    - [ ] 生成物包含 DTO 类型声明与每个接口对应的客户端方法。
    - [ ] 生成物的静态类型检查通过，且消费方可正常 `import` 并拿到方法签名。
    - [ ] 连续执行两次生成，第二次无文件变化（幂等）。
    - [ ] id 字段的类型映射问题已有明确结论（不再存在“TS 标 string、实际是 number”的隐患）。

- [ ] **P1-006 技术验证：framework-proc-rust 生成 Rust Client SDK**

  - **目标**：填补 `framework-proc-core` 只有 TS 生成能力的缺口，在同一份 `ApiMetadata` 上产出 Rust Client SDK，并验证其可用性。
  - **具体工作**：
    1. 在 `lingting-rust-framework` 中新增 crate `framework-proc-rust`（与 `framework-proc-ts` 平级，纯代码生成、不含 proc macro），依赖 `framework-proc-core` 取 `ApiMetadata`。
    2. 设计最小生成形态：一个 `call` 抽象（trait 必需方法，签名覆盖 method/path/body/query）+ 每个 API 一个默认方法（参数与返回类型按元数据还原）+ DTO 类型引用策略（复用服务端 DTO 类型 vs 生成独立类型，二选一并在 `decisions.md` 记录理由）。
    3. 保证与 TS 侧**同源同义**：同名接口、同名参数、同一 method+path；校验规则与 `TypescriptApiBuilder` 一致（方法名重复、method+path 重复直接报错）。
    4. 生成一个 Rust 客户端 crate（或模块），实现 `call` 的 reqwest 版本，编译通过。
    5. 处理 Rust 生成特有的类型还原问题：`Option<T>`、`Vec<T>`、`i64 → string` 的差异（Rust 侧应保持 `i64`，与 TS 侧 `string` 的差异需记录为已知差异）。
    6. 写清能力边界：不支持路径参数、不支持流式返回，与 TS 侧完全一致。
  - **前置依赖**：P1-004
  - **交付物**：`framework-proc-rust` crate（最小可用版）；生成的 Rust Client SDK；`decisions.md` 中 D-003、D-004 结论。
  - **验收标准**：
    - [ ] `framework-proc-rust` 能独立完成一次生成（输入 `api_metadata_iter()`，输出 Rust 源码）。
    - [ ] 生成代码 `cargo build` 通过，无手写补丁。
    - [ ] 生成的接口集合与 TS SDK 的接口集合完全一致（可按 name + method + path 对照）。
    - [ ] 重复生成幂等。
    - [ ] 若某项能力最终未实现，必须在 `decisions.md` 中明确降级方案（例如退回 OpenAPI 生成），不得静默放弃。

- [ ] **P1-007 API 契约单一来源与生成流程固化**

  - **目标**：把 P1-004 ~ P1-006 的验证结果固化成“唯一来源 + 一条命令 + CI 可校验”的正式流程。
  - **具体工作**：
    1. 确定 API 定义的唯一来源位置（服务端 API 定义 + DTO 定义），并规定：任何 API 变更必须先改这里。
    2. 固化生成命令（如 `cargo run -p <api-gen>` 或 `cargo xtask gen-sdk`），一条命令同时产出 TS SDK 与 Rust SDK。
    3. 规定生成物落盘位置、是否入库、包名与版本号规则、生成物头部标记（“自动生成，勿手改”）。
    4. 规定生成失败的构建行为：生成器失败必须让命令以非 0 退出，不得写出半成品。
    5. 规定 API 命名规范：path 前缀（如 `/api/`）、资源命名、动作动词（`create` / `list` / `cancel`）、DTO 命名后缀（`*Params` / `*VO`）、SSE 事件命名。
    6. 规定 API 版本管理初版策略（path 前缀 `/api/v1` 或 header，二选一并记录；P1 只需预留，不实现多版本并存）。
    7. 更新 `decisions.md` 中 D-004、D-005、D-006 的状态。
  - **前置依赖**：P1-005、P1-006
  - **交付物**：生成命令；命名规范与版本策略文档化条目；生成物目录约定。
  - **验收标准**：
    - [ ] 单命令可从零产出两份 SDK，过程中不需要任何手工步骤。
    - [ ] 文档写清了“改 API 的完整步骤”（改定义 → 跑生成 → 提交生成物）。
    - [ ] 生成器在元数据非法时以非 0 退出。

---

## Batch B：Server 基础能力

> Batch B 与 Batch C 之间除 P1-014 依赖 P1-007 外无强耦合，可在 P1-007 完成后并行推进。

- [ ] **P1-008 Server 运行时骨架**

  - **目标**：`agent server` 能启动一个真实监听、有日志、有 trace、配置可控的 HTTP 服务。
  - **具体工作**：
    1. 实现 Clap 的命令分发骨架与 `server` 子命令参数（`--addr`、`--port`、`--log-level`）。
    2. 实现配置装载优先级（命令行 > 环境变量 > 配置文件 > 默认值），默认 `127.0.0.1:20100`（固定端口见 D-022）。
    3. 初始化日志（经 `framework-core` 的 logging），输出到 stdout，级别可配置；启动失败给出可读错误与退出码。
    4. 接入 `framework-web-axum` 的 `axum_builder(...)` + `bind()`，就绪后打印 `listening on <addr>`。
    5. 确认 `x-trace-id` 透传行为（请求头存在则沿用，缺失则服务器生成雪花 ID 并回写响应头），并让日志带上 trace id。
    6. 明确崩溃策略：panic 由框架 `catch_panic` 兜底，服务器进程不退出。
  - **前置依赖**：P1-003
  - **交付物**：可运行的 `agent server`；配置装载逻辑；日志初始化；退出码约定实现。
  - **验收标准**：
    - [ ] `agent server` 启动后 `curl` 任意已定义路由有响应。
    - [ ] `agent server --port <占用中的端口>` 以非 0 退出并给出可读错误。
    - [ ] 请求头带 `x-trace-id` 时，响应头回显同一值，且日志中出现该值。
    - [ ] Ctrl+C 能退出进程（此时允许非优雅，优雅关闭由 P1-013 负责）。

- [ ] **P1-009 统一错误响应与健康检查**

  - **目标**：所有接口共享一套错误语义，并有一个最小健康检查接口供客户端探活。
  - **具体工作**：
    1. 定义 `ApiError` 契约：HTTP 状态码 + `R{code, message}` 的映射表；明确 `code` 的分配规则（稳定、可枚举、不得随意变更）。
    2. 实现错误分类映射：参数错误、未找到、未授权、内部错误、panic 各自的状态码与 code 段位。
    3. 用 `web_api_get` 实现 `health` 与 `version` 接口（返回服务版本、启动时间、协议版本）。
    4. 保证错误响应**不泄漏内部细节**（堆栈、SQL/路径等），内部细节只进日志。
    5. 为错误码建一份清单（写入 `decisions.md` 或独立表格），后续阶段只追加不复用。
  - **前置依赖**：P1-008
  - **交付物**：`ApiError` 契约与映射表；`health` / `version` 接口；错误码清单。
  - **验收标准**：
    - [ ] 至少 4 类错误（参数、404、内部、panic）各有可复现的样例响应，且结构一致。
    - [ ] 错误码清单存在且每条有含义说明。
    - [ ] 错误响应体不包含 panic 消息、文件路径等内部信息。

- [ ] **P1-010 Mock 会话业务 API**

  - **目标**：定义 P2 将复用的核心业务契约（Session / Message / Run），并用内存 Mock 实现。
  - **具体工作**：
    1. 定义 DTO：`SessionSummary`、`Message`、`Run`、`RunStatus`、`CreateSessionParams`、`SendMessageParams` 等；字段一次到位（为 P2 预留 `token 用量`、`模型`、`时间戳` 等）。
    2. 定义接口：创建会话、列出会话、获取会话消息、发送消息（创建 Run）、取消 Run、查询 Run。
    3. 实现内存存储（`Mutex<HashMap>` 级别即可），保证 Session 隔离。
    4. 遵守 P1-004 确认的路径/参数约束（无路径占位符，id 走 body/query）。
    5. 生成 SDK 后，用生成的客户端方法名反查契约是否合理（命名不自洽则改定义，不改生成物）。
    6. 记录：Mock 与真实实现的替换点（P2 在此处接入 Repository）。
  - **前置依赖**：P1-009、P1-007
  - **交付物**：核心 DTO 定义；Mock 会话接口；契约与 P2 替换点说明（写入 `decisions.md` 或独立说明）。
  - **验收标准**：
    - [ ] 可以创建会话、发消息、列出消息、创建并取消 Run。
    - [ ] 两个会话的消息互不可见（Session 隔离可验证）。
    - [ ] 重新生成 SDK 后，Rust 与 TS 客户端都能调用上述全部接口。

- [ ] **P1-011 Mock SSE 流式响应**

  - **目标**：建立 Streaming 契约与端到端通路，使 P2 的 LLM token 流只需替换事件生产方。
  - **具体工作**：
    1. 定义 `RunEvent` 事件枚举：`run_started`、`message_delta`、`run_completed`、`run_failed`、`heartbeat`（P2 可追加 `tool_call_*`，不得改已有语义）。
    2. 用 `WebResponse::stream` + `content_type: text/event-stream` 产出 SSE，实现 Mock 逐段输出（定时产出若干 delta）。
    3. 处理 SSE 必需细节：`event:` / `data:` 编码、JSON 序列化、`id`、心跳间隔、结束事件、错误事件。
    4. 处理客户端断开：流被 drop 时释放资源、停止 Mock 任务，不 panic、不泄漏任务。
    5. 明确 SSE 与 SDK 的关系：当前元数据只有 `Void` / `Blob` / `Type`，无流式语义（见 `decisions.md` D-007），记录选定方案。
  - **前置依赖**：P1-010
  - **交付物**：`RunEvent` 契约；SSE 接口；流式取消行为。
  - **验收标准**：
    - [ ] `curl -N` 能看到连续事件，事件以 `run_completed` 结束。
    - [ ] 客户端中途断开后，服务端日志无 panic，且进行中的任务被清理。
    - [ ] 事件枚举与字段在文档中有完整表格，P2 可直接复用。

- [ ] **P1-012 CORS、静态资源与 WebUI 兜底路由**

  - **目标**：让 WebUI 能通过浏览器访问 Server API，并为 P1-017 的资源嵌入准备好托管入口。
  - **具体工作**：
    1. 使用 `AxumBuilder::with_cors` 配置 CORS：默认仅允许本地来源（`http://127.0.0.1:<webui port>`、`http://localhost:<webui port>`），拒绝通配。
    2. 处理 `OPTIONS` 预检（框架已返回 204，需验证响应头符合浏览器要求）。
    3. 规划静态资源路由与 SPA fallback（未命中 API 的 GET 回落到 `index.html`），保证不影响 API 404 语义。
    4. 明确 API 路径前缀（如 `/api/`）与静态资源路径的隔离规则。
    5. P1 阶段静态资源先以本地目录提供，P1-017 再切换为嵌入资源（两者接口一致）。
  - **前置依赖**：P1-009
  - **交付物**：CORS 配置；静态资源托管与 fallback 路由；路径隔离规则。
  - **验收标准**：
    - [ ] 浏览器来源下 API 调用不被 CORS 拦截，非允许来源被拒绝。
    - [ ] 访问未知前端路由返回 `index.html`，访问未知 API 路径返回 API 404 结构。
    - [ ] OPTIONS 预检响应包含正确的 `access-control-*` 头。

- [ ] **P1-013 优雅关闭与生命周期**

  - **目标**：让 Server 可被可靠停止：不产生半条 SSE 事件、不丢日志、及时退出。
  - **具体工作**：
    1. 解决框架能力缺口：`framework-web-axum` 的 `run(wrapper)` 未暴露关闭入口（已核对源码）。二选一并记录：向上游补 `run_with_shutdown(shutdown_future)`，或在 P1 内用 axum `serve(...).with_graceful_shutdown(...)` 自行组装。
    2. 实现关闭信号处理：Ctrl+C / SIGTERM → 停止接受新连接 → 等待进行中请求 → 主动终止 SSE 流并发送终止事件 → 退出。
    3. 实现兜底超时（默认 5s）后强制退出，并记录返回值。
    4. 定义退出码：`0` 正常、`1` 运行错误、`2` 参数错误；关闭超时是否为错误需明确结论。
    5. 明确 Server 重启后的客户端行为预期（P1 不保证会话恢复，P2 才引入持久化）。
  - **前置依赖**：P1-008
  - **交付物**：关闭信号链路；关闭超时机制；退出码实现。
  - **验收标准**：
    - [ ] Ctrl+C 后进程在超时内退出，日志有完整的关闭过程记录。
    - [ ] 关闭瞬间的 SSE 客户端收到终止事件或明确断开，不会挂死。
    - [ ] 关闭期间新请求被拒绝且返回可读错误，而非连接被静默丢弃。
    - [ ] `framework-web-axum` 的改动（若有）已记录在 `decisions.md`，并说明是否已合入上游。

---

## Batch C：客户端

- [ ] **P1-014 CLI 命令结构与运行模式分发**

  - **目标**：一个 binary 承载全部命令，且每个命令都通过生成的 SDK 访问 Server。
  - **具体工作**：
    1. 用 Clap 定义命令树：无子命令 → TUI；`tui`、`server`、`run`、`status` 子命令各自参数。
    2. 实现 Server 地址解析（命令行 > 环境变量 > 配置文件 > 默认值），并保证 TUI / run / status 共用同一套解析。
    3. 实现 SDK 的 Transport 层：基于 reqwest 的 `call` 实现、超时、连接失败错误分类（拒绝连接 / 超时 / 非 2xx / 反序列化失败）。
    4. 实现 `agent run "<task>"`（Mock 下等价于创建会话 + 发消息 + 打印结果），支持 `--json` 与 `--stream` 两种输出。
    5. 实现 `agent status`（输出服务地址、健康状态、版本、运行中的 Run 数）。
    6. 实现错误呈现：所有失败路径都是“一行可读错误 + 非 0 退出码”，无堆栈直出。
  - **前置依赖**：P1-007、P1-008
  - **交付物**：命令树；地址解析；SDK Transport；`run` / `status` 命令。
  - **验收标准**：
    - [ ] `agent --help` 与 `agent --version` 输出符合预期，退出码 0。
    - [ ] `agent run "..."` 在 Server 运行时可返回结果；未运行时给出可读错误与非 0 退出码。
    - [ ] `agent status` 能区分“服务未启动”“服务异常”“服务正常”三种情况。
    - [ ] 客户端代码中不存在除探活外的裸 HTTP path 字面量（以代码检索为证据）。

- [ ] **P1-015 TUI 客户端与 Streaming 展示**

  - **目标**：TUI 能完成“发问 → 看流式回答 → 中断”的最小闭环，并在断线时给出明确反馈。
  - **具体工作**：
    1. 用 Ratatui 搭最小界面：状态栏（Server 地址、连接状态）、会话区、消息区、输入框。
    2. 实现事件循环：键盘输入、SSE 事件、渲染 tick 三类事件不互相阻塞（避免输入卡顿）。
    3. 实现消息增量渲染：收到 `message_delta` 立即追加并重绘，保持滚动位置合理。
    4. 实现断线处理：连接失败显示提示与“重试”操作；流式中断保留已收到的内容并标注“连接中断”。
    5. 实现中断当前 Run（`Esc`）→ 调用取消接口 → 界面回到可输入状态。
    6. 实现退出（`q` / `Ctrl+C`）时恢复终端状态（不残留 alternate screen / raw mode）。
  - **前置依赖**：P1-014、P1-011
  - **交付物**：TUI 界面与事件循环；流式渲染；断线与取消处理；终端状态恢复。
  - **验收标准**：
    - [ ] 无参数运行 `agent` 直接进入 TUI；`agent tui` 行为一致。
    - [ ] 流式回答可见逐字增长，期间输入框仍可响应按键。
    - [ ] 结束 TUI 后终端无残留（光标、滚动、颜色均正常）。
    - [ ] 关闭 Server 后 TUI 有明确提示且不崩溃。

- [ ] **P1-016 WebUI 最小对话界面**

  - **目标**：浏览器中能完成“发问 → 看流式回答”的最小闭环，且全部调用走生成的 TS SDK。
  - **具体工作**：
    1. 确定前端技术栈与工程脚手架（React + Vite + TS 为推荐项，最终结论写入 `decisions.md` D-010）。
    2. 接入生成的 TS SDK：实现抽象类的 `call` 具体子类（fetch Transport），处理错误响应。
    3. 实现 SSE 消费：浏览器 `EventSource` 或 fetch stream 二选一，需支持中断与错误事件（写入 `decisions.md` D-007）。
    4. 实现最小界面：消息列表、输入框、发送按钮、流式增量渲染、错误提示条。
    5. 明确前端如何使用 SDK 生成物（本地路径依赖 / npm workspace / 直接引用 dist），并写入 `.steps` 约定。
  - **前置依赖**：P1-007、P1-012
  - **交付物**：最小 WebUI；TS SDK 接入；SSE 消费；构建产物。
  - **验收标准**：
    - [ ] 浏览器可发送消息并看到流式输出。
    - [ ] 所有 API 调用均来自生成 SDK（无手写 path 字面量）。
    - [ ] Server 关闭时界面显示可读错误而非白屏。

- [ ] **P1-017 WebUI 静态资源嵌入 Binary**

  - **目标**：WebUI 产物编译进 Rust binary，使单文件部署时无需携带静态目录。
  - **具体工作**：
    1. 选择嵌入方案（`rust-embed` vs `include_dir` vs 构建期生成，写入 `decisions.md` D-009）。
    2. 实现资源读取 → `WebResponse`（含 `Content-Type`、`Content-Length`、缓存头）。
    3. 处理 SPA fallback 与 API 路由的优先级；处理 404 与压缩资源。
    4. 保证开发模式仍可指向本地目录（热更新不受嵌入影响）。
    5. 确定前端构建与 Rust 构建的先后顺序，固化到构建脚本。
  - **前置依赖**：P1-016
  - **交付物**：资源嵌入实现；构建顺序脚本；开发/发布两种模式。
  - **验收标准**：
    - [ ] Release binary 在**不含任何静态目录**的环境运行后，浏览器可正常打开 WebUI。
    - [ ] 资源响应头正确（`content-type` 与文件类型一致）。
    - [ ] 开发模式下修改前端代码无需重新编译 Rust。

---

## Batch D：收口

- [ ] **P1-018 单 Binary Release 构建与跨平台策略**

  - **目标**：产出可独立运行、可分发的 Release 产物，并留下体积与启动时间的基线数据。
  - **具体工作**：
    1. 配置 release profile（`lto`、`codegen-units`、`strip`、`panic` 策略），权衡体积与启动时间。
    2. 固化构建顺序：前端构建 → SDK 生成 → Rust 构建，一条命令产出完整 binary。
    3. 明确跨平台策略：Windows 为第一目标；Linux / macOS 的构建方式（CI 矩阵或交叉编译）给出结论，不在 P1 强求全部可用。
    4. 记录 binary 体积与冷启动时间（首次 `server` 启动到打印 `listening on`），作为后续阶段对比基线。
    5. 验证“无开发工具环境可运行”：只拷贝 binary 到干净目录执行 TUI / `server`。
  - **前置依赖**：P1-015、P1-017
  - **交付物**：release 构建配置；一键构建脚本；基线数据记录；跨平台结论。
  - **验收标准**：
    - [ ] 单条命令产出可运行 binary。
    - [ ] 干净目录下 binary 可启动 server 并打开 WebUI。
    - [ ] 体积与启动时间数据记录在 `.steps` 中，且注明测量环境与方式。
    - [ ] `--help` / `--version` / 三个运行模式在 release 构建下均可用。

- [ ] **P1-019 API 一致性与生成物 CI 门禁**

  - **目标**：让“忘记重新生成 SDK”变成构建失败，而不是依赖人工记忆。
  - **具体工作**：
    1. 实现一致性检查命令：重新生成 → 与工作区生成物比对 → 有差异即失败并输出 diff 摘要。
    2. 把检查接入构建/CI 流水线（Rust 检查 + 生成物一致性 + 前端静态类型检查）。
    3. 明确失败提示文案：指出“API 定义已变更，请运行 <命令> 重新生成并提交”。
    4. 规定生成物的 merge 策略（生成物冲突时重新生成，不手工合并）。
    5. 记录 CI 步骤与本地步骤的等价性（同一命令，避免“本地过 CI 不过”）。
  - **前置依赖**：P1-007、P1-018
  - **交付物**：一致性检查命令；CI 配置；失败提示与处理约定。
  - **验收标准**：
    - [ ] 修改任一 API 定义后不重新生成，检查命令以非 0 退出并给出可操作提示。
    - [ ] 重新生成后检查通过。
    - [ ] 本地与 CI 使用完全相同的检查命令。

- [ ] **P1-020 P1 端到端 Mock 闭环验收与结论回填**

  - **目标**：按 `acceptance.md` 逐条完成 P1 验收，并把过程中的偏差回填到文档。
  - **具体工作**：
    1. 按 `acceptance.md` 九类验收逐条执行并记录结论。
    2. 更新 `tasks.md` 状态与 `.steps/README.md` 的进度表。
    3. 回填 `decisions.md`：把 `OPEN` 改为 `DECIDED`，补充实现过程中发现的偏离。
    4. 记录 P1 遗留问题清单（哪些不做、为什么、在哪个阶段处理）。
    5. 产出 P2 的输入：确认 P2 需要复用的契约与替换点清单。
  - **前置依赖**：P1-018、P1-019（以及 Batch A–C 全部任务）
  - **交付物**：验收结论；已更新的路线图文档；P1 遗留问题清单；P2 输入清单。
  - **验收标准**：
    - [ ] `acceptance.md` 中每一条都有“通过 / 不通过 + 证据”的结论。
    - [ ] 不存在状态与实际不符的任务勾选。
    - [ ] P1 遗留问题清单每条都有归属阶段。
