# 开发规范

## 开发命令（Makefile）

根目录 `Makefile` 是唯一命令入口，不再提供 `scripts/` 脚本；需在 Git Bash 中执行（recipe 使用 POSIX sh）。

| 命令 | 作用 |
|------|------|
| `make gen` | 由服务端 API 定义生成 Rust / TS 两份客户端 SDK（产物落 `packages/`，不入库） |
| `make build` | 编译 workspace 全部成员；`ARGS` 透传，如 `make build ARGS="--release"` |
| `make check` | `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`，不执行测试 |
| `make fmt` | `cargo fmt --all` |
| `make test ARGS="..."` | 生成 SDK、构建 `bin-agent` 后运行测试入口；运行期准备（如构建 WebUI 产物）由入口按选中用例自行完成。`ARGS` 传用例名前缀或 `--all`（详见“测试”一节） |
| `make clean-gen` | 删除生成物目录 `packages/` |
| `make ui-install` / `make ui-dev` / `make ui-build` / `make ui-lint` | 在 `lingting-agent-ui/` 内执行对应 pnpm 命令 |
| `make ui-e2e-install` | 安装 Playwright 的 chromium（端到端用例用；其他机器或 CI 首次运行前执行一次） |

`make help` 的文本放在 `.make/help.txt`：make 在 Windows 上按 ANSI 代码页解析 Makefile，中文写在 Makefile 的 recipe 里会变成乱码。

## 固定端口

项目各进程一律使用固定端口，不使用随机端口；唯一例外是测试入口为隔离用例动态申请空闲端口（见“测试”一节）。

| 服务 | 端口 | 说明 |
|------|------|------|
| lingting-agent | 20100 | 服务端端口 |
| 开发环境 WebUI | 20101 | 开发环境 WebUI 端口 |
| 开发环境服务端 | 20102 | 开发环境服务端端口 |
| API 探针服务端 | 20103 | API 探针服务端端口 |
| 测试共享服务端 | 20104 | 测试入口启动的唯一共享服务端端口；隔离用例使用动态空闲端口 |

约定：

- 端口冲突时直接报错退出，不静默改用随机端口。
- 未显式配置时的默认端口按构建形态切换（D-022）：**debug 构建用 `20102`（开发环境服务端）**，**release 构建用 `20100`**。服务端与客户端（`tui` / `run` / `status`）共用这一默认值，因此 `cargo run -p bin-agent -- server` 与 `make ui-dev` 的代理目标天然对齐。
- 端口变更必须同时更新本表与 `.steps/p1/decisions.md` 的 D-022。

## 测试

端到端测试由 `crates/bin-tests`（包名 `bin-tests`）统一承载，用一个入口管理资源并分发用例，替代手工 curl 复测。

| 项 | 说明 |
|---|---|
| 运行命令 | `make test ARGS="..."`：先执行 `make gen`（编译期必需，`lib-bin-cli` 用 `#[path]` 引入生成物）、构建 `bin-agent`，再运行测试入口 |
| 运行期准备 | 用例通过 `Case.prep` 声明前置（如 `Prep::WebUiBuild`），入口在启动服务端之前去重执行一次；WebUI 产物比源码新时直接复用，不重复构建 |
| 用例选择 | `ARGS` 传用例名前缀（如 `session` 选中 `session.*`，`session.message` 精确选中单个），可传多个；`--all` 运行全部；`--list` 只列用例 |
| 并发 | 默认取 CPU 内核数，`--concurrency N` 覆盖；单个用例失败不中断其余用例，结束后汇总 |
| 共享服务端 | 用例注册为 `Shared` 时复用入口启动的唯一服务端，固定端口 20104；写用例时必须自建会话、不得断言全局计数 |
| 隔离服务端 | 用例注册为 `Isolated` 时自起 `bin-agent server` 子进程，端口动态取空闲端口，适合有副作用（如触发关闭）或需要干净状态的用例 |
| 子进程文件 | 根目录为系统临时目录下的 `lingting-agent-tests/<运行 ID>/`：共享服务端用 `shard-shared/`，隔离用例用 `<用例名>/`；`server.log` 是子进程输出，`data/`、`config.toml`、`logs*/` 来自 `AGENT_HOME` |
| 服务端二进制 | 默认取测试入口同目录的 `bin-agent`，可用 `AGENT_TEST_SERVER_BIN` 覆盖；缺失时提示先执行 `make build` |
| 前端用例 | `web.ui` 转发 `pnpm exec vp test run --maxWorkers 4`（jsdom 单元用例），`web.e2e` 转发 `pnpm exec playwright test`（真实 Chromium）；两者都在 `lingting-agent-ui/` 内执行，命令写在 Rust 侧 `frontend` 模块，不依赖 package.json scripts |
| 浏览器依赖 | 端到端用例需要 Playwright 的 chromium：先执行 `make ui-e2e-install`；用例不会自动下载，缺失时报错原样透出 |
| 测试报告 | 全部用例结束后写入 `target/tests/<运行 ID>/README.md`（汇总与用例索引）；用例按领域汇总到同目录 `<领域>.md`，README 索引给出该用例在领域文档中的行号范围，领域文档顶部也提供行号索引；每条含结果、耗时、服务地址、日志目录与用例返回的结果信息 |

约定：

- 用例名为 `<领域>.<场景>`，放在 `crates/bin-tests/src/cases/<领域>.rs`，并在 `cases/mod.rs` 汇总；领域内用例变多时拆成 `cases/<领域>/` 子模块，由 `cases/<领域>.rs` 统一注册与放公共辅助。
- 测试只写在 `bin-tests`：其他 crate 不写 `#[cfg(test)]` 单元测试，为测试提供的入口必须由 feature 隔离（如 `lib-bin-tui` 的 `testing`）且只在 `bin-tests` 启用。
- 用例优先只通过生成的 SDK（`lib-bin-cli::client::Client`）访问服务端，不手写 API 定义。**例外**：`web.*` 里断言静态资源、SPA 回落与 CORS 预检这类不经过 SDK 的路径，用 workspace 内的 `framework-http` 直发原始请求；接口 path 仍复用生成的 `PROBE_HEALTH` / `PROBE_SHUTDOWN` / `RUN_STREAM` 常量，只有「本就不存在」的路径保留字面量（那正是断言目标）。
- 前端用例分两层：`lingting-agent-ui/tests/*.test.ts(x)` 是 jsdom 单元用例（Transport、SSE 切帧、`useChat`、首屏渲染），`lingting-agent-ui/tests/e2e/*.spec.ts` 是 Playwright 端到端用例（真实浏览器 + 真实服务端）。前端命令由 `bin-tests` 的 `frontend` 模块执行，不在 `package.json` 里另设 `test` 脚本。
- 测试进程只作为客户端，不进程内启动服务端：`lifecycle` 关闭标记与日志初始化都是进程级单例，多实例会互相干扰。
- 一次性测试与临时脚本不进入仓库，回归用例一律登记到测试入口。

## 配置装载

服务端与客户端（`tui` / `run` / `status`）共用同一份配置来源，按「命令行 > 环境变量 > 配置文件 > 默认值」装载（D-012）。

| 项 | 说明 |
|---|---|
| 配置文件 | `<应用根目录>/config.toml`，内容为 `[server]` 下的 `addr` / `port` / `log_level`；文件不存在时全部取默认值 |
| 应用根目录 | `framework-core` 的 `ApplicationDirectory::user_in("agent", ".lingting")`：Windows 为 `%USERPROFILE%\.lingting\agent`；debug 构建为可执行文件同级的 `runtime` 目录 |
| 环境变量 | `AGENT_SERVER_ADDR`（接受 `host` / `host:port` / `http://host:port`）、`AGENT_SERVER_PORT`、`AGENT_LOG_LEVEL` |
| 应用根目录覆盖 | 环境变量 `AGENT_HOME` 指向的目录；设置后 `data`、`config.toml`、`logs` / `logs-cli` / `logs-tui` 全部落在该目录，默认位置不再使用（测试用它隔离每个服务端实例） |
| 默认值 | 监听 `127.0.0.1:20100`（release 构建）/ `127.0.0.1:20102`（debug 构建，见“固定端口”），日志级别 `info` |
| 目录字段 | `lib-core` 的 `AppDirectories`：`data`、`config_file`、`logs_server`、`logs_cli`、`logs_tui` |

约定：

- `--port 0` 视为参数错误：框架对 0 端口会改用随机端口，破坏客户端默认寻址（D-022）。
- 客户端用 `--server <ADDR>` 覆盖同一份来源，没有独立的端口参数；解析结果拼成 `http://host:port` 作为基地址。
- 配置文件解析失败按参数错误处理，错误信息包含文件路径。

## 日志

- 初始化入口只有 `lib_core::logger` 的 `init_server` / `init_cli` / `init_tui`：调用方传入 `LevelFilter`，并让返回的 `LoggingGuard` 存活到进程退出。
- 目录：服务端 `logs`、CLI `logs-cli`、TUI `logs-tui`，与 `config.toml`、`data/` 同级（D-023）。
- 服务端与 CLI 同时输出控制台；TUI 只写文件（控制台日志会破坏界面）。
- 归档保留 2 天，不写 `combined.log`。
- 文件日志为纯文本，控制台带 ANSI 颜色，两者除颜色外文字一致。

## 退出码

| 退出码 | 含义 |
|---|---|
| 0 | 正常退出（含 `--help` / `--version`） |
| 1 | 运行错误（监听失败、运行期错误） |
| 2 | 参数或配置错误（clap 解析失败、配置文件无法解析、端口为 0 等） |

## 客户端（CLI）

命令树定义在 `lib-bin-cli::command`，`bin-agent` 只做分发；客户端命令（`tui` / `run` / `status`）共用同一套地址解析与同一个 SDK Transport。

| 命令 | 说明 |
|---|---|
| `agent` / `agent tui [--server]` | 进入终端界面（无子命令时的默认行为，界面由 P1-015 实现） |
| `agent server [--addr] [--port] [--log-level]` | 启动 HTTP 服务（前台常驻） |
| `agent run <TASK> [--stream\|--json] [--server]` | 创建会话 → 发消息 → 消费 Run 事件流；`--stream` 逐段打印回复，`--json` 打印 `{sessionId, runId, status, content}` |
| `agent status [--server]` | 探活：服务地址、健康状态、版本与协议版本、运行时长、运行中 Run 数 |

传输与错误处理约定：

- HTTP 收发只经 `framework-http`（`reqwest` 后端，见 `docs/workspace.md` 的依赖表），客户端不直接使用 reqwest 发请求。
- 两个客户端分工：普通接口带 30 秒总超时；事件流不设总超时——一次回复可能持续很久，总超时会在回复中途截断，连接存活由服务端心跳（15 秒）与用户中断决定。
- 成功响应统一是 `R<T>`（`{"code":200,"message":"Success","data":<载荷>}`），Transport 负责解包后交给生成的 SDK；`Void` 的 204 空响应与 `Blob` 的原始响应体原样透传。
- 错误分类：连接被拒绝 / 超时 / 非 2xx（取 `ApiError.message`）/ 响应解析失败 / 不支持的请求形态。分类不依赖后端具体类型：连接被拒绝看错误链上的 `std::io::ErrorKind::ConnectionRefused`，超时看 `TimedOut` 与 reqwest 固定的 `operation timed out` 文案。
- 错误呈现统一为一行：非 JSON 错误体会折叠空白并截断到 200 字符，不把 HTML 或多行文本原样写到终端；退出码见上一节。
- 客户端不出现任何 path 字面量：地址只来自生成的 `ApiDefinition.path`。`Run` 事件流在元数据里是 `Blob`，生成的 `run_stream` 只能整体返回响应体，因此 `Client::subscribe_run` 用同一个 `RUN_STREAM` 常量的 path/method 走流式读取并自行解析 SSE 帧。

## SDK 生成

客户端 SDK（Rust / TypeScript）全部由服务端 API 定义生成，**不允许手写**。

| 项 | 说明 |
|---|---|
| 生成命令 | `make gen`（内部依次执行 `cargo run -p lib-web --example gen_rust_sdk` 与 `gen_ts_sdk`） |
| Rust 产物 | `packages/sdk-rust/src/{types,enums,api}.rs`，由 `lib-bin-cli` 用 `#[path]` 作为自身模块引入 |
| TS 产物 | `packages/sdk-ts/`（npm ESM 包），由 `lingting-agent-ui/pnpm-workspace.yaml` 的 `../packages/*` 引入 |
| 是否入库 | **不入库**（`packages/` 被 `.gitignore` 忽略，D-006）；改 API 后重新生成即可，没有“提交生成物”步骤 |
| 生成文件头 | Rust 侧写“自动生成，请勿手动修改” + `#![allow(warnings)]`；TS 侧由框架写入 Specta 的 do-not-edit 标记 |
| 失败行为 | 校验与源码构建先于写盘完成；写盘先落同级暂存目录（`.gen-tmp`），成功后才整体替换目标目录，失败以非 0 退出并保留上一次产物 |

执行顺序约束（重要）：

1. 干净 checkout 后**先**跑 `make gen`，再 `make build`；否则 `lib-bin-cli` 编译期找不到生成文件，报
   `couldn't read .../packages/sdk-rust/src/api.rs`。
2. 前端侧同样先 `make gen`，再 `make ui-install`；否则 `workspace:*` 解析不到 `packages/sdk-ts`。
3. 改了服务端 API 定义（`crates/lib-web` 下的 DTO / handler）后必须重新生成，不得手改生成物。

生成器当前是 `lib-web` 的 example（`crates/lib-web/examples/gen_{rust,ts}_sdk.rs`，输出目录处理在 `examples/support/sdk_out.rs`）；命令入口已由 P1-007 固化为根 `Makefile`。

## API 契约规范

**单一来源**：服务端 `#[web_api_*]` 标注的 handler 与 `#[auto_type]` / `#[auto_enum]` 标注的 DTO，全部位于 `crates/lib-web/src/`。客户端 SDK 完全由此生成，不存在第二份手写 API 定义。任何 API 变更先改这里。

**接口文档**：人类可读的接口说明（领域划分、接口清单、参数表与样例）放在 `docs/api/`，入口是 `docs/api/readme.md`，按领域一文件（`probe.md`、`session.md`）。它只做说明、不构成第二份契约，与代码不一致时以代码为准；改接口时同步对应领域文档。

### 改 API 的完整步骤

1. 改 `crates/lib-web/src/` 下的 handler 与 DTO。
2. 同步 `docs/api/` 中对应领域文档（接口清单、参数表、样例）；新增领域时同时登记到 `docs/api/readme.md` 的索引表。
3. `make gen` 重新生成两份 SDK；生成失败按提示修定义，不手改生成物。
4. `make check`（格式 + Clippy）与 `make build`（生成代码参与编译）通过。
5. 前端侧 `make ui-install` / `make ui-lint`；生成物不入库，提交里只有 API 定义与消费方代码。

### 命名规范

| 项 | 规范 |
|---|---|
| path 前缀 | 统一 `api/` 开头，不带版本段（见下） |
| path 资源段 | 小写单数名词，多词用 `-` 连接，如 `api/probe/echo` |
| path 动作段 | 固定动词：`create` / `update` / `delete` / `get` / `list` / `cancel` / `stream`，不造同义动词 |
| 元数据名 | 由 handler 函数名（snake_case）派生，两侧 SDK 的标识符同源派生，不手工改 |
| 请求 DTO | 请求体 `*PO`，查询参数 `*Params` |
| 响应 DTO | `*VO` |
| 枚举 | `<业务名>Kind`（与框架 `RCodeKind` 一致），生成侧为同名结构 + `<NAME>_ALL` / `<NAME>_MAP` 映射 |
| 字段 | camelCase；id 用 `String`（D-018）；`i64` 在 wire 上以字符串传输（D-018）；查询参数不出现 `bool`（D-021） |
| SSE 事件 | 端点是 `GET` + 查询参数，动作段用 `stream`；返回 `WebResponse::stream`，元数据里是 `Blob`（D-007），SDK 只导出 path 常量，事件类型由服务端枚举 + DTO 生成到两侧 SDK。判别字段统一 `event`，取值 snake_case，与 SSE 的 `event:` 行同值；帧编码用 `lib-web-core` 的 `sse` 模块，不手拼字符串（D-026） |

### 统一错误响应

所有失败响应由框架统一生成（`framework-web` 的 `WebResponse::from_error`），业务代码不手写错误响应体。

| 项 | 约定 |
|---|---|
| 响应结构 | `{"code": <HTTP 状态码>, "message": "<对外消息>"}`；`code` 与状态码同值，4xx 为客户端错误、5xx 为服务端错误，已分配取值只追加不复用 |
| 契约类型 | `ApiError { code, message }` 定义在 `crates/lib-web/src/error.rs`（`auto_type`），随生成进入两侧 SDK；客户端解析错误体只用它，不手写重复定义 |
| 错误分类 | 只用 `WebError::parameter` / `not_found` / `unauthorized` / `forbidden` / `internal` 表达，映射表与错误码清单见 `p1/decisions.md` D-024 |
| 内部细节 | 堆栈、SQL、文件路径、第三方错误一律放进 `WebError::internal(msg, source)` 的 `source`，只进日志；**禁止**用裸 `anyhow!` 携带内部细节（会归入 `WebErrorKind::Message` 并原样对外） |
| 探活接口 | `GET api/probe/health`（版本、协议版本、启动时间、运行时长、未结束的 Run 数量）与 `GET api/probe/version`（版本、协议版本），均显式 `AuthRule::anonymous()` |

`ApiError` 当前落在 `lib-web` 以便随 SDK 生成导出；将来 `lib-web-core` 落实 Error Mapping 时可整体迁入（见 D-024 与 `docs/workspace.md` 的 `lib-web-core` 职责）。

### 会话业务接口（P1 Mock）

P1-010 起的会话接口定义在 `crates/lib-web/src/session/api.rs`，业务实现在 `service-core`，契约类型在 `types-core` 的 `session` 模块；P2 只替换实现、不改契约。接口清单、参数表、载荷字段与样例见 `docs/api/session.md`；Mock 行为边界、错误映射与 P2 替换点见 `.steps/p1/decisions.md` D-025。

约定：会话与 Run 的 ID 走查询参数或请求体（无路径占位符）；`messageCount`、token 用量与 `*Ms` 时间戳按 D-018 以字符串传输；业务失败的 404（会话/Run 不存在）与 400（参数或状态不满足规则）由 `SessionError` 映射，错误体仍是统一的 `{code,message}`。

### API 版本管理

P1 不引入版本段：path 保持 `api/...`，不预留 `/api/v1`。将来需要多版本并存时采用 path 前缀（`api/v2/...`），不采用请求头版本化；届时按路径整体重排一次（D-019）。

## 前端工程

WebUI 工程位于 `lingting-agent-ui/`，同时是 pnpm workspace 根。

| 项 | 说明 |
|---|---|
| 工程目录 | `lingting-agent-ui/`（`pnpm-workspace.yaml` 在该目录内，`packages` 指向 `../packages/*`） |
| 技术栈 | React + Vite + TypeScript，UI 组件用 antd 与 `lingting-react-ui`（LRI） |
| 依赖安装 | 先跑 `make gen`，再执行 `make ui-install` |
| 开发启动 | `make ui-dev`（内部在 `lingting-agent-ui/` 执行 `pnpm dev`，固定端口 20101，见本文件端口表） |
| 后端代理 | 开发服务器把 `^/api/` 代理到 `http://127.0.0.1:20102`，不做路径重写（后端路由自带 `api/` 前缀） |
| SDK 引入 | 依赖项 `"@lingting/agent-api": "workspace:*"`，指向生成的 `packages/sdk-ts` |
| 代码检查 | `make ui-lint`（内部执行 `pnpm run lint`：`vp fmt` + `vp lint --fix` + `tsc --noEmit`） |
| 单元用例 | `lingting-agent-ui/tests/*.test.ts(x)`，jsdom 环境，由 `bin-tests` 的 `web.ui` 执行 `pnpm exec vp test run --maxWorkers 4`（用例从 `vite-plus/test` 导入，不额外装 vitest） |
| 端到端用例 | `lingting-agent-ui/tests/e2e/*.spec.ts`，Playwright + 真实服务端，由 `web.e2e` 执行 `pnpm exec playwright test`；`playwright.config.ts` 不启 `webServer`，服务端地址由 `WEBUI_BASE_URL` 传入（未设置时回落 `http://127.0.0.1:20102`） |
| 端到端浏览器 | `make ui-e2e-install`（`pnpm exec playwright install chromium`）；`@playwright/test` 版本与 `lingting-agent-flow-ui` 对齐 |
| 失败产物 | `tests/.artifacts/`（截图与追踪，已被 `.gitignore` 忽略） |
| SDK 调用约束 | 所有请求经生成的 `ApiClient` 子类（`src/api/client.ts`）；SSE 用 fetch 流式响应自行切帧（`src/api/runStream.ts`），path 取自 `ApiDefinitions`，页面代码不出现 `api/…` 字面量（由 `web.sdk-only` 用例守护） |
| 缩进 | 2 空格（根 `.editorconfig` 的 `[*.{ts,tsx,js,jsx,mjs,cjs,css,html}]` 段；`[*]` 默认 4 空格只用于 Rust 等） |

## 待补充
