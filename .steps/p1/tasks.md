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

- [x] **P1-005 技术验证：TypeScript SDK 生成链路**

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
    - [x] 生成物包含 DTO 类型声明与每个接口对应的客户端方法。
    - [x] 生成物的静态类型检查通过，且消费方可正常 `import` 并拿到方法签名。（静态检查与 `workspace:*` 引入已在前端工程内实测通过；调用级使用归 P1-016）
    - [x] 连续执行两次生成，第二次无文件变化（幂等）。
    - [x] id 字段的类型映射问题已有明确结论（不再存在“TS 标 string、实际是 number”的隐患）。
    - **实测结论**：生成器为 `cargo run -p lib-web --example gen_ts_sdk`，产物落 `packages/sdk-ts`（不入库，pnpm workspace 消费）；`i64` 经 `auto_type` 标注为 `string`，运行时同样是字符串，标注与 wire 一致；`WebResponse` 类返回在 TS 侧退化为 `Blob`。报告见 `docs/reports/p1-005-ts-sdk.md`；`tsc --noEmit` 与 `workspace:*` 引入已在前端工程 `lingting-agent-ui/` 内实测通过。

- [x] **P1-006 技术验证：framework-proc-rust 生成 Rust Client SDK**

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
    - [x] `framework-proc-rust` 能独立完成一次生成（输入 `api_metadata_iter()`，输出 Rust 源码）。**形态变更**：Rust 导出并入 `framework-proc-core` 的 `rust` 特性（`RustApiBuilder` / `RustTypeBuilder` / `RustEnumBuilder` / `CrateBuilder`），未新增独立 crate，见 D-003。
    - [x] 生成代码 `cargo build` 通过，无手写补丁。
    - [x] 生成的接口集合与 TS SDK 的接口集合完全一致（7 个接口，name / method / path 同源同一份元数据）。
    - [x] 重复生成幂等。
    - [x] 若某项能力最终未实现，必须在 `decisions.md` 中明确降级方案（例如退回 OpenAPI 生成），不得静默放弃。
    - **实测结论**：生成器为 `cargo run -p lib-web --example gen_rust_sdk`，产物落 `packages/sdk-rust/src`（不入库、不生成 `Cargo.toml`，`lib-bin-cli` 用 `#[path]` 引入）；DTO 走“自带快照定义”策略；已知差异与未实现项（reqwest Transport 实现、流式返回、路径参数）见 `docs/reports/p1-006-rust-sdk.md` 与 D-003。

- [x] **P1-007 API 契约单一来源与生成流程固化**

  - **目标**：把 P1-004 ~ P1-006 的验证结果固化成“唯一来源 + 一条命令 + CI 可校验”的正式流程。
  - **具体工作**：
    1. 唯一来源：服务端 `#[web_api_*]` handler 与 `#[auto_type]` / `#[auto_enum]` DTO，位于 `crates/lib-web/src/`；任何 API 变更必须先改这里。
    2. 固化生成命令：根 `Makefile` 的 `make gen`，一条命令顺序产出 TS SDK 与 Rust SDK；`scripts/` 已删除，Makefile 是唯一命令入口。
    3. 生成物落位 `packages/sdk-rust/src`（`#[path]` 引入）与 `packages/sdk-ts`（pnpm workspace 消费）；**不入库**，因此不存在“提交生成物”步骤；npm 包名 `@lingting/agent-api`，版本由 `CARGO_PKG_VERSION` 同步 workspace 版本；Rust 侧头部写“自动生成，请勿手动修改”，TS 侧为框架写入的 Specta do-not-edit 标记。
    4. 失败行为：全部校验与源码构建先于写盘完成；写盘先落同级暂存目录（`.gen-tmp`），成功后才整体替换目标目录；失败以非 0 退出并保留上一次产物。
    5. 命名规范：path 前缀 `api/`（不带版本段）；资源段小写单数名词、多词用 `-`；动作动词固定为 `create` / `update` / `delete` / `get` / `list` / `cancel`；请求体 `*PO`、查询参数 `*Params`、响应 `*VO`、枚举 `<业务名>Kind`；SSE 事件 P1 未建模（D-007），事件名字段统一 `event`。规范写入 `docs/develop.md`「API 契约规范」。
    6. 版本策略：P1 不做版本化，path 保持 `api/...`，不预留 `/api/v1`；将来引入多版本时用 path 前缀而非请求头（D-019 已 DECIDED）。
    7. `decisions.md` 的 D-004、D-005、D-006、D-019 已回填并置为 DECIDED。
  - **前置依赖**：P1-005、P1-006
  - **交付物**：`Makefile` 生成入口；`docs/develop.md`「API 契约规范」；`docs/reports/p1-007-api-contract.md`；生成物目录约定。
  - **验收标准**：
    - [x] 单命令 `make gen` 可从零产出两份 SDK，过程中不需要任何手工步骤。
    - [x] 文档写清了“改 API 的完整步骤”（改定义 → 跑生成 → `make check` / `make build`；生成物不入库，无提交生成物步骤）。
    - [x] 生成器在元数据非法时以非 0 退出（校验先于写盘，失败保留上一次产物）。
  - **实测结论**：`make gen` 连续执行幂等；暂存目录 `.gen-tmp` 成功后清理、失败时旧产物保持可用；`make check` 通过。详见 `docs/reports/p1-007-api-contract.md`。

---

## Batch B：Server 基础能力

> Batch B 与 Batch C 之间除 P1-014 依赖 P1-007 外无强耦合，可在 P1-007 完成后并行推进。

- [x] **P1-008 Server 运行时骨架**

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
    - [x] `agent server` 启动后 `curl` 任意已定义路由有响应。
    - [x] `agent server --port <占用中的端口>` 以非 0 退出并给出可读错误。
    - [x] 请求头带 `x-trace-id` 时，响应头回显同一值，且日志中出现该值。
    - [x] Ctrl+C 能退出进程（此时允许非优雅，优雅关闭由 P1-013 负责）。

- [x] **P1-009 统一错误响应与健康检查**

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
    - [x] 至少 4 类错误（参数、404、内部、panic）各有可复现的样例响应，且结构一致。
    - [x] 错误码清单存在且每条有含义说明。
    - [x] 错误响应体不包含 panic 消息、文件路径等内部信息。
    - **实测结论**：`code` 与 HTTP 状态码同值（框架 `WebResponse::error_body` 固定，业务码需改框架，P1 不做）；契约类型 `ApiError` 落在 `crates/lib-web/src/error.rs`（`auto_type`，已进入 TS / Rust SDK）；`health` / `version` 按用户要求直接扩展探针模块（`api/probe/health`、`api/probe/version`，载荷 `ProbeHealthVO` / `ProbeVersionVO`，协议版本常量 `lib_web::probe::PROTOCOL_VERSION`），启动时刻由 `probe::mark_startup()` 在服务启动时记录；`internal` / `panic` 两条样例接口补齐错误分类验证；错误码清单与 curl 复现样例见 D-024。注意裸 `anyhow!` 错误（`WebErrorKind::Message`）会把原始 message 原样对外。

- [x] **P1-010 Mock 会话业务 API**

  - **目标**：定义 P2 将复用的核心业务契约（Session / Message / Run），并用内存 Mock 实现。
  - **具体工作**：
    1. 定义 DTO：`SessionSummary`、`Message`、`Run`、`RunStatus`、`SessionCreatePO`、`MessageSendPO` 等；字段一次到位（为 P2 预留 `token 用量`、`模型`、`时间戳` 等）。命名以 `docs/develop.md` 的契约规范为准（请求体 `*PO`、查询 `*Params`），D-025 有说明。
    2. 定义接口：创建会话、列出会话、获取会话消息、发送消息（创建 Run）、取消 Run、查询 Run。
    3. 实现内存存储（`Mutex<HashMap>` 级别即可），保证 Session 隔离。
    4. 遵守 P1-004 确认的路径/参数约束（无路径占位符，id 走 body/query）。
    5. 生成 SDK 后，用生成的客户端方法名反查契约是否合理（命名不自洽则改定义，不改生成物）。
    6. 记录：Mock 与真实实现的替换点（P2 在此处接入 Repository）。
  - **前置依赖**：P1-009、P1-007
  - **交付物**：核心 DTO 定义；Mock 会话接口；契约与 P2 替换点说明（写入 `decisions.md` 或独立说明）。
  - **验收标准**：
    - [x] 可以创建会话、发消息、列出消息、创建并取消 Run。
    - [x] 两个会话的消息互不可见（Session 隔离可验证）。
    - [x] 重新生成 SDK 后，Rust 与 TS 客户端都能调用上述全部接口。
    - **实测结论**：契约类型落在 `crates/types-core/src/session/`（`auto_type` / `auto_enum`，`framework-proc-auto/collect` 已在 `types-core` 开启），Mock 业务在 `service-core/src/session/`（`Mutex<HashMap>` 存储 + 进程级 `holder`），Handler 在 `crates/lib-web/src/session/api.rs`；接口共 6 条（`api/session/create`、`api/session/list`、`api/message/list`、`api/message/create`、`api/run/get`、`api/run/cancel`）。`make gen` 后 17 条接口（11 条探针 + 6 条会话）同时进入两份 SDK：TS 侧方法为 `sessionCreate` / `sessionList` / `messageList` / `messageCreate` / `runGet` / `runCancel`，Rust 侧为同名 snake_case，`Vec<SessionSummary>` 映射为 `SessionSummary[]`，枚举为 `RunStatus`（`QUEUED`/`RUNNING`/`COMPLETED`/`CANCELLED`/`FAILED`）与 `MessageRole`。
    - **运行期验证（已执行）**：`bin-agent server` 启动后逐条调用，20 项检查全部通过 —— 创建会话（显式标题 / 默认标题 `新会话` / 默认模型 `mock-model` / 显式模型）、发消息返回 `USER` 消息与 `QUEUED` 的 Run 且 `message.runId` 指向该 Run、两个会话各自只看到自己的消息、会话 `messageCount` 与 `updatedAtMs` 倒序正常、`api/run/get` 查到 `QUEUED`、`api/run/cancel` 变为 `CANCELLED` 且 `finishedAtMs` 有值、状态持久化、重复取消返回 400、未知会话（含发消息时）与未知 Run 返回 404、空白内容返回 400、缺字段返回 400 且错误体只有 `{code,message}`；服务端日志无 panic、无 500，参数错误与业务错误的 `error_chain` 不再重复同一句话。SDK 侧：Rust 生成物中 6 个方法随 `lib-bin-cli` 编译通过，TS 生成物 `dist/index.d.ts` 在 `tsc --noEmit --strict`（`skipLibCheck: false`）下通过；客户端 Transport 实现属 P1-014 / P1-016，本节只验证生成物可调用。契约与 P2 替换点见 D-025。

- [x] **P1-011 Mock SSE 流式响应**

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
    - [x] `curl -N` 能看到连续事件，事件以 `run_completed` 结束。
    - [x] 客户端中途断开后，服务端日志无 panic，且进行中的任务被清理。
    - [x] 事件枚举与字段在文档中有完整表格，P2 可直接复用。
    - **实测结论**：契约类型落在 `crates/types-core/src/session/event.rs`（`RunEventKind` 为 `auto_enum`，serde / strum 均为 `snake_case`；`RunEvent` 为 `auto_type` 扁平结构体），入参 `RunStreamParams` 在 `request.rs`；接口为 `GET api/run/stream?runId=`（新增动作动词 `stream`，D-026）。分层：`service-core/src/session/stream.rs` 只产出 `RunEvent`（`mpsc` + 生产任务），`lib-web/src/session/stream.rs` 把事件映射成 SSE 帧，`lib-web-core/src/sse.rs` 负责 `id` / `event` / `data` 编码与响应头常量。Mock 节奏：助手回复按 4 字符切分、每段间隔 120ms，`inputTokens` / `outputTokens` 按字符数估算。
    - **运行期验证（已执行）**：`bin-agent server --port 20100` 起服务后，`curl -N` 收到 19 帧（`run_started` + 17 × `message_delta` + `run_completed`），事件按约 120ms 逐个到达、`transfer-encoding: chunked`（无 `content-length`，A-4 直连未被缓冲）；响应头含 `content-type: text/event-stream; charset=utf-8`、`cache-control: no-cache`、`x-accel-buffering: no`，`x-trace-id` 正常回写。`run_completed` 后 `api/run/get` 为 `COMPLETED`（`finishedAtMs`、token 用量齐全），`api/message/list` 多出一条 `ASSISTANT` 消息且 `messageCount` 递增。断线（`curl --max-time 0.6` 中途退出）后 Run 变为 `CANCELLED` 且带 `finishedAtMs`，只有 `USER` 消息、没有半成品助手消息，日志无 panic。流式期间 `api/run/cancel` 后事件停止推送（7 帧后 EOF）、Run 保持 `CANCELLED`、不写助手消息。重复订阅返回 400 `Run 当前状态为 COMPLETED，不可订阅事件流`，未知 Run 返回 404 `Run 不存在：999`；全程日志只有这两条预期业务错误，无 panic。SDK：`make gen` 后接口总数 18 条，Rust 生成物导出 `RUN_STREAM.path = api/run/stream`（返回 `Vec<u8>`）与 `RunEvent` / `RunEventKind`，TS 生成物导出 `ApiDefinitions.runStream.path` 与同名类型，`tsc --noEmit --strict --skipLibCheck false` 通过。契约、断线语义与 P2 替换点见 D-026，`run_failed` 在 P1 无产出方（Mock 不会失败）。

- [x] **P1-012 CORS、静态资源与 WebUI 兜底路由**

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
    - [x] 浏览器来源下 API 调用不被 CORS 拦截，非允许来源被拒绝。
    - [x] 访问未知前端路由返回 `index.html`，访问未知 API 路径返回 API 404 结构。
    - [x] OPTIONS 预检响应包含正确的 `access-control-*` 头。
    - **运行期验证（已执行）**：`make ui-build` 产出 `lingting-agent-ui/dist` 后启动 `bin-agent server --port 20100`。预检 `OPTIONS api/probe/void`（`Origin: http://127.0.0.1:20101`）返回 204，带 `access-control-allow-origin` / `allow-methods` / `allow-headers` / `expose-headers` / `max-age`；非白名单 Origin 的预检与实际请求都不输出任何 `access-control-*`。`GET api/probe/health`（白名单 Origin）返回 200 且带 allow-origin。`GET api/probe/not-exist`、`GET api/unknown.js`、`POST chat/abc` 均返回 404 `{"code":404,"message":"请求接口不存在"}`。`GET /` 与 `GET /chat/abc` 返回 200 `text/html` 且与 `dist/index.html` 字节一致（`cache-control: no-cache`）。`GET assets/index-*.js|css` 返回 200 对应 `content-type` 与 `public, max-age=31536000, immutable`。`GET assets/missing.js`、`--path-as-is /../Cargo.toml` 与 `/%2e%2e/Cargo.toml` 均返回 404 文本且未泄露文件。无 dist 目录时前端路由返回 503 文本，启动日志告警，两次运行均无 panic。

- [x] **P1-013 优雅关闭与生命周期**

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
    - [x] Ctrl+C 后进程在超时内退出，日志有完整的关闭过程记录。
    - [x] 关闭瞬间的 SSE 客户端收到终止事件或明确断开，不会挂死。
    - [x] 关闭期间新请求被拒绝且返回可读错误，而非连接被静默丢弃。
    - [x] `framework-web-axum` 的改动（若有）已记录在 `decisions.md`，并说明是否已合入上游。
    - **实现结论（2026-11-07）**：框架侧已提供关闭能力（`AxumServer::shutdown()` / `AxumShutdown` / `AxumContext::subscribe_stopping()` / `AxumStatus`），本仓库未修改框架，D-013 由 `OPEN` 改为 `DECIDED`。关闭链路落在 `lib-bin-web` 的 `shutdown` 模块：`serve` 启动前用 `lib_core::lifecycle::set_shutdown_hook` 注册“停止服务”的动作，再拉起信号任务；`Ctrl+C` / `SIGTERM` 与 debug 探针 `POST api/probe/shutdown` 都只调用 `lifecycle::request_shutdown()`，由钩子先等 `REJECT_WINDOW = 200ms` 的拒绝窗口再调用 `AxumShutdown::shutdown()`（立刻停监听会让关闭期的新请求在连接层被丢弃，无法返回可读错误）。`serve` 用 `select!` 同时等 `server.run` 与 `shutdown::deadline`（`TIMEOUT = 5s`，从进入停止流程起算），超时以错误返回、退出码 1，全程不用 `std::process::exit`（保证 `LoggingGuard` 刷盘）。关闭标记是 `lib-core` 的进程级 `AtomicBool`，`service-core` 的 SSE 生产方在每段增量前读取：命中即把 Run 落成 `CANCELLED` 并推一条 `RunEventKind::ServerShutdown`（`status = CANCELLED`、`error = 服务正在关闭，事件流已终止`）后收尾；关闭期间 `ShutdownGuard` 拦截器（`order = -1`）对所有请求返回 `503 {"code":503,"message":"服务正在关闭，请稍后重试"}`。退出码沿用 `bin-agent` 现有映射（0 正常 / 1 运行错误 / 2 参数与配置错误，关闭超时归入 1）。`make gen` 后 Rust / TS 两份 SDK 均导出 `ServerShutdown` / `"server_shutdown"`，`docs/api/session.md` 补齐事件表与 SSE 关闭语义，决策与遗留见 D-028。
    - **运行期验证（已执行，debug 构建 + `POST api/probe/shutdown` 触发）**：探针返回 204；紧接着的 `GET api/probe/health` 返回 503 `{"code":503,"message":"服务正在关闭，请稍后重试"}`；进行中的 SSE 流在下一段增量处收到末帧 `event: server_shutdown`（`status=CANCELLED`、`error="服务正在关闭，事件流已终止"`）后 EOF，没有半条帧；进程自行退出、退出码 0；日志依次为 `进入关闭拒绝窗口，新请求返回 503 reject_window_ms=200` → `Axum 服务进入停止流程，等待进行中的请求结束` → `Axum 服务已停止` → `服务已停止`。同一代码的 release 构建 `已收集的接口数量 count=18`（debug 为 19），`POST api/probe/shutdown` 返回 404，证明该探针只在 debug 编译。Ctrl+C 路径由人工确认，与探针共用同一条 `request_shutdown()` 链路；关闭超时（退出码 1）没有可稳定制造的长请求，未做运行期验证。

---

## Batch C：客户端

- [x] **P1-014 CLI 命令结构与运行模式分发**

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
    - [x] `agent --help` 与 `agent --version` 输出符合预期，退出码 0。
    - [x] `agent run "..."` 在 Server 运行时可返回结果；未运行时给出可读错误与非 0 退出码。
    - [x] `agent status` 能区分“服务未启动”“服务异常”“服务正常”三种情况。
    - [x] 客户端代码中不存在除探活外的裸 HTTP path 字面量（以代码检索为证据）。

- [x] **P1-015 TUI 客户端与 Streaming 展示**

  - **目标**：TUI 能完成“发问 → 看流式回答 → 中断”的最小闭环，并在断线时给出明确反馈。
  - **具体工作**：
    1. 用 Ratatui 搭最小界面：状态栏（Server 地址、连接状态）、会话区、消息区、输入框。
    2. 实现事件循环：键盘输入、SSE 事件、渲染 tick 三类事件不互相阻塞（避免输入卡顿）。
    3. 实现消息增量渲染：收到 `message_delta` 立即追加并重绘，保持滚动位置合理。
    4. 实现断线处理：连接失败显示提示与“重试”操作；流式中断保留已收到的内容并标注“连接中断”。
    5. 实现中断当前 Run（`Esc`）→ 调用取消接口 → 界面回到可输入状态。
    6. 实现退出（`q` / `Ctrl+D`）时恢复终端状态（不残留 alternate screen / raw mode）；`Ctrl+C` 不响应。
  - **前置依赖**：P1-014、P1-011
  - **交付物**：TUI 界面与事件循环；流式渲染；断线与取消处理；终端状态恢复。
  - **验收标准**：
    - [x] 无参数运行 `agent` 直接进入 TUI；`agent tui` 行为一致。
    - [x] 流式回答可见逐字增长，期间输入框仍可响应按键。
    - [x] 结束 TUI 后终端无残留（光标、滚动、颜色均正常）。
    - [x] 关闭 Server 后 TUI 有明确提示且不崩溃。
    - **实现结论（2026-11-09）**：代码已落地，`lib-bin-tui` 按 `lib.rs`（终端 + 事件循环）/ `terminal.rs`（raw 模式与备用屏幕的进入与恢复）/ `state.rs`（界面状态与按键迁移）/ `ui.rs`（渲染）/ `worker.rs`（工作线程）/ `event.rs`（两侧事件契约）/ `text.rs`（显示宽度与折行）/ `testing.rs`（`testing` feature 下的无终端驱动）拆分。界面为「状态栏 / 提示行 / 会话区 + 消息区 / 输入框 / 快捷键」五段布局，状态栏显示服务地址、连接状态（连接中 / 已连接 v版本 / 未连接）与流式阶段（等待响应 / 流式输出中 / 正在中断）。`message_delta` 到达即追加到最后一条助手消息并重绘，消息区按显示宽度自行折行、默认跟随底部，`PgUp`/`PgDn` 可回看。断线（连接失败、事件流中断、`server_shutdown`）保留已收到的增量并追加「（连接中断，内容可能不完整）」标记，提示行给出原因与 `Ctrl+R` 重试；重试成功按服务端消息整体重载。空闲且连接正常时每 5 秒探活一次（只上报失败），服务端被关掉后无需用户操作即可看到「未连接」。`Esc` 先调 `api/run/cancel` 再等服务端结束事件流，收尾后回到可输入状态；`Ctrl+D` 退出（有进行中的 Run 时先发 `api/run/cancel` 再退出），`Ctrl+C` 不响应，`q` 仅在会话区焦点退出且与 `Ctrl+D` 同一条退出路径，退出与 panic 都经 `Terminal` 的 `Drop` 恢复终端。连接管理与取消时序见 D-011。
    - **自动化覆盖（2026-11-10）**：`bin-tests` 的 `tui.*` 用例通过 `lib-bin-tui` 的 `testing` feature 以无终端驱动运行（按键走 `handle_key`、更新走 `apply`、渲染落 `TestBackend`）：`tui.input` / `tui.paste`（输入编辑与粘贴）、`tui.render`（渲染与光标定位）、`tui.commands`（Ctrl+N / Ctrl+R / Ctrl+J）、`tui.keys` 组的 `tui.ctrl_c_ignored` / `tui.ctrl_d_cancel`（退出键）、`tui.esc_cancel`（中断）、`tui.send`（发送与忙碌占位）、`tui.stream`（流式闭环）、`tui.offline`（服务端未启动）、`tui.interrupted`（运行中断线）。`lib-bin-tui` 内的单元测试已按项目约束移除，等价断言全部迁到 `bin-tests`。
    - **验收结论（2026-11-10，按 `.steps/p1/acceptance.md` 的 T-1 ~ T-9）**：
      - T-1 默认进入：非交互环境下 `agent` 与 `agent tui` 都进入事件循环（6 秒超时后仍未退出），输出备用屏幕 `?1049h` 与 bracketed paste `?2004h` 序列，日志落 `logs-tui`；`agent status` 立即以「服务未启动」退出作对照，说明无参数确实分发到 TUI 分支。界面布局与配色已随人工验证 T-7 时的真机操作一并过目。
      - T-2 基本闭环：`tui.stream` 断言增量从第一段起持续变长、最终回显用户输入，且渲染帧里能看到助手标签与回复正文。
      - T-3 输入不阻塞：`tui.stream` 在流式期间键入并断言输入框内容变化；`tui.send` 断言回复未结束时按 Enter 不发送、输入保留并给提示。
      - T-4 中断：`tui.esc_cancel` 断言 `Esc` 后进入「正在中断」、事件流收尾后回到空闲并给被中断回复打标，服务端 Run 终态为已取消。
      - T-5 启动时 Server 未运行：`tui.offline` 断言界面不退出、显示服务地址与失败原因，`Ctrl+R` 重试仍失败时继续显示未连接且可正常渲染。
      - T-6 运行中断线：`tui.interrupted`（isolated 用例，debug 探针触发服务关闭）断言断线前的增量被保留、助手消息标注「内容可能不完整」、提示行为「连接中断」、状态栏转为未连接，断线后渲染与输入仍可用。
      - T-7 终端恢复：**通过（人工验证）**。真实终端里 `agent tui` → 发问 → `Ctrl+D` 退出后无残留（光标、滚动、颜色正常）；恢复由 `Terminal::Drop` 的 `?1049l` / `?2004l` / `?25h` 序列完成。无 tty 环境下无法投递按键，故未做自动化。
      - T-8 全部走 SDK：静态检索 `crates/lib-bin-tui`、`crates/lib-bin-cli`（`sdk` 目录之外）与 `crates/bin-tests`，均无 `api/` 裸 path 字面量。
      - T-9 退出键：`tui.ctrl_c_ignored` 断言 `Ctrl+C` 在输入焦点与会话焦点都不退出、不改输入；`tui.ctrl_d_cancel` 断言流式输出中 `Ctrl+D` 退出并取消 Run，服务端 Run 落成 `Cancelled`。
      - 回归结果：`bin-tests` 的 `tui.*` 共 11 个用例、`--all` 全量 16 个用例全部通过，连续多轮结果一致。
    - **验收状态**：`T-1` ~ `T-9` 全部通过；其中 T-7 由人工在真实终端验证，其余各项由 `bin-tests` 用例或静态检索覆盖。

- [ ] **P1-016 WebUI 最小对话界面（进行中）**

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
    - **实现结论（代码已落地，验收未完成）**：Transport 在 `lingting-agent-ui/src/api/client.ts`（`AgentApiClient extends ApiClient`，负责拼地址、判定状态码、拆 `R<T>` 包装、把失败统一成 `ApiRequestError`；基地址留空 = 同源，开发期走 Vite 代理、发布态由 Rust 托管），SSE 消费在 `src/api/runStream.ts`（fetch 流式响应 + 自行切帧，path 取自 `ApiDefinitions.runStream.path`，不用 `EventSource`，理由见 D-007 的 P1-016 回填）。界面在 `src/pages/chat/`：`ChatMain`（布局 + 错误提示条 + 空状态）、`useChat`（会话创建 → `messageCreate` → 消费事件流 → 中止/取消的状态机）、`MessageList`（`@ant-design/x` 的 `Bubble.List`，流式气泡 `status=updating`、中断气泡 `abort`）、`Composer`（`Sender`，生成中显示停止按钮）、`chatMessage`（界面消息模型）。停止生成先调 `api/run/cancel` 再 `AbortController.abort()`（避免先断流导致取消请求撞上 400）；断线、`run_failed`、`server_shutdown` 都在回复末尾追加说明并把原因写进提示条。SDK 消费方式与目录约定见 D-010 的 P1-016 回填。
    - **服务端侧验收（已落地，2026-11-10）**：`bin-tests` 新增 `web` 领域 6 个用例，`make test ARGS="web"` 6/6、`--all` 22/22 通过 —— `web.assets`（首页与构建产物托管、缺失资源 404 文本）、`web.fallback`（前端路由回落 index.html，`api/` 与非 GET 仍为 API 404）、`web.cors`（白名单预检 204 + 实际请求 allow-origin / expose-headers，非白名单无 CORS 头）、`web.stream`（订阅已取消的 Run → 400 可读错误体，不是空流）、`web.shutdown`（关闭期间请求 → 503 `{code,message}`）、`web.sdk-only`（扫描前端 13 个源码文件的非注释行，无手写 `api/…` 字面量）。前置准备由用例声明驱动：`make test` 只保留 `gen`（编译期必需），`ui-build` 由 `bin-tests` 的 `prepare` 按选中的 `web.*` 用例执行一次。计划与逐步骤结论见 `.plan/p1-016-webui-tests.md`。
    - **前端逻辑验收（已落地，2026-11-10）**：`lingting-agent-ui/tests/` 下 4 个文件 21 条用例（`pnpm test` → `vp test run`），覆盖 SDK Transport（`R<T>` 解包、查询串与请求体、204、错误分类）、SSE 切帧（跨 chunk、粘包、CRLF、非法 JSON）、`useChat` 状态机（成功流式、会话复用、空白输入、停止生成、`server_shutdown`、事件流报错、建会话失败）与 `ChatMain` 首屏渲染；`bin-tests` 的 `web.ui` 用例转发同一命令，`make test ARGS="--all"` 23/23 通过。用例从 `vite-plus/test` 导入，未新增 vitest 依赖；新增 devDependencies 仅 `jsdom` 与 `@testing-library/react`。
    - **待验收**：真实浏览器里的验收（流式渲染观感、停止按钮交互、关服提示条）仍由计划中的步骤 4（Playwright）覆盖，尚未开始。

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
    3. 明确失败提示文案：指出“API 定义已变更，请运行 `make gen` 重新生成”（生成物不入库，不提示提交）。
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
