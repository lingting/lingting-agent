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
| D-005 | 代码生成时机 | DECIDED (待验证) |
| D-006 | 生成物是否纳入版本控制 | DECIDED |
| D-007 | SSE 与 SDK 的类型化支持 | OPEN |
| D-008 | 路径参数与 REST 风格路径 | DECIDED (已核实) |
| D-009 | WebUI 静态资源嵌入方案 | OPEN |
| D-010 | WebUI 技术栈 | OPEN |
| D-011 | TUI 与 Server 的连接管理 | OPEN |
| D-012 | Server 地址配置与发现 | DECIDED |
| D-013 | 优雅关闭实现方式 | OPEN |
| D-014 | 鉴权策略 | OPEN |
| D-015 | `framework-*` 版本对齐与获取方式 | OPEN |
| D-016 | 单 Binary 的功能裁剪与体积权衡 | OPEN |
| D-017 | P1 是否引入 SQLite | DECIDED |
| D-018 | `i64` 账号/ID 的跨语言表示 | DECIDED |
| D-019 | API 版本管理策略 | OPEN |
| D-020 | P1 跨平台构建策略 | OPEN |

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
- **状态**：`DECIDED (待验证)` —— 方向已定，具体 API 形态、DTO 策略在 P1-006 中验证后回填本节。

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
- **状态**：`DECIDED`

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
- **状态**：`DECIDED (待验证)` —— 命令的具体形态（独立 crate 二进制 / `cargo xtask` / `just` 脚本）在 P1-007 落地时确定。

---

## D-006 生成物是否纳入版本控制

- **问题**：生成的 TS 包与 Rust 客户端源码是否提交到仓库？
- **可选方案**：
  1. 入库（提交生成物），CI 校验“重新生成无 diff”。
  2. 不入库，构建时生成（每个消费者各自生成）。
- **推荐方案**：方案 1。
- **决策理由**：入库后可在 PR 中直接看到 API 变更对 SDK 的影响；CI 一致性检查有可比对的基线；WebUI 的本地开发不需要先编译 Rust 生成器。方案 2 会让前端构建依赖 Rust 工具链，且 diff 不可见。
- **配套约束**：生成文件头部必须带“自动生成，勿手改”；合并冲突时一律重新生成，不手工合并。
- **状态**：`DECIDED`

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
- **状态**：`OPEN` —— 若方案 1 的缓解手段不足以满足“客户端零手写 path”，则升级到方案 2，并在 P1-011 之前定案。

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
- **约束**：P1 不引入重型组件库；状态管理用原生 hooks，不引入额外状态库。
- **状态**：`OPEN`（等确认，若无异议按推荐方案执行）

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
  1. 固定优先级：命令行 > 环境变量 `AGENT_SERVER_ADDR` > 配置文件 `[server] addr` > 默认 `http://127.0.0.1:8787`。
  2. 仅环境变量 + 命令行。
  3. 端口文件（server 启动写 `<config_dir>/agent/server.port`，client 读取）。
- **推荐方案**：方案 1 为主，方案 3 作为将来“多实例/随机端口”时的扩展，P1 只预留不实现。
- **决策理由**：方案 1 对开发者与最终用户都直观；方案 2 缺少持久化默认值，用户每次都要传参；方案 3 在 P1 无多实例需求时属于过度设计。
- **配套约束**：`agent server` 不得静默使用随机端口；端口冲突必须报错退出。
- **待定**：配置目录名的最终取值（`agent` / `personal-agent`）、配置文件是否与未来 P2 的 SQLite 放在同一目录。
- **状态**：`DECIDED`（优先级已定；目录命名细节随 P1-008 确定）

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
- **状态**：`OPEN`（等 P1-004 实测结论后定案，倾向方案 1）

---

## D-015 `framework-*` 版本对齐与获取方式

- **问题**：P1 如何引入 `framework-*`？
- **背景（已核实）**：
  - 本地源码仓库 `D:/code/lingting/lingting-rust-framework` 为 workspace 统一版本 **26.11.0**，`edition 2024`、`rust-version 1.98.0`。
  - registry 发布状态（已按 crates.io 稀疏索引核实）：`framework-core`、`framework-proc-core`、`framework-proc-ts`、`framework-proc-web`、`framework-web`、`framework-web-axum` **均已发布 26.11.0**，且 feature 定义（`collect` 链）与本地源码一致。本机 cargo 缓存对 web 三件套只到 26.8.150，属缓存滞后，不代表未发布。
  - `framework-proc-core` 硬约束 `specta = "=2.0.0-rc.25"`、`specta-typescript = "0.0.12"`。
- **可选方案**：
  1. 全部走 `path` 依赖本地 `lingting-rust-framework`（版本必然一致，改动即时可见）。
  2. 全部走 registry 26.11.0（需先确认 web 系列是否已发布 26.11.0）。
  3. 混用（明确 REJECTED：正是要避免的情况）。
- **推荐方案**：方案 1 起步（P1 期间大概率需要改 `framework-proc-rust` 与 `framework-web-axum`）；在 26.11.0 全部发布且改动合入后切回方案 2。
- **决策理由**：P1 依赖对框架的两处改动（新增 `framework-proc-rust`、可能新增 shutdown 入口），path 依赖是唯一能让改动即时生效的方式；混用会造成 trait/类型跨版本不兼容（`inventory` 注册的元数据类型必须唯一）。
- **反例警示**：不同版本的 `framework-proc-core` 同时进入依赖图会导致 `ApiMetadata` 类型不统一、注册表分裂，表现为“接口定义了但生成器看不到”。
- **状态**：`OPEN`（registry 26.11.0 已全量发布，P1-003 只需在 path / registry 之间做最终取舍）

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

---

## D-019 API 版本管理策略

- **问题**：API 版本如何表达？
- **可选方案**：
  1. path 前缀（`/api/v1/...`）。
  2. 请求头（`x-api-version`）。
  3. P1 不做版本化，`/api/...` 直接演进。
- **推荐方案**：P1 采用方案 1 的形式预留，但**不实现多版本并存**（即 v1 只是路径的一部分，尚无 v2 处理器）。
- **决策理由**：本地 single-user 阶段没有并行版本需求，但路径里留一个版本段成本极低，将来引入 v2 时不必重排全部 path；方案 2 对 SSE 与静态资源场景更麻烦。
- **状态**：`OPEN`（确认是否值得在 P1 就加 `v1`，或留到 P2 一次到位）

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

## 假设清单（未验证）

| ID | 假设 | 影响 | 何时验证 |
|----|------|------|----------|
| A-1 | `framework-proc-rust` 可在不修改 `framework-proc-core` 的前提下，只靠公开的 `ApiMetadata` 完成 Rust 代码生成 | 若不可行，需向上游暴露更多元数据（如泛型参数的完整路径） | P1-006 |
| A-2 | 生成的 Rust 客户端可以引用服务端 DTO 类型而不产生循环依赖（契约 crate 独立于业务 crate） | 影响 D-003 的 DTO 策略 (a)/(b) | P1-006 |
| A-3 | `framework-web-axum` 的 `AxumBuilder` 与静态资源 fallback 可以共存（框架用 `fallback` 做 dispatch，需确认不会与 SPA fallback 冲突） | 影响 P1-012 的实现方式 | P1-012 |
| A-4 | SSE 经 `WebBody::Stream` 在 Windows + 浏览器 / reqwest 下不被缓冲（无 `Transfer-Encoding` 或反代缓冲问题） | 影响流式体验 | P1-011 |
| A-5 | `inventory` 注册在 Windows 的动态链接场景下工作正常（proc-macro + static 注册的已知坑） | 若失败，元数据收集链路需要换方案 | P1-005 |
| A-6 | WebUI 的 SSE 消费可用 `EventSource`（原生），无需为 POST + SSE 引入 fetch stream | 影响 WebUI 传输实现（`EventSource` 只支持 GET） | P1-016 |

> A-6 需要特别注意：`EventSource` 只能发 GET 且不能带自定义 header/body，而 `RunEvent` 流通常由 POST 触发。若确认必须 POST，则 WebUI 需用 `fetch` + `ReadableStream` 手动解析 SSE，这会直接影响 D-007 的落地方式。
