# P1 技术决策记录

每项记录包含：问题 / 背景 / 可选方案 / 推荐方案 / 决策理由 / 状态。

状态定义：

| 状态 | 含义 |
|------|------|
| `DECIDED` | 已确定，可直接执行 |
| `DECIDED (待验证)` | 方向已定，但依赖某个验证任务确认细节 |
| `OPEN` | 未确定，动手前必须讨论，或对应任务中必须先做实验再定 |
| `REJECTED` | 已否决（保留记录，不删除） |

**重要**：本文件区分「已核实的框架事实」与「推断/假设」。凡标注「已核实」的，均来自对 crate 源码或 README 的实际阅读；凡标注「假设」的，未经验证。

---

## 决策索引

| ID | 主题 | 状态 |
|----|------|------|
| D-001 | 服务端 API 定义方式 | DECIDED (已核实) |
| D-002 | TypeScript SDK 生成方式 | DECIDED (已核实) |
| D-003 | Rust Client SDK 生成方式（新增 `framework-proc-rust`） | DECIDED (待验证) |
| D-004 | API Schema 唯一来源与元数据获取方式 | DECIDED |
| D-005 | 代码生成时机 | DECIDED (P1-007 已固化) |
| D-006 | 生成物是否纳入版本控制 | DECIDED |
| D-007 | SSE 与 SDK 的类型化支持 | DECIDED (P1-011) |
| D-008 | 路径参数与 REST 风格路径 | DECIDED (已核实) |
| D-009 | WebUI 静态资源嵌入方案 | OPEN |
| D-010 | WebUI 技术栈 | OPEN |
| D-011 | TUI 与 Server 的连接管理 | OPEN |
| D-012 | Server 地址配置与发现 | DECIDED |
| D-013 | 优雅关闭实现方式 | OPEN |
| D-014 | 鉴权策略 | DECIDED (已实测) |
| D-015 | `framework-*` 依赖引入方式与版本矩阵 | DECIDED (已核实) |
| D-016 | 单 Binary 的功能裁剪与体积权衡 | OPEN |
| D-017 | P1 是否引入 SQLite | DECIDED |
| D-018 | `i64` 账号/ID 的跨语言表示 | DECIDED |
| D-019 | API 版本管理策略 | DECIDED |
| D-020 | P1 跨平台构建策略 | OPEN |
| D-021 | 参数提取与错误响应约束 | DECIDED (已实测) |
| D-022 | 固定端口分配 | DECIDED |
| D-023 | 日志目录与归档策略 | DECIDED |
| D-024 | 统一错误响应与错误码分配 | DECIDED |
| D-025 | P1 Mock 会话契约与 P2 替换点 | DECIDED |
| D-026 | P1 Streaming 契约（RunEvent + SSE） | DECIDED |

---

## D-001 服务端 API 定义方式

- **问题**：服务端 HTTP 接口用什么方式定义，才能同时产出运行时路由与可导出的 API 元数据？
- **背景**：已核实 `framework-proc-web` 提供 `web_api` / `web_api_get` / `web_api_post` / `web_api_put` / `web_api_patch` / `web_api_delete`，把 `async fn` 展开为 `WebRoute` 构造函数，并自动叠加 `framework-proc-ts::ts_api` 元数据收集。开启 `collect` 时额外生成 `push_web_api!`，供 `framework-web-axum` 的 `WebRouter` 自动收集路由。
- **可选方案**：
  1. 纯手工 Axum Router + 手写 TypeScript 类型（两份定义）。
  2. `framework-proc-web` 宏定义接口，框架收集路由与元数据。
  3. 手写 OpenAPI 文件 + 代码生成（服务端也由 spec 生成）。
- **推荐方案**：方案 2。
- **决策理由**：服务端代码即契约唯一来源，路由与元数据来自同一次宏展开，物理上不可能出现两份不一致的定义；方案 1 违反“禁止两套 API Schema”；方案 3 需要额外维护 spec 与实现的一致性，且服务端生成代码调试体验差。
- **代价**：受框架宏约束（见 D-008）；框架本身需要小幅扩展（见 D-003、D-013）。
- **P1-005 验证结论（回填）**：TS 侧直接用框架现成构建器（`TypescriptTypeBuilder` / `TypescriptEnumBuilder` / `TypescriptApiBuilder` / `PackageBuilder`），生成器为 `cargo run -p lib-web --example gen_ts_sdk`，产物落 `packages/sdk-ts`（不入库，pnpm workspace 消费）。生成物只含抽象类，Transport 由 WebUI 自实现。唯一未通过的验收项是 `tsc --noEmit`（本地未安装 typescript），见 `docs/reports/p1-005-ts-sdk.md`。
- **状态**：`DECIDED`

---

## D-002 TypeScript SDK 生成方式

- **问题**：WebUI 的 TypeScript SDK 如何生成？
- **背景**：已核实 `framework-proc-core`（需 `collect` feature）提供 `api_metadata_iter()`，以及三个 `TypescriptBuilder` 实现：
  - `TypescriptTypeBuilder`：基于 specta 导出 DTO 类型声明。
  - `TypescriptEnumBuilder`：导出 `<Name>All` 数组与 `<Name>Map`。
  - `TypescriptApiBuilder`：导出 `ApiDefinitions` 常量 + 类型工具 + 抽象类 `ApiClient`（`protected abstract call<T>(method, path, body?, query?): Promise<T>`）。
  - `PackageBuilder`：把上述结果写成 npm ESM 包（`package.json` + `dist/<name>.js|.d.ts` + `index.*`）。
  - 构建期校验：方法名 camelCase 后重复、`method + path` 重复、`class_name` 非法均返回错误。
- **可选方案**：
  1. 使用 `TypescriptApiBuilder` + `PackageBuilder`（框架现成能力）。
  2. 自研 TS 生成器（复制一份元数据遍历逻辑）。
  3. 生成 OpenAPI 再由第三方生成 TS 客户端。
- **推荐方案**：方案 1。
- **决策理由**：现成能力已覆盖 DTO、枚举、接口方法与类型校验，且与 Rust 侧共用同一份 `ApiMetadata`；方案 2 会引入第二套遍历/命名逻辑，长期必然漂移；方案 3 引入额外中间层与第三方质量依赖。
- **注意**：生成物只有**抽象类**，`call` 的具体实现（HTTP Transport）由 WebUI 自己写，这符合“允许客户端自实现 Transport，不允许自维护 API 定义”的原则。
- **状态**：`DECIDED`

---

## D-003 Rust Client SDK 生成方式（新增 `framework-proc-rust`）

- **问题**：CLI / TUI 需要 Rust 客户端，而框架目前**没有** Rust 代码生成能力，如何得到与 TS 同源同义的 Rust SDK？
- **背景（已核实）**：
  - `framework-proc-core` 的全部构建器只输出 TypeScript（`TypescriptBuildResult { js, dts }`）与 npm 包；全文没有任何 Rust 代码生成逻辑。
  - `framework-proc-*` 系列中不存在 `framework-proc-rust`。
  - 元数据 `ApiMetadata` 是 Rust 结构体，且通过 `inventory` 在进程内静态注册，因此**Rust 侧天然可读取完整元数据**，不需要任何中间格式（如 OpenAPI）。
- **可选方案**：
  1. **在 `lingting-rust-framework` 新增 `framework-proc-rust` crate**：读取 `api_metadata_iter()`，输出 Rust 客户端源码（trait + 默认方法 + DTO 引用）。
  2. 生成 OpenAPI，再用第三方 Rust 生成器（`openapi-generator` / `progenitor`）产出客户端。
  3. 手写 Rust 客户端（CLI 维护一份接口定义）。
  4. CLI 直接复用服务端的 DTO 与 handler 函数引用（不生成客户端，直接调用）。
- **推荐方案**：方案 1（本轮已确认方向）。
- **决策理由**：
  - 方案 3 直接违反“客户端不得自维护 API 定义”，排除。
  - 方案 4 破坏 Server/Client 边界（CLI 会 link 执行逻辑），排除。
  - 方案 2 需要先把 Rust 元数据降级成 OpenAPI 再复原成 Rust，中间丢失类型精度，且引入 Java 工具链或额外 proc-macro 依赖，与“Native Binary、工具链轻量”的目标冲突。
  - 方案 1 与 TS 生成器完全对称：同一 `api_metadata_iter()`、同一命名规则、同一校验规则，两份 SDK 不可能不同步。
- **设计约束（供 P1-006 落地）**：
  1. `framework-proc-rust` 是**纯代码生成 crate**，不含 proc macro，依赖 `framework-proc-core` 获取 `ApiMetadata`，形态与 `framework-proc-core` 的 `typescript` 模块平行（如提供 `RustApiBuilder` 实现一个 `RustBuilder` trait，`build()` 返回生成的 Rust 源码字符串）。
  2. 生成物形态：一个 `trait`（或结构体）内含 `call` 必需方法（覆盖 `method` / `path` / `body` / `query`），每个 API 生成一个默认方法；Rust 没有抽象类，用 trait 表达，具体 Transport 由 CLI 实现该 trait。
  3. 校验规则与 `TypescriptApiBuilder` 保持一致（方法名重复、`method + path` 重复报错），错误信息包含 `namespace::name`。
  4. DTO 处理需二选一并在 P1-006 结论中确定：**(a)** 生成物只输出方法签名并 `use` 服务端 DTO 类型（要求客户端依赖契约 crate）；**(b)** 生成物自带 DTO 定义。倾向 (a)，避免两份 DTO。
  5. 已知差异必须显式记录：TS 侧 `i64` 映射为 `string`，Rust 侧应为 `i64`；此差异属于类型系统差异，不是契约不一致，但需在 `acceptance.md` A-5 中说明。
  6. 若 `framework-proc-rust` 最终无法覆盖某类接口（例如流式），必须在本文档写明降级方案，不得静默放弃。
- **P1-006 验证结论（回填）**：
  1. **实现形态变更**：本地框架未新增 `framework-proc-rust` crate，而是把 Rust 导出并入 `framework-proc-core` 的 `rust` 特性，提供 `RustApiBuilder` / `RustTypeBuilder` / `RustEnumBuilder` / `CrateBuilder`；同时 `framework-proc-ts` 被上游删除。因此「新增 crate」这一条不再适用，生成器以 `lib-web` 的 example 形式驱动，最终形态归 P1-007。
  2. **API 形态**：`ApiDefinition` 常量表（name / namespace / method / path / arguments / return_type）+ `ApiClient` trait，必需方法为 `send(definition, body, query) -> Result<Vec<u8>, Self::Error>`，每个接口一个默认方法（参数聚合与 TS 的 `request_value` 一致）。返回值三态：`Void → ()`、`Blob → Vec<u8>`、`Type → 反序列化 DTO`。
  3. **DTO 策略选 (b)：自带快照定义**。理由：`auto_type` 已提供定义快照，`RustTypeBuilder` 可直接产出真实 DTO；而 (a) 需要客户端 path 依赖服务端契约 crate，`CrateBuilder` 不支持 path 依赖，且会引入「生成的 crate 先于 workspace 存在」的引导死锁。代价是 DTO 存在两份，靠同一份定义 + 生成保证一致。
  4. **消费方式**：生成物落 `packages/sdk-rust/src/`，不生成 `Cargo.toml`，由 `lib-bin-cli` 用 `#[path]` 作为自身模块引入（详见 D-006 落位）。客户端**不依赖 lib-web**，也不会 link 服务端执行路径。
  5. **已知差异**：`i64` Rust 侧保持 `i64` + `serde_as(DisplayFromStr)`，TS 侧为 `string`，wire 上双方一致（见 D-018）；`WebResponse`（`Blob`）两侧都只能拿到二进制体，自定义响应头丢失。
  6. **降级方案**：reqwest Transport 由客户端自行实现 `send`（不生成，不阻塞链路）；流式返回未建模，与 TS 侧一致（D-007 仍为 OPEN）；路径参数由框架层不支持，id 走 body/query（D-008）。
  7. 生成文件自带 `#![allow(warnings)]`，避免机器生成代码污染 `clippy -D warnings` 门禁。
  8. 框架缺陷一处已修：`framework-proc-core/src/rust/api_trait.rs` 的 `Blob` 分支缺 `Result` 包装导致生成代码无法编译。
- **状态**：`DECIDED (P1-006 已验证)` —— 详细证据见 `docs/reports/p1-006-rust-sdk.md`。

---

## D-004 API Schema 唯一来源与元数据获取方式

- **问题**：谁拥有 API 定义的唯一真相？生成器如何拿到元数据？
- **背景**：`inventory` 注册要求编译期链接，因此元数据只能在**进程内**迭代，不能在纯构建脚本里凭空获得。
- **可选方案**：
  1. 服务端 API crate（开启 `collect`）+ 独立生成器可执行程序（`cargo run -p <api-gen>` 或 `cargo xtask gen-sdk`）链接该 crate，遍历 `api_metadata_iter()`。
  2. `build.rs` 中生成（不可行：`build.rs` 无法链接下游业务 crate）。
  3. 服务端运行时额外提供 `GET /_api/metadata` 接口，由外部脚本抓取后生成（可行但引入运行期依赖，生成需要先启动服务器）。
- **推荐方案**：方案 1。
- **决策理由**：唯一来源是 Rust 源码；生成不依赖运行中的服务；生成器就是普通可执行程序，任何环境都能跑；方案 2 技术上不成立；方案 3 把“生成”绑定到“能启动服务器”，在 CI 与首次搭建时很别扭（可作为将来的调试辅助手段，不作为主链路）。
- **P1-007 回填**：唯一来源固定为 `crates/lib-web/src/` 的 `#[web_api_*]` handler 与 `#[auto_type]` / `#[auto_enum]` DTO；生成入口固定为根 `Makefile` 的 `make gen`；命名与流程规范写在 `docs/develop.md`「API 契约规范」。
- **状态**：`DECIDED (P1-007 已固化来源与入口)`

---

## D-005 代码生成时机

- **问题**：SDK 生成在构建期自动发生，还是作为独立命令？
- **可选方案**：
  1. 独立命令（`<gen 命令>`），构建过程本身不触发；CI 中额外跑一次一致性检查。
  2. Rust 构建期自动生成（`build.rs` / proc macro）。
  3. 前端构建期生成（Vite 插件）。
- **推荐方案**：方案 1。
- **决策理由**：方案 2 不可行（见 D-004）；方案 3 会把 Rust 元数据采集绑定到 Node 环境，且 Rust 侧仍需一次生成；方案 1 让“生成”成为显式、可重复、可 diff 的动作，配合 CI 门禁（P1-019）即可避免漏生成。
- **配套约束**：生成命令必须幂等；生成失败必须非 0 退出且不落半成品；CI 与本地用同一条命令。
- **P1-006 验证结论（回填）**：当前形态为「隔离在 `lib-web` 的 example（`gen_rust_sdk` / `gen_ts_sdk`）顺序执行」，生成物不入库（D-006）。`build.rs` 方案已实测不可行：放在 `lib-web` 会自引用循环，放在 `lib-bin-cli` 且产物被 path 依赖时会造成 workspace 加载死锁。
- **P1-007 回填**：命令入口固化为根 `Makefile` 的 `make gen`（不再使用 `scripts/*.sh`，`scripts/` 已删除）；生成器形态仍是 `lib-web` 的 example，不新增独立 crate。失败行为落实为「全部校验与源码构建先于写盘完成；写盘先落同级暂存目录 `.gen-tmp`，成功后整体替换目标目录」，失败以非 0 退出且保留上一次产物。
- **状态**：`DECIDED (P1-006 已验证；P1-007 已固化命令入口与失败行为)`

---

## D-006 生成物是否纳入版本控制

- **问题**：生成的 TS 包与 Rust 客户端源码是否提交到仓库？
- **可选方案**：
  1. 入库（提交生成物），CI 校验“重新生成无 diff”。（原方案，已废弃）
  2. 不入库，生成物仅存在于本地构建产物与工作站内，随生成命令重建。
- **推荐方案**：方案 2（P1-006 修订）。
- **决策理由**：生成物完全可由服务端 API 定义重现，来源只有 Rust 定义一处；入库会让每次 API 变更都附带大段机器生成 diff，掩盖真实改动。
- **落位**：
  - Rust Client SDK 由 `cargo run -p lib-web --example gen_rust_sdk` 写入 `packages/sdk-rust/src/`，且**不生成 Cargo.toml**：`lib-bin-cli` 用 `#[path = "../../../packages/sdk-rust/src/*.rs"]` 把生成文件当作自己的模块引入，避免「path 依赖的 manifest 先于生成存在」的引导死锁。
  - TypeScript SDK 由 `cargo run -p lib-web --example gen_ts_sdk` 写入 `packages/sdk-ts/`（pnpm workspace 内消费）。
  - `packages/` 整体被 `.gitignore` 忽略；`make gen` 一条命令产出两份 SDK，且必须在 `make build` / `make ui-install` 之前执行。
- **配套约束**：生成文件头部必须带“自动生成，勿手改”；生成失败必须非 0 退出且不落半成品；不得手工修改生成物，改 API 定义后重新生成；**不存在“提交生成物”步骤**，API 变更的提交只包含定义与消费方代码。
- **P1-007 回填**：命令入口由 `scripts/gen.sh` 改为根 `Makefile` 的 `make gen`；生成物落位不变，仍不入库；`tasks.md` 中“改定义 → 跑生成 → 提交生成物”的表述已修正为“改定义 → 跑生成”，不再有提交生成物的环节。
- **状态**：`DECIDED (已修订为不入库；P1-007 已固化命令入口)`

---

## D-007 SSE 与 SDK 的类型化支持

- **问题**：流式响应如何与生成 SDK 共存？事件类型如何跨语言共享？
- **背景（已核实）**：
  - `framework-web` 的 `WebResponse` 支持 `WebBody::Stream`（`BoxStream<'static, Result<Bytes, std::io::Error>>`），并有 `WebResponse::stream(status, content_type, stream)` 构造器，“用于 SSE 等持续输出场景”。
  - 但 `framework-proc-ts::ts_api` 的返回值映射只有三种：省略/`()` → `Void`，`WebResponse` → `Blob`，其他 → `Type(name)`。**没有流式语义**，SSE 接口在元数据里只会表现为 `Blob`。
  - 现有生成器（TS / 待做的 Rust）都不会为 `Blob` 生成类型化事件流方法。
- **可选方案**：
  1. SSE 端点仍用 `web_api_post` 定义（`WebResponse` 返回），但**不进 SDK 的类型化方法**；由客户端各自实现 SSE 消费（原生 `EventSource` / fetch stream / reqwest + 手写解析），事件类型通过服务端导出的枚举（TS 生成 + Rust 生成）共享。
  2. 扩展框架元数据：新增 `ApiReturnType::Stream(EventType)`，并让 TS/Rust 生成器输出流式方法。
  3. SSE 端点完全绕开 `web_api` 宏，用原生 Axum 路由手写（脱离元数据体系）。
- **推荐方案**：P1 采用方案 1；把方案 2 作为框架演进项，在 P2 前评估。
- **决策理由**：方案 3 会破坏“路由与元数据来自同一次定义”的一致性，且需为鉴权、trace、错误处理做重复实现；方案 2 是正确终局，但需要同时改 `framework-proc-ts` 元数据枚举、`TypescriptApiBuilder` 与新的 `framework-proc-rust`，属于跨仓库的协议级改动，放进 P1 会拖住主线。方案 1 能在 P1 内跑通全链路，并把“事件契约”保持在单一来源（服务端事件枚举 → 生成两侧类型）。
- **P1 必须接受的不完美**：SSE 路径的 path 仍需人工出现在客户端代码里（因元数据不表达流式）。**缓解**：让 SSE 端点 path 也由生成物导出为常量（元数据里它已是 `Blob` 接口，可导出其 path），客户端不手写字面量。
- **P1-011 定案**：采用方案 1。缓解手段已验证 —— 生成物导出 `RUN_STREAM.path`（Rust）与 `ApiDefinitions.runStream.path`（TS），客户端不手写 path 字面量；事件类型来自生成的 `RunEvent` / `RunEventKind`。方案 2 仍是正确终局，留作框架演进项（P2 前评估）。具体事件契约与断线语义见 D-026。
- **状态**：`DECIDED`（P1-011）

---

## D-008 路径参数与 REST 风格路径

- **问题**：能否使用 `/api/sessions/{id}/messages` 这类路径？
- **背景（已核实）**：
  - `web_api` 宏的 `path` 参数只接受字符串，两端 `/` 被裁掉；`framework-web-axum` 的 `WebRouter` 按 `method + path` **精确字符串匹配**查找路由。
  - README 明确：“路径不做前缀拼接与参数占位符解析”。
  - 另外：GET 接口最多只能有一个参数对象；参数类型必须是 `Json<T>`、`Query<T>` 或 `PaginationParams`。
- **可选方案**：
  1. 资源标识全部走 body（POST）或 query（GET），路径只用静态段。
  2. 自行在服务端做路径匹配（绕开 `WebRouter`），手写 Axum 路由。
  3. 扩展框架支持占位符与路径参数提取。
- **推荐方案**：方案 1。
- **决策理由**：方案 2 放弃路由收集与统一 trace/错误处理，代价大于收益；方案 3 属于框架能力扩展，P1 不必承担。方案 1 的代价是 API 风格偏 RPC，但一致性好、生成器无需理解路径参数。
- **落地约定**：路径采用 `api/<资源>/<动作>` 形式（如 `api/session/create`、`api/session/messages`），资源 id 通过 body/query 传递。
- **P1-004 实测补充（详见 `docs/reports/p1-004-api-probe.md`）**：
  - 精确匹配已确认：`/api/probe/void/` 与 `/api/probe/void` 等价；方法与路径不匹配都返回 404，没有 405。
  - 关闭 `collect` 后手动挂载在本框架版本**不可行**：`WebRouter` 没有插入路由的 API，宏生成的 `build_<fn>_route` 是 `pub(crate)`，且 `framework-web-axum` 在未开 `collect` 时自身无法编译。P1 固定开启 `collect`，并把“路由收集 + 链接锚点”收在 `lib_web::web_routes()`。
- **状态**：`DECIDED`

---

## D-009 WebUI 静态资源嵌入方案

- **问题**：如何把 Vite 构建产物嵌入 Rust binary？
- **可选方案**：
  1. `rust-embed`：`#[derive(RustEmbed)]` + 目录，debug 模式可读磁盘（天然支持开发热更新）。
  2. `include_dir`：纯 `include_bytes!` 展开，编译期全量嵌入，无 debug/磁盘回退。
  3. 构建期生成 Rust 源码（自研 codegen，把文件写成 `&[u8]` 常量）。
- **推荐方案**：方案 1。
- **决策理由**：debug 与 release 行为差异正好满足“开发指向本地目录、发布嵌入”的双模式需求；方案 2 在开发时需要重新编译 Rust，体验差；方案 3 是重复制造 `rust-embed`。
- **待确认**：`rust-embed` 与 `framework-web` 的 `WebResponse` 集成是否需要额外的 MIME 推断逻辑（`WebResponse` 无内建 MIME 表）。
- **状态**：`OPEN`

---

## D-010 WebUI 技术栈

- **问题**：WebUI 用什么框架与构建工具？
- **可选方案**：
  1. React + Vite + TypeScript。
  2. Vue + Vite + TypeScript。
  3. Svelte / Solid + Vite。
  4. 无框架（原生 TS + 少量 DOM 操作）。
- **推荐方案**：方案 1（React + Vite + TS）。
- **决策理由**：P1 只需要一个最小对话界面，四种方案都能胜任；选 React 是为了后续复杂交互（多会话、工具调用可视化、设置面板）时有最大生态。方案 4 在 P2 之后会迅速失控。
- **约束（脚手架落地后修订）**：UI 组件库使用 antd 与 `lingting-react-ui`；路由、请求缓存与轻量状态分别使用 `@tanstack/react-router` / `@tanstack/react-query` / `@tanstack/react-store`，不再限定「原生 hooks、不引入额外状态库」。
- **状态**：`DECIDED`（技术栈：React + Vite + TypeScript + antd/LRI；工程目录 `lingting-agent-ui/`，同时是 pnpm workspace 根）
- **备注（裁决后回填）**：前端脚手架 `lingting-agent-ui/` 已落地，依赖含 antd、`lingting-react-ui` 与 `@tanstack/*` 系列，与原「不引入重型组件库、不引入额外状态库」的约束冲突；已裁决**保留当前依赖组合**，约束按本节修订条目执行。

---

## D-011 TUI 与 Server 的连接管理

- **问题**：TUI 如何维持连接、如何断线重连、如何取消流？
- **可选方案**：
  1. 单一长连接 SSE + 独立 HTTP 请求（事件流与命令请求分离）；断线后手动/自动重连，重连不带断点续传。
  2. 全双工 WebSocket（需要额外协议与服务端支持）。
  3. 轮询（简单但延迟高、浪费请求）。
- **推荐方案**：方案 1。
- **决策理由**：与 WebUI 保持同一套通信形态（HTTP + SSE），避免为 TUI 单独引入 WebSocket 协议与鉴权路径；P1 无持久化，断点续传本来也无意义。
- **待定细节（P1-015 决定）**：重连是否自动、重连时如何呈现“可能丢失的中间 token”、`Esc` 取消的时序（先发取消请求还是先关流）。
- **状态**：`OPEN`

---

## D-012 Server 地址配置与发现

- **问题**：客户端如何知道 Server 地址？
- **背景**：`framework-web-axum` 的 `axum_builder(addr, port)` 在端口 ≤ 0 或越界时使用**随机端口**，这会破坏客户端默认寻址。
- **可选方案**：
  1. 固定优先级：命令行 > 环境变量 `AGENT_SERVER_ADDR` > 配置文件 `[server] addr` > 默认 `http://127.0.0.1:20100`（见 D-022）。
  2. 仅环境变量 + 命令行。
  3. 端口文件（server 启动写 `<config_dir>/agent/server.port`，client 读取）。
- **推荐方案**：方案 1 为主，方案 3 作为将来“多实例/随机端口”时的扩展，P1 只预留不实现。
- **决策理由**：方案 1 对开发者与最终用户都直观；方案 2 缺少持久化默认值，用户每次都要传参；方案 3 在 P1 无多实例需求时属于过度设计。
- **配套约束**：`agent server` 不得静默使用随机端口；端口冲突必须报错退出。
- **P1-008 落地（目录与配置文件）**：
  1. 应用根目录取 `framework-core` 的 `ApplicationDirectory::user_in("agent", ".lingting")`，即 Windows `%USERPROFILE%\.lingting\agent`、Linux/macOS `$HOME/.lingting/agent`；debug 构建取可执行文件同级的 `runtime` 目录。不使用 `%APPDATA%`。
  2. 配置文件为 `<应用根目录>/config.toml`（`lib-core` 的 `AppDirectories::config_file`），只存路径、不预创建；数据目录为 `<应用根目录>/data`，P2 的 SQLite 预期落该目录，与配置文件不同层级。
  3. 服务端专属环境变量：`AGENT_SERVER_ADDR`（接受 `host`、`host:port`、`http://host:port`）、`AGENT_SERVER_PORT`、`AGENT_LOG_LEVEL`；配置文件键为 `[server] addr` / `port` / `log_level`；优先级仍是命令行 > 环境变量 > 配置文件 > 默认值，默认 `127.0.0.1:20100`。
  4. 日志目录与配置/数据同级：`logs`（服务端）、`logs-cli`、`logs-tui`（见 D-023）。
- **状态**：`DECIDED`（优先级、目录命名与配置文件位置均已定）

---

## D-013 优雅关闭实现方式

- **问题**：如何在 `framework-web-axum` 上实现优雅关闭？
- **背景（已核实）**：`framework-web-axum` 公开 API 只有 `axum_builder(...)`、`AxumBuilder::with_cors`、`bind()`、`AxumServer::context()`、`router()`、`run(wrapper)`；`run` 持续运行且“重复调用返回错误”，**没有任何 shutdown 信号或 handle 的入口**。源码中无 shutdown 相关实现。
- **可选方案**：
  1. 向上游 `lingting-rust-framework` 增加 `run_with_shutdown(shutdown_future)`（或返回 `AxumServerHandle` 用于关闭），本仓库直接使用。
  2. 本仓库绕开 `AxumServer::run`，自行调用 `axum::serve(...).with_graceful_shutdown(...)`。**已核实**：`build_router_with` 是 `pub(crate)`，公开的 `build_router(wrapper)` 只返回框架自有 `WebRouter`（不是 axum `Router`），因此该方案同样要求上游先暴露入口，否则需要在本仓库复制 dispatch 逻辑。
  3. P1 接受非优雅关闭（Ctrl+C 直接退出）。
- **推荐方案**：方案 1；若上游改动周期不可控，退化为方案 2。
- **决策理由**：方案 1 是“正确修在框架里”，其他使用该框架的项目也会受益，且不需要在本仓库复制服务启动逻辑；方案 2 会分散关停职责（路由由框架组装、serve 由本仓库启动），埋下行为差异；方案 3 不满足 P1 验收 S-8/S-9。
- **待定**：上游改动是否在 P1 时间盒内完成；若走方案 2，需确认 `build_router_with` 与 `AxumServer` 上下文（`AxumContext`）的一致性。
- **状态**：`OPEN`

---

## D-014 鉴权策略

- **问题**：P1 是否引入认证？`web_api` 的 `auth` 参数默认是 `AuthRule::login()`。
- **背景（已核实）**：`web_api` 宏的 `auth` 默认值为 `framework_web::AuthRule::login()`；本地单用户场景没有登录体系。
- **可选方案**：
  1. 所有接口显式声明 `AuthRule::anonymous()`，鉴权整体留到后续阶段（本地 single-user）。
  2. P1 引入本地 token（server 启动生成，写入配置文件，客户端读取）。
  3. 保留 `login()` 默认值并实现一个本地“永远成功”的登录上下文。
- **推荐方案**：方案 1，同时把“未来引入 token 时的改动面”记录下来（需要中间件 + 客户端配置项）。
- **决策理由**：P1 目标是架构闭环，不是安全模型；方案 2 会牵动 SDK（需要 header 注入约定）与配置格式；方案 3 是伪认证，容易误导后续开发。
- **风险**：默认 `login()` 未显式覆盖时，本地调用会被拒绝且错误信息可能晦涩——P1-004 必须实测并记录。
- **实测结论（P1-004）**：默认 `login()` 的接口在没有任何身份信息时正常返回 200——`framework-web-axum` 的默认路由包装器直接 `route.invoke()`，不读取 `auth`，只有自定义 `WebRouteWrapper` 才会校验。“默认值导致本地调用被拒绝且报错晦涩”的风险不成立。
- **状态**：`DECIDED`（方案 1；是否显式写 `AuthRule::anonymous()` 不影响功能，正式 API 统一显式声明，鉴权留到后续阶段）

---

## D-015 `framework-*` 依赖引入方式与版本矩阵

- **问题**：P1 如何引入 `framework-*`，以及 P1 所需依赖各自锁定到哪个版本？
- **背景（已核实）**：
  - 本地源码仓库 `D:/code/lingting/lingting-rust-framework` 为 workspace 统一版本 **26.11.0**，`edition 2024`、`rust-version 1.98.0`。
  - registry 发布状态（已按 crates.io 稀疏索引核实）：`framework-core`、`framework-proc-core`、`framework-proc-ts`、`framework-proc-web`、`framework-web`、`framework-web-axum` **均已发布 26.11.0**，且 feature 定义（`collect` 链）与本地源码一致。本机 cargo 缓存对 web 三件套只到 26.8.150，属缓存滞后，不代表未发布。
  - `framework-proc-core` 硬约束 `specta = "=2.0.0-rc.25"`、`specta-typescript = "0.0.12"`、`specta-serde = "0.0.12"`。
  - P1-003 已在本地源码上完成复核：`path` 引入后整个 workspace 可编译，`Cargo.lock` 中全部 `framework-*` 均为唯一版本 26.11.0，`cargo tree -d` 无框架 crate 重复。
- **可选方案**：
  1. 全部走 `path` 依赖本地 `lingting-rust-framework`（版本必然一致，改动即时可见）。
  2. 全部走 registry 26.11.0。
  3. 混用（明确 REJECTED：正是要避免的情况）。
- **决策**：方案 1 —— 全部 `path` 引入，**当前不写 `version` 约束**；等 `framework-proc-rust` 开发完成、P1 期间对框架的改动收敛后，再统一补上 `version = "<当时框架版本>"` 作为防漂移约束（本地源码版本被改动而未更新本仓库声明时，`cargo build` 立即报版本不匹配，而不是静默使用新版本）。
- **决策理由**：
  - P1 需要向框架提交两处改动（D-003 新增 `framework-proc-rust`、D-013 补齐优雅关闭入口），`path` 依赖是唯一能让改动即时生效、且改动与消费方在同一次 `cargo build` 中被验证的方式。
  - registry 26.11.0 虽已全量发布，但一旦 P1 期间改动框架，registry 版本就必然滞后，届时仍需切回 `path`，来回切换的代价高于直接使用 `path`。
  - 混用会造成 `inventory` 注册的元数据类型跨版本分裂（不同 `framework-proc-core` 版本 → 两套注册表）。
  - 现在不写 `version` 的理由：P1 期间框架仓库会持续新增/修改 crate（先 `framework-proc-rust`，再优雅关闭入口），每次框架侧版本推进都要同步改本仓库的 `version` 声明，属纯噪音；等改动收敛后一次性补上，防漂移能力不失，维护成本最低。
- **反例警示**：不同版本的 `framework-proc-core` 同时进入依赖图会导致 `ApiMetadata` 类型不统一、注册表分裂，表现为“接口定义了但生成器看不到”。
- **P1-006 框架变更（回填）**：本地框架在 26.11.0 之后的提交中**删除了 `framework-proc-ts` crate**，并把 TS / Rust 导出统一并入 `framework-proc-core` 的 `ts` / `rust` 特性（`framework-web` 也提供 `ts` / `rust` 转发特性）。因此本项目移除了 `framework-proc-ts` 依赖；`lib-web` 现声明 `framework-proc-core = { features = ["collect", "ts", "rust"] }`。本文档其他位置出现的 `framework-proc-ts` 描述属 26.11.0 时期的历史记录，不再代表当前依赖。

### P1 依赖版本矩阵

| 依赖 | 锁定写法 | 为什么是它 |
|---|---|---|
| `framework-*`（core / web / web-axum / proc-*） | `path`（暂不带 `version`，`framework-proc-rust` 完成后补） | 见上：改动需即时生效；`version` 约束推迟到框架改动收敛，避免每轮框架改动都要同步本仓库声明 |
| `axum` | `0.8` | 必须与 `framework-web-axum` 内部依赖同一大版本，否则同一次构建会出现两个 axum（框架不支持 0.7，0.9 尚未发布） |
| `tokio` | `1.52`（`rt-multi-thread`、`sync`、`time`、`macros`；`lib-bin-web` 额外开 `net`） | 与框架 workspace 对齐；`net` 供 `lib-bin-web` 自建运行时启用 IO driver（`axum::serve` 需要），其余 crate 不重复声明 |
| `clap` | `4.6`（`derive`） | 4.x 当前维护线；P1 只用 derive 定义子命令，`env` 等 feature 等真正需要时再开 |
| `toml` | `0.9` | 解析 `<应用根目录>/config.toml`（D-012），只做 serde 反序列化；`serde_json` 无法覆盖 TOML 语法 |
| `reqwest` | `0.13`（`json`、`stream`） | 客户端 Transport 用；`stream` 是 SSE 增量读取的前提；默认 `default-tls` 已是 rustls，与本机其他 Rust 项目保持一致，避免同机多版本 |
| `ratatui` | `0.30` | 0.30 是当前稳定线，默认 feature 已含 crossterm 后端，TUI 不额外引后端 crate |
| `serde` | `1.0`（`derive`） | 与框架 workspace 一致，DTO 序列化与框架元数据共用 |
| `serde_json` | `1.0` | 与 `framework-proc-core` 依赖的版本一致，避免 JSON 类型跨版本 |
| `specta` | `=2.0.0-rc.25` | `framework-proc-core` 的硬约束（等号锁定）；换成 rc.28 会与框架的类型注册表不兼容 |
| `specta-typescript` | `0.0.12` | 同上，框架硬约束；0.0.15 与框架使用的导出 API 不兼容 |
| `specta-serde` | `0.0.12` | 框架硬约束，仅经 `framework-proc-core` 传递使用 |

约束语义：除 `specta` 用等号外，其余都是**下限约束**（caret），`Cargo.lock` 的实际解析结果是 `axum 0.8.9`、`tokio 1.54.1`、`clap 4.6.7`、`reqwest 0.13.5`、`ratatui 0.30.2`。P1 不写 `=x.y.z` 的理由：框架侧同样使用下限约束，钉死会与框架的依赖期望无谓冲突，且 patch 级升级不应要求改代码；真正的可复现性由 `Cargo.lock` 入库保证。

未在 P1-003 锁定的项：`sqlx`（P2 才用，保留在 workspace 依赖中但无消费方）、`rust-embed`（`lib-bin-web` 的 `embed-ui` feature，P1-016 再定）、`framework-proc-rust`（P1-006 新增后按同一规则以 `path` + `version` 引入）。

### Feature 传递结论（已核实源码）

- `framework-web` 的 `collect = ["dep:inventory", "framework-proc-web/collect"]`：同时打开路由注册表与 `web_api_*` 宏的 `push_web_api!` 展开。
- `framework-web-axum` 的 `default = ["collect"]`，经它引入 `framework-web` 时 `collect` 自动成立。
- `framework-proc-ts` 的 `collect = ["framework-proc-core/collect"]`，本身不改变宏展开，只是转发开关。
- **关键**：`framework-proc-ts::ts_api` 的展开**无条件**输出 `::framework_proc_core::push_api_metadata!`，而该宏与 `framework_proc_core::__private::inventory` 只在 `framework-proc-core/collect` 下存在 → 定义 API 的 crate 若没开 `framework-proc-core/collect`，是**编译错误**，不是“生成器看不到”。
- 宏展开使用绝对路径 `::framework_web` / `::framework_proc_ts` / `::framework_proc_core`，因此定义 API 的 crate 必须把三者声明为**直接依赖**，只靠传递依赖无法通过编译。
- 结论（谁显式开 collect）：
  - 服务端 API crate（P1 为 `lib-web`）：显式开启 `framework-web`、`framework-web-axum`、`framework-proc-ts`、`framework-proc-core` 的 `collect`。
  - 生成器 crate（P1-007 定形）：只需 `framework-proc-core` 的 `collect`（提供 `api_metadata_iter()`），并正常 link 定义 API 的 crate。
- 复核方式：`cargo tree -e features -p lib-web -i framework-proc-core` 可见 `framework-proc-core feature "collect"` 由 `lib-web` 与 `framework-proc-ts/collect` 共同开启；`framework-web feature "collect"` 由 `lib-web` 与 `framework-web-axum/collect` 共同开启。

### `inventory` 注册机制的副作用

1. 注册是**静态 trait-object 提交**（`inventory::submit!`），在链接期收集：元数据只在“定义 API 的 crate 真正被 link 进当前二进制”时存在。仅 `cargo build` 服务端不会产出元数据表，生成器必须链接服务端 API crate。
2. `collect` 关闭时注册表整体不存在：`push_web_api!` 与 `api_metadata_iter()` 都消失，路由需要手工挂载，生成器无从读取。
3. `framework-proc-auto/collect` 目前**未开启**（P1-003 范围外，属 `types-core` 定义 DTO 时的事）：`#[auto_type]` / `#[auto_enum]` 展开不产出类型元数据。P1-010 定义 DTO 时必须为 `types-core` 开启，否则 TS 侧只有方法签名、没有 DTO 类型。
4. feature 合并的连带效果：一旦开启 `framework-proc-auto/collect`，`framework-core` 自身用 `#[auto_type]` 声明的类型（如 `R`）也会注册进同一张表。生成器必须按 `ApiMetadata.namespace`（`module_path!()`）过滤，不能假设表里只有本项目的类型。
5. `collect` 是**编译期**开关，切开会改变产出的二进制内容；因此服务端、生成器、测试都要基于同一份 feature 组合构建，否则会出现“同一个接口在不同二进制里可见性不同”。

### 切回 registry 的条件

同时满足以下四条才切换（切换是一次独立改动，需重跑 `cargo build` 与 `cargo tree -d`）：

1. `framework-proc-rust`（D-003）与优雅关闭入口（D-013）的改动已合入框架并发布；
2. registry 上相关 `framework-*` 的 `collect` feature 链与本地源码一致；
3. 全部 `framework-*` 改为 `version = "<已发布版本>"` 后，`cargo tree -d` 中无框架 crate 重复；
4. 生成器与服务端使用同一来源（不允许部分 `path`、部分 registry）。

在切回 registry 之前，还须先按上文补齐本地源码期间应有的 `version` 约束并跑通构建 —— 即“先 `path` + `version`，再换成纯 `version`”，避免版本事实只存在于脑记里。

### 已知遗留

- `cargo tree -d` 中第三方重复不属于本次对齐目标：`syn 2.0.119` 与 `syn 3.0.9` 并存（不同 proc-macro 生态），`bytes` / `serde_core` / `serde_json` / `memchr` 的“重复”是 host（proc-macro/构建脚本）与 target 两套构建上下文，同版本不同上下文，不影响契约类型一致性。
- 外部 `path` 依赖的编译警告（当前为 `framework-proc-auto` 的 3 条 `dead_code`）会出现在本仓库的构建输出里，且**无法在本仓库侧按 crate 静音**：Cargo 只对 registry / git 依赖自动加 `--cap-lints allow`，`path` 依赖一律按本地处理；`cargo clippy -- <flag>` 的额外参数也只作用于本仓库的目标。可选做法只有两种 —— 在 `lingting-rust-framework` 里修掉死代码（推荐），或在本仓库用全局 `RUSTFLAGS` / `--cap-lints allow` 一刀切（会连带削弱本仓库自己的 lint 基线，不采用）。因此 P1-002 的“`cargo clippy` 零 warning”基线只对本仓库成员成立。

- **状态**：`DECIDED`

---

## D-016 单 Binary 的功能裁剪与体积权衡

- **问题**：一个 binary 同时含 server、TUI、WebUI 资源，是否需要 feature 裁剪？
- **可选方案**：
  1. 单一 feature 集，不裁剪（最小实现，binary 较大）。
  2. 通过 Cargo feature 区分 `cli-only` / `server-only` / `full`，发布时可选。
  3. 拆成三个 binary（`agent` / `agent-server` / `agent-cli`）——**违反“单 Binary”目标，REJECTED**。
- **推荐方案**：P1 用方案 1；若体积超标（例如 > 30 MB）再评估方案 2。
- **决策理由**：P1 优先保证“一个文件能跑通”，feature 矩阵会让 CI 与文档复杂度翻倍。方案 2 保留为 P6（发布）的优化项。
- **待定**：体积阈值与判断依据（P1-018 记录基线数据后决定）。
- **状态**：`OPEN`

---

## D-017 P1 是否引入 SQLite

- **问题**：P1 是否需要持久化？
- **可选方案**：
  1. 不引入，内存 Mock 存储，进程重启丢失。
  2. 引入 SQLite，Session/Message 落盘。
- **推荐方案**：方案 1。
- **决策理由**：P1 的验收重点是通信边界、API 契约与 SDK 生成；引入存储会把范围扩大到 schema 设计、迁移、并发与事务，且 P2 无论如何都要重做 Repository 层。方案 1 与 `p1/README.md` 「Mock 必须与真实契约同构」的要求不冲突。
- **状态**：`DECIDED`

---

## D-018 `i64` 账号/ID 的跨语言表示

- **问题**：DTO 中的 `i64`（如会话 ID、消息 ID）在 TS 侧如何表示？
- **背景（已核实）**：`framework-proc-ts` 的类型映射把 `i64`、`u64`、`i128`、`u128` 映射为 TypeScript `string`；README 明确指出该映射与 `auto_type` 对同名字段的 `DisplayFromStr` 处理一致，**裸 `i64` 返回值会被标注成 `string` 但 serde 实际输出数字**，建议用 `auto_type` 结构体包装。
- **可选方案**：
  1. ID 一律用 `String`（如 UUID / 雪花 ID 字符串），彻底规避跨语言数值精度问题。
  2. ID 用 `i64`，DTO 字段用 `auto_type` 包装，让 TS 侧确实是 `string` 且运行时输出字符串。
  3. ID 用 `i64` 裸字段（REJECTED：类型标注与运行时不一致）。
- **推荐方案**：P1 采用方案 1；若后续需要数据库自增主键，再在 DTO 边界转为字符串（方案 2 作为备选）。
- **决策理由**：方案 1 让 TS / Rust / SQLite 三侧表示一致，且不需要记住“哪些字段必须 `auto_type`”这一隐性规则；方案 2 正确但增加了“忘记包装就出错”的陷阱。P1 的 Mock 阶段正好可以低成本确立这个约定。
- **状态**：`DECIDED`（P1-005 与 P1-010 执行时若发现与框架冲突，回填变更）
- **P1-005 / P1-006 实测回填**：`auto_type` 的 `i64` 字段在 TS 生成物里是 `string`、在 Rust 生成物里仍是 `i64` + `#[serde_as(as = "DisplayFromStr")]`，两侧 wire 上都是字符串，因此“标注与运行时一致”在方案 2 下成立。探针 DTO 故意保留 `i64`（`ProbeEchoPO.id`、`ProbeQueryPO.size` 等）用于验证该映射；**正式业务 DTO 仍按本决策使用 `String`**。

---

## D-019 API 版本管理策略

- **问题**：API 版本如何表达？
- **可选方案**：
  1. path 前缀（`/api/v1/...`）。
  2. 请求头（`x-api-version`）。
  3. P1 不做版本化，`/api/...` 直接演进。
- **决策**：P1 采用方案 3 —— 不做版本化，path 保持 `api/...`，**不预留** `/api/v1`。
- **决策理由**：本地 single-user 阶段没有并行版本需求；预留无处理器的 `v1` 段会让所有 path、前端代理与探针用例都背上一个当期无收益的常量。将来确实需要多版本并存时再用方案 1（path 前缀，如 `api/v2/...`）并整体重排一次，成本主要是机械改动；方案 2（请求头）在 SSE 与静态资源场景更麻烦，不作为备选。
- **P1-007 回填**：P1 的探针接口全部是 `api/probe/...`，保持不变（P1-009 起共 11 条，见 D-024）；`docs/develop.md`「API 契约规范」已记录“path 不带版本段”的现行约定。
- **状态**：`DECIDED`

---

## D-020 P1 跨平台构建策略

- **问题**：P1 需要支持哪些平台，如何构建？
- **可选方案**：
  1. 仅 Windows（本机平台），其他平台留到 P6。
  2. Windows + Linux 通过 CI 矩阵构建（原生 runner，不做交叉编译）。
  3. 交叉编译（`cargo-zigbuild`，本机已安装）。
- **推荐方案**：方案 1 做验收，方案 2 作为 CI 目标（不阻塞 P1 验收）。
- **决策理由**：P1 的验收标准要求“Release Binary 可独立运行”，本机 Windows 即可验证；方案 3 在引入 `rust-embed`、SQLite（P2）后会遇到链接与原生依赖问题，不适合在 P1 就承担。
- **状态**：`OPEN`（确认 Linux 是否属于 P1 交付范围）

---

## D-021 参数提取与错误响应约束

- **问题**：`web_api` 的参数提取与错误响应在实际运行中约束到什么程度，P1 的 API 定义该怎么写？
- **背景（P1-004 实测，详见 `docs/reports/p1-004-api-probe.md`）**：
  - 查询参数（`Query<T>`）由 `query_json()` 组装，所有值都是 JSON 字符串：`String` 可用；`i64` 因 `auto_type` 注入 `DisplayFromStr` 可用；`bool` 不可用，返回 400 `查询参数转换失败`；查询键名大小写敏感且必须与 serde 名（camelCase）一致，否则被静默忽略。
  - 请求体（`Json<T>`）不校验 `content-type`；未知字段静默忽略（未启用 `deny_unknown_fields`）；`i64` 传 JSON 数字直接 400。
  - 错误响应固定为 `{"code": <HTTP 状态码>, "message": "<对外消息>"}`：参数类错误的 message 是固定文案（`请求参数转换失败` / `查询参数转换失败` / `请求体不是有效的 JSON`），不含字段名；业务侧 `WebError::parameter(msg, ...)` 的 msg 原样返回。
  - 路由不存在与方法不匹配都返回 404 `请求接口不存在`。
- **落地约定**：
  1. GET 的查询 DTO 只放字符串类字段；布尔与数值入参用字符串表达，或在 DTO 上自定义 `deserialize_with`。
  2. 对外 DTO 一律沿用 `auto_type`，`i64` 以字符串传输（与 D-018 一致），客户端不得发送数字。
  3. 客户端只依赖 `code` 做分支，不得依赖参数错误的 `message` 文案。
  4. 每个消费路由或元数据的二进制都必须引用 `lib_web::web_routes()` 作为链接锚点，否则 `inventory` 注册项被链接器丢弃、接口全部 404（P1-004 实测：未引用时收集到 0 条路由，引用后 7 条）。
- **状态**：`DECIDED`

---

## D-022 固定端口分配

- **问题**：项目各进程使用哪些端口？
- **决策**：全部固定，不使用随机端口。长期维护的端口表见 `docs/develop.md`。

| 服务 | 端口 | 说明 |
|------|------|------|
| lingting-agent | 20100 | 服务端端口 |
| 开发环境 WebUI | 20101 | 开发环境 WebUI 端口 |
| 开发环境服务端 | 20102 | 开发环境服务端端口 |
| API 探针服务端 | 20103 | API 探针服务端端口 |

- **决策理由**：固定端口让客户端默认寻址（D-012）、CORS 白名单（P1-012）与开发期调试都有确定值；随机端口会让这些约定失效。
- **配套约束**：
  1. 端口冲突必须报错退出，不得静默改用随机端口。
  2. 端口表在 `docs/develop.md` 与本节各留一份，修改时必须同时更新。
  3. API 探针示例的默认端口已对齐到 `20103`（`crates/lib-web/examples/api_probe_server.rs`），仍可被环境变量 `AGENT_PROBE_PORT` 覆盖。
- **状态**：`DECIDED`

---

## D-023 日志目录与归档策略

- **问题**：服务端、CLI、TUI 的日志如何分目录、保留多久、控制台与文件的格式如何对齐？
- **背景**：三个运行模式可能同时存在（server 常驻 + CLI/TUI 客户端），共用同一批日志文件会互相覆盖；归档保留过久会长期占用用户目录空间。
- **决策**：
  1. 日志目录与配置/数据同级且互不复用：服务端 `logs`、CLI `logs-cli`、TUI `logs-tui`，统一由 `lib-core` 的 `AppDirectories` 提供（`logs_server` / `logs_cli` / `logs_tui`）。
  2. 归档保留 2 天（框架默认 7 天）；关闭 `combined.log`（与单级别日志重复写盘），只保留单级别文件。
  3. 控制台输出：服务端与 CLI 开启（stdout），TUI 关闭（会破坏界面渲染）；三者都写各自日志目录。
  4. 控制台带 ANSI 颜色，文件层独立格式化 span 字段（`framework-core` 的 `FileFields`），两侧除颜色外文字一致。
  5. 日志初始化入口收敛在 `lib-core` 的 `logger`：`init_server` / `init_cli` / `init_tui`，调用方只传级别并持有返回的守卫。
- **决策理由**：分目录避免多进程抢同一批文件；2 天归档与关闭 combined 控制用户目录占用；入口收敛避免各入口重复拼装 `LoggingConfig`。
- **代价**：日志分散在三个目录，排查跨模式问题需分别查看；2 天保留对长期归档场景偏短。
- **状态**：`DECIDED`

---

## D-024 统一错误响应与错误码分配

- **问题**：所有接口的错误响应结构、HTTP 状态码与 `code` 如何分配，错误码清单如何维护，客户端探活接口返回什么？
- **背景（已核实源码 + P1-004 实测）**：
  - `framework-web` 的 `WebResponse::from_error` 固定输出 `{"code": <HTTP 状态码>, "message": "<对外消息>"}`，`code` 直接取 `WebError::status()`，与 HTTP 状态码同值；框架没有独立业务码的传参入口（`web_api` 的返回值要么是 `anyhow::Result<T>`，要么是 `WebResponse` / `WebResponseOf<T>`）。
  - `WebError::public_message()` 对 `Internal` / `ReturnConversion` / `Panic` 固定返回 `服务器内部错误`；原始 message、source 链、`Location` 与 backtrace 只经 `WebError::log` 进日志。
  - 注意例外：`WebErrorKind::Message`（裸 `anyhow` 错误被归一化后的分类）**不**走固定文案，会把原始 message 对外输出。
- **决策**：
  1. `code` 与 HTTP 状态码保持一致，不引入独立业务码（用户确认）。段位：4xx 客户端错误、5xx 服务端错误；**已分配的取值只追加、不复用**，新增错误（含后续阶段的业务错误）先在本表登记再落地。
  2. 契约类型 `ApiError { code: u32, message: String }` 定义在 `lib-web`（`crates/lib-web/src/error.rs`，`auto_type`），进入 SDK 生成物（TS `ApiError`，Rust `ApiError`），客户端解析错误体只依赖它，不手写重复定义。
  3. 错误分类由框架统一映射，业务侧只用 `WebError::parameter` / `not_found` / `unauthorized` / `forbidden` / `internal` 表达，**不手写错误响应体**、不自行返回 `WebResponse` 拼错误 JSON。
  4. 携带内部细节（SQL、路径、第三方错误）一律走 `WebError::internal(msg, source)` 的 `source`，不放进 `msg`；禁止用裸 `anyhow!("...")` 承载内部细节（会被归入 `Message` 分类并原样对外）。
- **错误码清单（P1 已分配）**：

| 分类（`WebErrorKind`） | 构造 / 触发方式 | HTTP | `code` | 对外 `message` | 备注 |
|---|---|---|---|---|---|
| `Parameter` | `WebError::parameter` / `Json<T>`、`Query<T>` 提取失败 | 400 | 400 | 调用方 message 或框架固定文案（`请求参数转换失败` / `查询参数转换失败` / `请求体不是有效的 JSON`） | 不含字段名与解析器细节 |
| `Unauthorized` | `WebError::unauthorized` | 401 | 401 | 调用方 message | P1 未启用鉴权，暂无样例 |
| `Forbidden` | `WebError::forbidden` | 403 | 403 | 调用方 message | P1 未启用鉴权，暂无样例 |
| `NotFound` | `WebError::not_found` / 路由不存在 / 方法不匹配 | 404 | 404 | 调用方 message 或 `请求接口不存在` | |
| `Message` | 裸 `anyhow` 错误归一化 | 500 | 500 | **原始 message** | 会外泄内部细节，业务代码禁止用它携带细节 |
| `ReturnConversion` | 返回值 JSON 序列化失败 | 500 | 500 | `服务器内部错误` | |
| `Internal` | `WebError::internal(msg, source)` | 500 | 500 | `服务器内部错误` | 细节（含 `msg`）只进日志 |
| `Panic` | handler 内 panic，由 `catch_panic` 兜底 | 500 | 500 | `服务器内部错误` | panic 消息与堆栈只进日志 |

- **可复现样例（探针服务 `cargo run -p lib-web --example api_probe_server`，默认 `127.0.0.1:20103`）**：

  ```text
  curl -i "http://127.0.0.1:20103/api/probe/fail?size=1000"   # 400 参数错误
    → {"code":400,"message":"size 超出探针允许范围"}
  curl -i "http://127.0.0.1:20103/api/probe/missing"          # 404 未找到
    → {"code":404,"message":"请求接口不存在"}
  curl -i "http://127.0.0.1:20103/api/probe/internal"         # 500 内部错误
    → {"code":500,"message":"服务器内部错误"}
  curl -i "http://127.0.0.1:20103/api/probe/panic"            # 500 panic
    → {"code":500,"message":"服务器内部错误"}
  ```

  四类响应结构完全一致（`code` + `message`），且都不含 panic 消息、SQL、文件路径。
- **探测接口（P1-009 起 11 条，取代 D-019 中“7 个探针接口”的表述）**：`api/probe/{void,echo,query,typed,fail,response,response_of,internal,panic,health,version}`。新增的四条按用户要求直接扩展探针模块，不另建业务 API；`health` 与 `version` 均显式 `AuthRule::anonymous()`（探活必须在鉴权之前可用）。
- **`health` / `version` 契约**：

  | 接口 | 路径 | 载荷 | 说明 |
  |---|---|---|---|
  | 健康检查 | `GET api/probe/health` | `ProbeHealthVO { version, protocolVersion, startedAtMs, uptimeMs }` | 客户端探活；`version` 取 workspace 版本，`protocolVersion` 取 `lib_web::probe::PROTOCOL_VERSION`（当前 `"1"`），时间为 Unix 毫秒（按 D-018 以字符串传输） |
  | 版本信息 | `GET api/probe/version` | `ProbeVersionVO { version, protocolVersion }` | 仅版本，不依赖时钟，便于客户端做兼容性判断 |

  启动时刻由 `lib_web::probe::startup()` 提供：服务端启动时调用 `mark_startup()` 记录（`lib-bin-web` 与探针示例均已接入），未显式记录时退化为首次访问时刻。两个接口的载荷都由 `R{code,message,data}` 包装（与其余 `Type` 返回一致）。
- **代价**：`code` 与 HTTP 状态码同值，无法用 `code` 同时表达“哪一类业务失败”；细分业务错误码需要框架先支持自定义 code（跨仓库改动），P1 不做。
- **状态**：`DECIDED`

---

## D-025 P1 Mock 会话契约与 P2 替换点

- **问题**：P1 的 Mock 会话 / 消息 / Run 接口，契约放在哪些 Crate、接口怎么切、Mock 行为边界在哪，P2 接入真实实现时改哪一层？
- **背景**：
  - P1 不创建任何 `agent-*` Crate（`docs/workspace.md` 8.1），而第六节把 Message / Run 归 `agent-core`；`lib-web` 在 P1 只暴露 Mock 契约接口。
  - P1 不引入 SQLite（D-017），会话存储是内存 Mock。
  - `web_api` 的路径不支持占位符（D-008），GET 最多一个参数对象、查询参数值全部来自字符串（D-021），`i64` 在 wire 上以字符串传输（D-018）。
- **决策**：
  1. **类型归属**：Session / Message / Run 的契约类型（含 `*PO` / `*Params` / `*VO` / 枚举）落在 `types-core` 的 `session` 模块（`crates/types-core/src/session/`），`framework-proc-auto/collect` 在 P1-010 开启。这是**阶段裁剪**：P2 建 `agent-core` 后按 `docs/workspace.md` 6.2 整体迁移，类型名与字段不变，生成物只是 namespace 变化。
  2. **业务归属**：Mock 业务在 `service-core`（`session/{service,store,error}.rs` + `holder/`），Handler 只做参数解包、业务调用与错误映射，`lib-web` 不承载业务规则。
  3. **命名**：按 `docs/develop.md` 的契约规范 —— 请求体 `*PO`、查询参数 `*Params`、响应 `*VO`、枚举 `<业务名>` + `auto_enum`（wire 值为 `SCREAMING_SNAKE_CASE`）。tasks.md 里 `CreateSessionParams` / `SendMessageParams` 的写法以本条为准（`SessionCreatePO` / `MessageSendPO`）。
  4. **ID 与数值**：id 一律 `String`（雪花 ID 转字符串，`lib_core::id::next_id`）；数量、token 用量、时间戳键（`*_ms`）用 `i64`，wire 上是字符串，TS 侧为 `string`（D-018）。
- **接口清单（P1-010 起 6 条，动作段只用固定动词）**：

  | 方法 | path | 入参 | 返回 | 说明 |
  |---|---|---|---|---|
  | POST | `api/session/create` | `SessionCreatePO { title?, model? }` | `SessionSummary` | 缺省标题为“新会话”，缺省模型为 `mock-model` |
  | GET | `api/session/list` | 无 | `SessionSummary[]` | 按 `updatedAtMs` 倒序 |
  | GET | `api/message/list` | `MessageListParams { sessionId }` | `Message[]` | 按写入顺序；会话不存在返回 404 |
  | POST | `api/message/create` | `MessageSendPO { sessionId, content }` | `MessageSendVO { message, run }` | 写入用户消息并创建 `QUEUED` 的 Run |
  | GET | `api/run/get` | `RunGetParams { runId }` | `Run` | Run 不存在返回 404 |
  | POST | `api/run/cancel` | `RunCancelPO { runId }` | `Run` | 只允许取消 `QUEUED` / `RUNNING`，否则 400 |

- **Mock 行为边界**：发送消息不产出助手回复、不推进 Run 状态（P1-011 的 SSE 流负责 `QUEUED → RUNNING → COMPLETED` 与助手消息，P2 换成 Agent Runtime）。会话隔离由存储的“消息按 sessionId 分桶”保证，没有跨会话读取入口。
- **错误映射（补齐 D-024 的业务侧用法）**：`service-core` 的 `SessionError` 只表达业务语义，由 `lib-web` 映射 —— `NotFound` → 404、`Invalid`（参数或状态不满足规则）→ 400、`Internal` → 500 且对外固定文案，细节只进日志。
- **P2 替换点**：

  | P1 位置 | P2 处理 |
  |---|---|
  | `service-core/src/session/store.rs`（`Mutex<HashMap>`） | 换成 Repository（`service-core` 的 `repository` 模块 + `lib-db` / SQLite） |
  | `service-core/src/session/service.rs` | 换成真实 Run 状态机（由 `agent-runtime` 驱动），方法签名与错误语义保持不变 |
  | `service-core/src/holder/mod.rs`（进程级单例） | 改为 `lib-bin-web` 启动时构造并注入，移除单例 |
  | `types-core/src/session/` | 迁入 `agent-core`（同名字段），DTO 不另起一套 |
  | `lib-web/src/session/api.rs` | 不改；契约变更只走“改定义 → `make gen`” |

- **代价与遗留**：
  1. Mock 的 Run 永远停在 `QUEUED`（除非被取消），列表里会看到未完成的 Run；P1-011 接入流式后才会推进到 `COMPLETED`。
  2. 会话列表没有分页与筛选，P2 需要时按 `PaginationParams` 补接口，属于新增而非修改。
  3. 运行期行为（创建会话、发消息、列表互不可见、取消 Run）已按本条命令逐条实测（见下“实测回填”），但 P1 不写自动化测试，回归靠人工重跑。
- **实测回填（2026-11-07）**：
  - 环境：`cargo build` 后 `./target/debug/bin-agent.exe server --port 20100`（日志目录 `~/.lingting/agent/logs`），逐条调用上述 6 个接口，20 项检查全部通过：显式/默认标题与模型、发消息返回 `USER` 消息 + `QUEUED` Run、`message.runId` 关联、两会话消息互不可见、`messageCount` 与 `updatedAtMs` 倒序、`run/get`、`run/cancel` 到 `CANCELLED` 且 `finishedAtMs` 有值、状态持久化、重复取消 400、未知会话（含发消息）/未知 Run 404、空白内容 400、缺字段 400；错误体始终只有 `{code,message}`，日志无 panic、无 500。
  - 验证中发现并修复：`SessionError::Invalid` 原先同一句文案既作对外 `message` 又作内部 `source`，日志出现重复的 `error_chain=… -> …`；改为 `source` 只写分类文案后变为 `消息内容不能为空 -> 会话业务参数或状态不满足规则`。
  - 已知日志观感问题（不改）：业务 404 在框架日志里仍标 `category=路由不存在`（`WebErrorKind::NotFound` 的固定标签），响应体与状态码不受影响。
  - 验证工具注意：Git Bash 下 `curl -d '{"title":"中文"}'` 会把中文按 GBK 字节发出，服务端返回 400 `请求体不是有效的 JSON`（属调用方编码问题，非实现缺陷）；需要用 `--data-binary @file` 或 UTF-8 环境发送。
  - SDK：Rust 生成物中 6 个方法随 `lib-bin-cli` 编译通过；TS 生成物在 `tsc --noEmit --strict --skipLibCheck false packages/sdk-ts/dist/index.d.ts` 下无错误。
- **状态**：`DECIDED`


---

## D-026 P1 Streaming 契约（RunEvent + SSE）

- **问题**：P1 的流式接口怎么定义，事件类型如何跨语言共享，断线与取消时 Run 处于什么状态，P2 替换哪一层？
- **背景**：
  - D-007 要求“事件契约单一来源”，但 `auto_type` 只支持具名字段结构体、`auto_enum` 只产出字符串联合，因此 serde 内部标签枚举（每个事件一个载荷结构体）无法生成到两侧 SDK。
  - `EventSource` 只支持 GET 且不能带自定义 header/body（A-6），若要复用原生实现，流式端点必须是 GET + 查询参数。
  - 框架的 `WebResponse::stream` 接受 `BoxStream<'static, Result<Bytes, std::io::Error>>`，`framework-web-axum` 的分发层用 `Body::from_stream` 直接转成流式响应，中间没有缓冲层。
- **决策**：
  1. **端点**：`GET api/run/stream?runId=`（`RunStreamParams { runId }`），动作段新增固定动词 `stream`。
  2. **事件建模**：`RunEvent` 是扁平结构体（`auto_type`），判别字段 `event: RunEventKind`（`auto_enum`，serde 与 strum 都是 `snake_case`），其余载荷字段为 `Option`。P2 追加事件只增加枚举值，不改已有语义。
  3. **帧编码**：落在 `lib-web-core` 的 `sse` 模块（`frame` / `json_frame` + content-type 与响应头常量）；`lib-web` 的 `session/stream.rs` 只做“事件 → 帧”的适配；`service-core` 的 `session/stream.rs` 只产出 `RunEvent`。
  4. **状态推进**：订阅即 `QUEUED → RUNNING`（`Store::start_run` 带状态守卫，保证一个 Run 只有一个生产方），`run_completed` 之前先落库助手消息与终态。
  5. **断线**：HTTP 流被 drop → 通道关闭 → 生产任务停止并把 Run 落成 `CANCELLED`（带 `finishedAtMs`），不写半成品助手消息。
  6. **取消**：流式期间 `api/run/cancel` 把 Run 置 `CANCELLED`，生产方在下一段增量前发现状态变化即停止推送，不再覆盖终态。
  7. **心跳**：等待增量的间隙超过 15 秒补一条 `heartbeat`；Mock 的增量间隔是 120ms，正常情况下不会触发，P2 接真实 token 流后才生效。
- **D-007 定案**：采用方案 1（SSE 端点按 `Blob` 进元数据 + 客户端自实现消费）。缓解手段已验证可行 —— 生成物同时导出 `RUN_STREAM: ApiDefinition { path: "api/run/stream" }`（Rust）与 `ApiDefinitions.runStream.path`（TS），客户端不需要手写 path 字面量；事件类型来自生成的 `RunEvent` / `RunEventKind`。方案 2（框架新增 `ApiReturnType::Stream`）留作框架演进项，P2 前再评估。
- **代价与遗留**：
  1. `run_failed` 在 P1 没有产出方（Mock 不会失败），只有契约与文档；P2 接真实执行后启用。
  2. 事件流不做断点续传与事件回放：中途断开只能重新发消息（P1 无持久化，续传本来也无意义）。
  3. `inputTokens` / `outputTokens` 在 Mock 下是字符数估算，不是真实分词。
  4. SSE 的 `id:` 是 `seq` 的十进制文本，而 `data` 里的 `seq` 因 D-018 是字符串，两者语义相同、表现不同。
- **实测回填（2026-11-07）**：
  - 正常流：`curl -N` 收到 19 帧（`run_started` + 17 × `message_delta` + `run_completed`），按约 120ms 逐个到达；响应用 `transfer-encoding: chunked`、无 `content-length`，直连未被缓冲（A-4 的直连部分成立，浏览器 / 反代场景留给 P1-016）；`content-type`、`cache-control`、`x-accel-buffering`、`x-trace-id` 均符合预期。
  - 终态一致性：`run_completed` 到达后 `api/run/get` 即 `COMPLETED`（含 `finishedAtMs` 与 token 估算），`api/message/list` 多出一条 `ASSISTANT` 消息。
  - 断线：`curl --max-time 0.6` 中途退出 → Run 变 `CANCELLED`（带 `finishedAtMs`），未写助手消息，服务端日志无 panic。
  - 取消：流式期间 `api/run/cancel` → 事件在下一段增量前停止（7 帧后 EOF），Run 保持 `CANCELLED`，不会被 `run_completed` 覆盖。
  - 负向用例：重复订阅 400 `Run 当前状态为 COMPLETED，不可订阅事件流`；未知 Run 404 `Run 不存在：999`。两条在日志里表现为框架的 `Web 请求异常`（`category=参数转换` / `路由不存在`），属既有观感问题，响应体与状态码正确。
- **状态**：`DECIDED`

---

## 假设清单（未验证）

| ID | 假设 | 影响 | 何时验证 |
|----|------|------|----------|
| A-1 | `framework-proc-rust` 可在不修改 `framework-proc-core` 的前提下，只靠公开的 `ApiMetadata` 完成 Rust 代码生成 | 若不可行，需向上游暴露更多元数据（如泛型参数的完整路径） | P1-006 |
| A-2 | 生成的 Rust 客户端可以引用服务端 DTO 类型而不产生循环依赖（契约 crate 独立于业务 crate） | 影响 D-003 的 DTO 策略 (a)/(b) | P1-006 |
| A-3 | `framework-web-axum` 的 `AxumBuilder` 与静态资源 fallback 可以共存（框架用 `fallback` 做 dispatch，需确认不会与 SPA fallback 冲突） | 影响 P1-012 的实现方式 | P1-012 |
| A-4 | SSE 经 `WebBody::Stream` 在 Windows + 浏览器 / reqwest 下不被缓冲（无 `Transfer-Encoding` 或反代缓冲问题） | 影响流式体验 | **直连已验证（P1-011）**：`curl -N` 下事件按约 120ms 逐个到达、`chunked` 无缓冲，响应头为 `cache-control: no-cache` / `x-accel-buffering: no`；浏览器与反代场景留待 P1-016 |
| A-5 | ~~`inventory` 注册在 Windows 的动态链接场景下工作正常~~ **已验证（P1-004）**：静态链接下可用，但消费方必须显式引用 crate 符号作为链接锚点，否则注册项被链接器丢弃 | 生成器与服务启动都必须先引用 `lib_web::web_routes()`，否则接口与元数据为空 | 已验证 |
| A-6 | WebUI 的 SSE 消费可用 `EventSource`（原生），无需为 POST + SSE 引入 fetch stream | 影响 WebUI 传输实现（`EventSource` 只支持 GET） | P1-016 |

> A-6 需要特别注意：`EventSource` 只能发 GET 且不能带自定义 header/body，而 `RunEvent` 流通常由 POST 触发。若确认必须 POST，则 WebUI 需用 `fetch` + `ReadableStream` 手动解析 SSE，这会直接影响 D-007 的落地方式。

> A-6 后续（P1-011）：事件流端点已定为 `GET api/run/stream?runId=`（D-026），原生 `EventSource` 可用；P1-016 只需验证浏览器侧的实际行为（含中断与错误事件）。
