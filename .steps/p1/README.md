# P1：基础工程架构

**状态：未开始**

P1 的目标不是做出 Agent，而是把「单 Binary + Server/Client + 统一 API 契约 + 双端 SDK 生成」这套地基一次性打正确。地基一旦错误，P2 之后的每一条 API 都要返工。

---

## 一、阶段目标

1. 建立**单 Binary、双运行模式**（`agent server` 服务端模式 / `agent`、`agent tui` 客户端模式）的可执行骨架。
2. 建立 **Server 是唯一业务逻辑执行方** 的通信边界：CLI / TUI / WebUI 只能通过 HTTP + 生成的 SDK 访问服务端。
3. 建立**服务端 API 单一来源**：Rust 服务端 API 定义 → API 元数据 → TypeScript SDK + Rust Client SDK。
4. 建立 **WebUI 静态资源嵌入 Binary** 与 Server 托管能力。
5. 用 **Mock API + Mock SSE** 验证整条链路（含错误响应、流式、Server 未启动）。

---

## 二、阶段边界

### In Scope（P1 必须完成）

- 工程骨架、工具链固定、依赖版本矩阵。
- Clap 命令结构、运行模式分发、配置与 Server 地址管理。
- Axum Server：健康检查、统一错误响应、trace id、日志、CORS、静态资源兜底、优雅关闭、SSE 基础能力。
- API 契约定义与元数据收集；TypeScript SDK 与 Rust Client SDK 的生成流程、生成物管理、CI 一致性门禁。
- TUI 最小对话界面 + Streaming 展示 + 断线提示。
- WebUI 最小对话界面 + Streaming 展示。
- 单 Binary Release 构建、运行验证、体积与启动时间初步记录。

### Out of Scope（P1 明确不做）

| 不做的事 | 归属 |
|----------|------|
| 真实 LLM 调用、Rig 接入 | P2 |
| Agent Loop、真实 Run 执行 | P2 |
| SQLite 或任何持久化 | P2 |
| Tool / MCP / Skills | P3 / P4 / P5 |
| 用户认证体系、多用户隔离 | 未排期（P1 只保留 `AuthRule` 预留位） |
| WebUI 设计精修、组件库选型定稿 | P1 只做最小可用界面 |
| Cargo Workspace / Crate 结构变更 | 结构以 `docs/workspace.md` 为准，P1 不新增或重命名 Crate |
| 微服务、消息队列、远程部署编排 | 不引入 |

> 关键约束：P1 的 Mock API 必须**结构与真实 API 一致**（同一套 DTO、同一套错误码、同一套 SSE 事件枚举），P2 接入真实 LLM 时只替换实现，不改契约。否则 P1 的 SDK 生成验证没有意义。

---

## 三、整体设计

### 3.1 单 Binary 双运行模式

```text
                       agent (单一可执行文件)
                                |
                +---------------+----------------+
                |                                |
        客户端模式（默认）                  服务端模式
        agent / agent tui                 agent server
        agent run "..."                   （常驻进程）
        agent status
                |                                |
        Clap 参数解析                      Axum HTTP Server
                |                                |
       生成的 SDK（Rust）                  Axum 路由 + 统一错误
                |                                |
                +-------- HTTP / SSE ------------+
                                                 |
                                          WebUI 静态资源
                                        （编译期嵌入 Binary）
```

命令面（P1 定型，后续可增）：

| 命令 | 行为 |
|------|------|
| `agent` | 无参数进入 TUI |
| `agent tui` | 显式进入 TUI |
| `agent server` | 启动服务端（前台常驻） |
| `agent run "<task>"` | 单次任务，输出到 stdout，支持 `--stream`/`--json` |
| `agent status` | 查询服务端状态（健康、版本、运行中的 Run） |
| `agent --help` / `--version` | Clap 自动生成 |

### 3.2 通信边界

- 唯一跨进程边界是 **HTTP（含 SSE）**。客户端不得 link 服务端业务 crate 中执行 Agent 的代码路径。
- 客户端可以使用服务端的 **DTO / 契约 crate**（类型来自同一份定义），但不得调用其中的业务逻辑。
- 客户端必须通过**生成的 SDK** 发起调用，Transport（reqwest 封装、SSE 解析、超时、重试）由客户端自己实现，但 method / path / 请求体 / 响应类型必须由生成物提供。

### 3.3 Server 生命周期

1. 读取配置（文件 + 环境变量 + 命令行覆盖）。
2. 初始化日志与 trace。
3. 绑定监听地址（`127.0.0.1` 默认）；启动失败要给出可读错误与退出码。
4. 输出就绪信息（`listening on <addr>`），供 `agent status` 与用户确认。
5. 常驻运行，接受 SIGINT / Ctrl+C 后执行优雅关闭：停止接受新连接 → 等待进行中的请求 → 关闭 SSE 流 → 退出。
6. 退出码约定：`0` 正常退出，`1` 启动失败 / 运行错误，`2` 命令行参数错误。

### 3.4 本地配置与 Server 地址管理

客户端需要稳定找到服务端。P1 采用**显式优先、约定兜底**：

```text
优先级（高 → 低）
1. 命令行参数     --server http://127.0.0.1:8787
2. 环境变量       AGENT_SERVER_ADDR
3. 配置文件       <config_dir>/agent/config.toml  [server] addr
4. 内置默认值      http://127.0.0.1:8787
```

配套约束：

- 配置目录使用跨平台目录约定（Windows `%APPDATA%`，Linux `~/.config`，macOS `~/Library/Application Support`），P1 定名并写入 `decisions.md`。
- 端口冲突时 `agent server` 应报错退出并提示可用端口，不静默改用随机端口（随机端口会破坏客户端默认寻址）。
- 若后续需要“自动发现”，以 server 写端口文件的方式演进，P1 只预留，不实现。

### 3.5 Server 未启动时的客户端行为

| 场景 | 期望行为 |
|------|----------|
| TUI 启动时连不上 Server | 不退出；显示明确提示（地址、错误原因）与重试入口，提供“重试 / 修改地址 / 退出” |
| TUI 运行中 Server 断开 | 中断当前 streaming 展示，标记该消息为“连接中断”，保留已有内容，提示重连 |
| `agent run` 连不上 | 非 0 退出码 + 一行可读错误 + 提示先运行 `agent server` |
| `agent status` 连不上 | 明确输出“服务端未运行（<addr>）”，退出码 1 |
| 浏览器访问 WebUI 但 Server 未启动 | 浏览器原生错误即可，不做额外处理 |

### 3.6 优雅关闭

- 目标：Ctrl+C 后不产生半条 SSE 事件、不丢日志、退出及时（默认 ≤ 5s 强制退出）。
- 现状约束：`framework-web-axum` 的 `AxumServer::run(wrapper)` 目前**不暴露关闭信号入口**（已核对源码，无 shutdown 相关 API）。因此本项必须先做决策：向上游框架补 `run_with_shutdown`，或在 P1 内改为直接组装 axum `Router` 并自行调用 `axum::serve(...).with_graceful_shutdown(...)`。见 `decisions.md` D-013。

### 3.7 API 契约与 SDK 生成链路

已核实的能力（源码级）：

```text
Rust async fn  #[web_api_post(path = "session/create")]
        |
        |  framework-proc-web：生成 WebRoute + 叠加 ts_api 元数据 + （collect 开启时）inventory 注册
        v
ApiMetadata { name, namespace, method, path, parameters[Body|Query], return_type[Void|Blob|Type] }
        |
        |  framework-proc-core::api_metadata_iter()  （需 collect feature）
        v
+---------------------------+        +--------------------------------+
| TypescriptApiBuilder      |        | framework-proc-rust（待新增）   |
| → ApiDefinitions +        |        | → Rust trait + 方法（待实现）   |
|   抽象类 ApiClient        |        +--------------------------------+
| → PackageBuilder 出 npm 包 |
+---------------------------+
```

关键事实与限制（全部已核对 `framework-proc-web` / `framework-proc-ts` / `framework-proc-core` 的 README 与源码）：

1. `web_api` 系列宏要求 `path`，**path 只做字符串精确匹配，不支持 `{id}` 占位符**，也不做前缀拼接。
2. 参数类型只能是 `Json<T>`（Body，GET 时退化为 query）、`Query<T>`、`PaginationParams`；**GET 最多一个参数对象**。
3. 返回值按剥壳后的类型映射：`()` → `Void`，`WebResponse` → `Blob`，其他 → `Type(name)`。
4. `i64/u64/i128/u128` 在 TS 侧映射为 `string`；因此 **id 类字段必须用结构体（`auto_type`）承载**，避免 TS 类型与 serde 实际输出不一致。
5. `TypescriptApiBuilder` 只生成**抽象类**，其中 `call<T>(method, path, body?, query?): Promise<T>` 是 `protected abstract`；Transport 由客户端实现。
6. `framework-proc-core` **没有任何 Rust 代码生成能力**，只到 TypeScript 与 npm 包为止。
7. 元数据通过 `inventory` 在生产进程内注册，因此**生成器必须是一个 link 了 API crate（开启 `collect`）的可执行程序**，不能在纯构建脚本里凭空得到元数据。

`framework-proc-rust` 承担第 6 项的缺口，方向已定（见 `decisions.md` D-003），具体 API 形态在 P1-006 中验证后固化。

### 3.8 客户端实现形态

- **CLI**：Clap 定义命令，`run` / `status` 走生成 SDK；TUI 作为子命令进入 Ratatui。
- **TUI**：`event loop + 状态机`，渲染会话列表与消息流；SSE 增量渲染；输入与中断（`Esc` 取消当前 Run，P1 为 Mock 取消）。
- **WebUI**：Vite 构建的静态站点，使用生成的 TS SDK + 自实现 SSE Transport；只做单会话、单输入框、消息列表、流式增量渲染、错误提示。
- 三个客户端共享同一套错误语义（HTTP 状态 + `R{code,message}`），提示文案各自本地化，语义一致。

---

## 四、关键约束

1. **不允许客户端自维护 API 定义**：任何 `POST /api/xxx` 字面量出现在客户端手写代码中（除 Health 探活的探活性 GET 之外）都视为违规，必须来自生成物。
2. **不允许两份 Schema**：禁止手写 OpenAPI 文件作为第二来源；若将来引入 OpenAPI，必须是从 `ApiMetadata` 生成的产物 或 生成器的输入来源之一，不得人工维护。
3. **生成物必须可重建**：同一次提交内“重新生成”不产生 diff（幂等）。
4. **Mock 不等于临时实现**：Mock API 的 DTO、错误码、SSE 事件必须与 P2 的真实契约同名同构。
5. **禁止提前拆微服务**、禁止引入独立的 Agent 服务进程；进程模型就是「一个 server 进程 + N 个客户端进程」。
6. **P1 不引入 SQLite**：会话与消息保存在内存（进程重启即丢失），持久化在 P2 完成。
7. **不锁死模块结构**：任务可以在具体 crate 划分确定后映射到代码位置；本阶段只约束逻辑职责与依赖方向（Client → 契约/DTO → Server；禁止 Server 反向依赖客户端）。

---

## 五、执行顺序

任务按 Batch 推进，批次内可并行，批次间严格顺序（详见 `tasks.md`）。

```text
Batch A  工程基线与契约可行性
  P1-001 仓库与工具链勘察
  P1-002 工程骨架与工具链固定        ← 001
  P1-003 依赖与版本矩阵对齐          ← 002
  P1-004 验证 framework-proc-web 定义  ← 003
  P1-005 验证 TS SDK 生成链路          ← 004
  P1-006 验证 framework-proc-rust      ← 004

  —— 契约固化（仍属 Batch A，完成后方可进入 Batch B）——
  P1-007 API 契约单一来源与生成流程固化  ← 005, 006

Batch B  Server 基础能力（与 Batch C 可部分并行：除 014 依赖 007/008 外无耦合）
  P1-008 Server 运行时骨架              ← 003
  P1-009 统一错误响应与健康检查         ← 008
  P1-010 Mock 会话业务 API              ← 009
  P1-011 Mock SSE 流式响应              ← 010
  P1-012 CORS / 静态资源 / WebUI 兜底   ← 009
  P1-013 优雅关闭与生命周期             ← 008

Batch C  客户端
  P1-014 CLI 命令结构与运行模式分发     ← 007, 008
  P1-015 TUI 客户端与 Streaming 展示    ← 014, 011
  P1-016 WebUI 最小对话界面             ← 007, 012
  P1-017 WebUI 静态资源嵌入 Binary      ← 016

Batch D  收口
  P1-018 单 Binary Release 构建与跨平台 ← 015, 017
  P1-019 API 一致性与生成物 CI 门禁     ← 007, 018
  P1-020 P1 端到端 Mock 闭环验收        ← 全部
```

> **顺序硬约束**：P1-005 / P1-006 未通过前不得开始写业务 API（P1-010），否则会返工两次。P1-004 未确认宏参数限制前不得设计 REST 风格路径。

---

## 六、与 P2 的衔接

P1 交付的“空壳”必须能无痛替换为真实实现：

| P1 产物 | P2 演进方式 |
|---------|-------------|
| Mock session/message API | 换成真实 Session/Message 存储（SQLite），DTO 与 endpoint 不变 |
| Mock SSE 事件 | 换成真实 LLM token 流，事件枚举只增不改 |
| Run 生命周期接口（创建/取消/查询） | 换成真实 Run 状态机，接口不变 |
| 内存存储 | 换成 Repository 抽象 + SQLite 实现 |
| Rig Provider | 新增 Adapter，不进入 API 层 |

因此 **P1 的 DTO 设计要一次到位**：即使是 Mock，也要定义 `SessionSummary`、`Message`、`Run`、`RunEvent`、`ApiError` 的完整字段与枚举值。
