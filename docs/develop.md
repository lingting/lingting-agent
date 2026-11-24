# 开发规范

## 开发命令（Makefile）

根目录 `Makefile` 是唯一命令入口；需要多步流程或中文提示的逻辑放在 `scripts/` 下的脚本里由 Makefile 调用（如 `scripts/gen-check.sh`），不写进 recipe。需在 Git Bash 中执行（recipe 使用 POSIX sh）。

| 命令 | 作用 |
|------|------|
| `make gen` | 由服务端 API 定义生成 Rust / TS 两份客户端 SDK（产物落 `packages/`，不入库） |
| `make build` | 编译 workspace 全部成员；`ARGS` 透传，如 `make build ARGS="--release"` |
| `make check` | `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`，不执行测试 |
| `make gen-check` | 生成物重建门禁：清空 `packages/` → `make gen` → 校验关键产物完整 → `cargo check --workspace --all-targets`；任一步失败非 0 退出，逻辑在 `scripts/gen-check.sh`（见“生成物门禁与 CI”一节） |
| `make ci` | 本地与 CI 共用的完整门禁：`gen-check` + `check` + `ui-check` |
| `make fmt` | `cargo fmt --all` |
| `make test ARGS="..."` | 生成 SDK、构建 `bin-agent` 后运行测试入口；运行期准备（如构建 WebUI 产物）由入口按选中用例自行完成。`ARGS` 传用例名前缀或 `--all`（详见“测试”一节） |
| `make clean-gen` | 删除生成物目录 `packages/` |
| `make ui-install` / `make ui-dev` / `make ui-build` / `make ui-lint` | 在 `lingting-agent-ui/` 内执行对应 pnpm 命令 |
| `make ui-check` | 前端静态类型检查（`pnpm exec tsc --noEmit`，只读不写，供门禁与 CI 使用） |
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
| 测试共享服务端 | 20104 | 测试入口启动的唯一共享服务端端口；隔离用例与自管用例（`stress.boundary` / `cli.async_server`）使用动态空闲端口 |

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
| 自管服务端 | 用例注册为 `SelfServed` 时自己决定服务端与 Mock 的启停与配置（入口只给目录与端口），适合需要注入 `[agent]` 参数或私有 Mock 的用例（`stress.boundary`、`cli.async_server`）；端口动态取空闲端口，不占用 20104 |
| 子进程文件 | 根目录为系统临时目录下的 `lingting-agent-tests/<运行 ID>/`：共享服务端用 `shard-shared/`，隔离用例用 `<用例名>/`；`server.log` 是子进程输出，`data/`、`config.toml`、`logs*/` 来自 `AGENT_HOME` |
| 服务端二进制 | 默认取测试入口同目录的 `bin-agent`，可用 `AGENT_TEST_SERVER_BIN` 覆盖；缺失时提示先执行 `make build` |
| 前端用例 | `web.ui` 转发 `pnpm exec vp test run --maxWorkers 4`（jsdom 单元用例），`web.e2e` 转发 `pnpm exec playwright test`（真实 Chromium）；两者都在 `lingting-agent-ui/` 内执行，命令写在 Rust 侧 `frontend` 模块，不依赖 package.json scripts |
| 浏览器依赖 | 端到端用例需要 Playwright 的 chromium：先执行 `make ui-e2e-install`；用例不会自动下载，缺失时报错原样透出 |
| 测试报告 | 全部用例结束后写入 `target/tests/<运行 ID>/README.md`（汇总与用例索引）；用例按领域汇总到同目录 `<领域>.md`，README 索引给出该用例在领域文档中的行号范围，领域文档顶部也提供行号索引；每条含结果、耗时、服务地址、日志目录与用例返回的结果信息 |

约定：

- 用例名为 `<领域>.<场景>`，放在 `crates/bin-tests/src/cases/<领域>.rs`，并在 `cases/mod.rs` 汇总；领域内用例变多时拆成 `cases/<领域>/` 子模块，由 `cases/<领域>.rs` 统一注册。
- 目录职责（P2.1-011 / D-P2.1-013）：`cases/` 只放用例（注册 + 断言 + 该用例独有的输入数据构造）；被两个以上用例复用的动作（HTTP/SSE 探测、子进程、临时目录、轮询等待、流程编排）放 `util/*.rs`（通用支持平铺，入口与调度框架 `main` / `cli` / `runner` / `report` / `case` 留顶层）；测试替身按被替身领域放 `mock/<领域>/`（当前只有 `mock/llm`）。依赖方向为 `cases → {util, mock}`、`mock → util`，`runner` / `util` / `mock` 不得依赖 `cases`。历史残留：`cases/web/*` 等领域内的探测 helper 仍留在领域模块里，未外移（口径见 `.steps/p2.1/decisions.md` D-P2.1-013）。
- 测试只写在 `bin-tests`：其他 crate 不写 `#[cfg(test)]` 单元测试，为测试提供的入口必须由 feature 隔离（如 `lib-bin-tui` 的 `testing`）且只在 `bin-tests` 启用。
- 用例优先只通过生成的 SDK（`lib-bin-cli::client::Client`）访问服务端，不手写 API 定义。**例外**：`web.*` 里断言静态资源、SPA 回落与 CORS 预检这类不经过 SDK 的路径，用 workspace 内的 `framework-http` 直发原始请求；接口 path 仍复用生成的 `PROBE_HEALTH` / `PROBE_SHUTDOWN` / `CLIENT_STREAM` 常量，只有「本就不存在」的路径保留字面量（那正是断言目标）。
- 前端用例分两层：`lingting-agent-ui/tests/*.test.ts(x)` 是 jsdom 单元用例（Transport、SSE 切帧、`useChat`、首屏渲染），`lingting-agent-ui/tests/e2e/*.spec.ts` 是 Playwright 端到端用例（真实浏览器 + 真实服务端）。前端命令由 `bin-tests` 的 `util::frontend` 模块执行，不在 `package.json` 里另设 `test` 脚本。
- 测试进程只作为客户端，不进程内启动服务端：`lifecycle` 关闭标记与日志初始化都是进程级单例，多实例会互相干扰。
- 一次性测试与临时脚本不进入仓库，回归用例一律登记到测试入口。

## 测试数据采集

端到端用例的数据只允许来自「编译期特性 + 进程内内存采集 + 结构化快照」（P2.2 起；约束条款见仓库根 `AGENTS.md` 的「测试数据采集」一节）。

| 项 | 说明 |
|---|---|
| 采集开关 | `metrics` 特性：`bin-agent` → `lib-bin-web` → `{agent-runtime, manager-agent, lib-web}` → `{lib-db, lib-core}`。`make test` 构建的 `bin-agent` 打开它；`cargo check -p bin-agent` / `make build` / `make release` 不开，此时采集代码是空实现，不占静态内存、不做原子操作 |
| 出口 | `GET api/probe/metrics`，`debug_assertions` + `metrics` 双限定，返回 `manager-agent` 的 `MetricsSnapshot`；用例侧由 `util::server::capture` 反序列化同一个类型，不复制 DTO |
| 采集点 | 各层在自己的 Crate 里累加原子量：清理链 / 会话停止 / 落盘 / 下行丢弃 / 会话锁 / 注册表 / 数据库取连接在 `manager-agent` 与 `lib-db`，Run 排队与执行在 `agent-runtime`（由 `lib-bin-web` 装配时以函数指针注册进 `manager-agent`），日志审计在 `lib-core` |
| 渲染点 | `bin-tests` 的 `util::observability`：本进程快照 + 用例自己的服务端快照 → 报告章节；全仓只有这一个渲染点 |
| 缺失 | 404（构建未开特性）/ 进程已退出 / 字段不匹配都记作「未采集：原因」写进报告；用例要么显式失败并点名缺失字段，要么在报告里写原因，不静默跳过断言 |
| 禁止 | 读取、解析或文本检索 `server.log` 与日志目录来断言或出报告；为采集而打的阈值日志（`warn!` / `debug!`）不得重新引入，日志只保留带 `run_id` / `session_id` 的运行期流程事件 |

快照字段清单见 `docs/api/probe.md` 的「MetricsSnapshot」一节。

## 配置装载

服务端与客户端（`tui` / `run` / `status`）共用同一份配置来源，按「命令行 > 配置文件 > 默认值」装载（D-012）。**配置项一律不由环境变量提供**（P2-017），唯一的环境变量是应用根目录覆盖。

| 项 | 说明 |
|---|---|
| 配置文件 | `<应用根目录>/config.toml`：`[server]` 的 `addr` / `port` / `log_level`、`[runtime.events]` 的事件文件轮转、`[provider]` / `[[provider.models]]` 的模型清单、`[agent]` 的运行参数；文件不存在时全部取默认值（`[provider]` 缺省即「Provider 未配置」，服务端照常启动） |
| `[provider]` / `[[provider.models]]` | 只读 Provider 配置：`id` / `kind`（只接受 `openai-compatible`）/ `base_url` / `api_key` / `default_model`；每个模型有 `id` / `context_window`（缺省 8192）/ `max_output_tokens`（缺省 1024）。段一旦存在，缺字段、未知 `kind`、`default_model` 不在 `models` 中、`models` 为空或 id 重复、数值非正或 `max_output_tokens > context_window` 都是启动期硬错误（退出码 2） |
| `[agent]` | 运行参数：`max_concurrent_runs`（缺省 `"3C"`，即 3 × CPU 核数；也可写整数如 `4` 表示绝对许可数）、`run_queue_timeout_ms`（缺省 120000）、`llm_idle_timeout_ms`（缺省 60000）、`context_max_messages`（缺省 40）、`context_safety_tokens`（缺省 256）。核数在装配时取一次（`available_parallelism()`），运行期不变；`0` / `0C` / 非法后缀 / 负数 / 超过信号量上限都是启动期硬错误（退出码 2） |
| 系统提示词 | `<应用根目录>/AGENTS.md`：名称全大写优先，其次忽略大小写的 Markdown 文件；每次组装 Context 现场读取、不缓存；缺失或为空时不发 system 消息 |
| 应用根目录 | `framework-core` 的 `ApplicationDirectory::user_in("agent", ".lingting")`：Windows 为 `%USERPROFILE%\.lingting\agent`；debug 构建为可执行文件同级的 `runtime` 目录 |
| 环境变量 | 只有 `AGENT_HOME`（应用根目录覆盖）；地址 / 端口 / 日志级别等配置项不再支持环境变量 |
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
- `agent server -a` / `--async` 派生的常驻进程：其 `stdout` / `stderr` 追加写 `<应用根目录>/logs/stdout.log`，PID 与服务地址写在 `<应用根目录>/server.pid`（两行 `pid = …` / `address = …`，探活通过后才写）。本阶段不提供 `--stop`（D-P2.2-004），Windows 用 `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` 脱钩，Unix 只做 stdio 重定向，终端关闭可能收到 SIGHUP（D-P2.2-005）。

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
| `agent server [--addr] [--port] [--log-level] [-a\|--async]` | 启动 HTTP 服务（前台常驻）；`-a` / `--async` 改为派生一个常驻服务进程，探活通过后打印 PID、服务地址、日志目录与 PID 文件后退出 |
| `agent run <TASK> [--stream\|--json] [--server]` | 注册客户端 → 建下行流 → 创建会话 → 占用会话 → 发消息 → 消费下行流 → 释放占用 → 反注册；`--stream` 逐段打印回复，`--json` 打印 `{sessionId, runId, status, content}` |
| `agent status [--server]` | 探活：服务地址、健康状态、版本与协议版本、运行时长、运行中 Run 数 |

传输与错误处理约定：

- HTTP 收发只经 `framework-http`（`reqwest` 后端，见 `docs/workspace.md` 的依赖表），客户端不直接使用 reqwest 发请求。
- 两个客户端分工：普通接口带 30 秒总超时；事件流不设总超时——一次回复可能持续很久，总超时会在回复中途截断，连接存活由客户端心跳（每 15 秒 `api/client/update {PING}`，服务端 45 秒无心跳即判超时）与用户中断决定。
- 成功响应统一是 `R<T>`（`{"code":200,"message":"Success","data":<载荷>}`），Transport 负责解包后交给生成的 SDK；`Void` 的 204 空响应与 `Blob` 的原始响应体原样透传。
- 错误分类：连接被拒绝 / 超时 / 非 2xx（取 `ApiError.message`）/ 响应解析失败 / 不支持的请求形态。分类不依赖后端具体类型：连接被拒绝看错误链上的 `std::io::ErrorKind::ConnectionRefused`，超时看 `TimedOut` 与 reqwest 固定的 `operation timed out` 文案。
- 错误呈现统一为一行：非 JSON 错误体会折叠空白并截断到 200 字符，不把 HTML 或多行文本原样写到终端；退出码见上一节。
- 客户端不出现任何 path 字面量：地址只来自生成的 `ApiDefinition.path`。下行流 `api/client/stream` 在元数据里是 `Blob`，生成的 `client_stream` 只能整体返回响应体，因此 `Client::subscribe_client` 用同一个 `CLIENT_STREAM` 常量的 path/method 走流式读取，并由 `EventStream` 自行解析 SSE 帧（`: keepalive` 注释帧直接丢弃）。
- 事件推送只有 `api/client/stream` 一条通道（`.steps/p1.1/decisions.md` D-P1.1-004）：客户端自行编排「注册 → 建流 → 占用 → 发消息 → 消费 → 对账」，不存在 Run 级事件流。

## SDK 生成

客户端 SDK（Rust / TypeScript）全部由服务端 API 定义生成，**不允许手写**。

| 项 | 说明 |
|---|---|
| 生成命令 | `make gen`（内部执行 `cargo run -p lib-web --example gen_sdk`，同进程依次生成 Rust 与 TS，省掉一次 cargo 启动） |
| Rust 产物 | `packages/sdk-rust/src/{types,enums,api}.rs`，由 `lib-bin-cli` 用 `#[path]` 作为自身模块引入 |
| TS 产物 | `packages/sdk-ts/`（npm ESM 包），由 `lingting-agent-ui/pnpm-workspace.yaml` 的 `../packages/*` 引入 |
| 是否入库 | **不入库**（`packages/` 被 `.gitignore` 忽略，D-006）；改 API 后重新生成即可，没有“提交生成物”步骤 |
| 生成文件头 | Rust 侧写“自动生成，请勿手动修改” + `#![allow(warnings)]`；TS 侧由框架写入 Specta 的 do-not-edit 标记 |
| 失败行为 | 校验与源码构建先于写盘完成；写盘先落同级暂存目录（`.gen-tmp`），成功后整体替换目标目录，失败以非 0 退出并保留上一次产物 |
| 重复生成 | 提交前按内容逐字节比对：与上次完全一致时只删暂存目录、不写盘（不刷新 mtime，避免 cargo 的 `#[path]` 依赖与前端依赖图无谓重编译）；有任一差异则整体替换目录，不做逐文件同步 |

执行顺序约束（重要）：

1. 干净 checkout 后**先**跑 `make gen`，再 `make build`；否则 `lib-bin-cli` 编译期找不到生成文件，报
   `couldn't read .../packages/sdk-rust/src/api.rs`。
2. 前端侧同样先 `make gen`，再 `make ui-install`；否则 `workspace:*` 解析不到 `packages/sdk-ts`。
3. 改了服务端 API 定义（`crates/lib-web` 下的 DTO / handler）后必须重新生成，不得手改生成物。

生成器当前是 `lib-web` 的 example（`crates/lib-web/examples/gen_{rust,ts}_sdk.rs`，输出目录处理在 `examples/support/sdk_out.rs`）；命令入口已由 P1-007 固化为根 `Makefile`。

## API 契约规范

**单一来源**：服务端 `#[web_api_*]` 标注的 handler 与 `#[auto_type]` / `#[auto_enum]` 标注的 DTO，全部位于 `crates/lib-web/src/`。客户端 SDK 完全由此生成，不存在第二份手写 API 定义。任何 API 变更先改这里。

**接口文档**：人类可读的接口说明（领域划分、接口清单、参数表与样例）放在 `docs/api/`，入口是 `docs/api/readme.md`，按领域一文件（`probe.md`、`session.md`、`runtime.md`、`occupancy.md`）。它只做说明、不构成第二份契约，与代码不一致时以代码为准；改接口时同步对应领域文档。

### 改 API 的完整步骤

1. 改 `crates/lib-web/src/` 下的 handler 与 DTO。
2. 同步 `docs/api/` 中对应领域文档（接口清单、参数表、样例）；新增领域时同时登记到 `docs/api/readme.md` 的索引表。
3. `make gen` 重新生成两份 SDK；生成失败按提示修定义，不手改生成物。
4. `make gen-check`（清空重建 + 产物校验 + 生成代码参与编译）与 `make check`（格式 + Clippy）通过。
5. 前端侧 `make ui-install` / `make ui-check`（只读类型检查；`make ui-lint` 会 `--fix`，本地随手修时用）；生成物不入库，提交里只有 API 定义与消费方代码。

### 命名规范

| 项 | 规范 |
|---|---|
| path 前缀 | 统一 `api/` 开头，不带版本段（见下） |
| path 资源段 | 小写单数名词，多词用 `-` 连接，如 `api/probe/echo` |
| path 动作段 | 固定动词：`create` / `update` / `delete` / `get` / `list` / `cancel` / `stream`，不造同义动词 |
| 元数据名 | 由 handler 函数名（snake_case）派生，两侧 SDK 的标识符同源派生，不手工改 |
| 请求 DTO | 请求体 `*PO`，查询参数 `*Params` |
| handler 参数 | 请求体与查询参数一律解构到 `po`：`Json(po): Json<*PO>`、`Query(po): Query<*Params>`；函数体内直接用 `po`，不引入 `params` / `query` 之类的中间变量，也不写 `.0` 二次取值 |
| 响应 DTO | `*VO` |
| 枚举 | `<业务名>Kind`（与框架 `RCodeKind` 一致），生成侧为同名结构 + `<NAME>_ALL` / `<NAME>_MAP` 映射 |
| 字段 | camelCase；id 用 `String`（D-018）；`i64` 在 wire 上以字符串传输（D-018）；查询参数不出现 `bool`（D-021） |
| SSE 事件 | 端点是 `GET` + 查询参数，动作段用 `stream`（P1.1 起只有 `api/client/stream` 一条）；返回 `WebResponse::stream`，元数据里是 `Blob`（D-007），SDK 只导出 path 常量，事件类型由服务端枚举 + DTO 生成到两侧 SDK。判别字段统一 `event`，取值 snake_case，与 SSE 的 `event:` 行同值；帧编码用 `lib-web-core` 的 `sse` 模块，不手拼字符串；空闲保活由 `lib-web` 的帧合成层注入 `: keepalive` 注释帧（D-P1.1-001） |

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

### 会话业务接口（P2 起真实执行）

会话接口定义在 `crates/lib-web/src/session/api.rs`，业务实现在 `service-core`（P2 起为 Service + Repository，数据落 `<应用根目录>/data/agent.db`），契约类型在 `agent-core`。P2 只换掉了执行内容（Mock 事件 → 真实 Agent Loop），接口、DTO 与 `RunEventKind` 取值没有变化：执行端口 `RunDriver` 由 `service-core` 定义、`agent-runtime` 实现、`lib-bin-web` 装配时注册。接口清单、参数表、载荷字段与样例见 `docs/api/session.md`。

P2 起的三处语义变化（字段与接口不变）：

- `run_started.inputTokens` 是预算估算器的估算值，`run_completed` 的 `inputTokens` / `outputTokens` 用 Provider 的真实 usage；Provider 没给用量时留空，不构造假值。
- `run_failed` 由执行层真实产出：`error` 是固定脱敏文案（如「调用模型失败：认证失败」），失败的 Run 不写助手消息。
- 未被占用的会话里 Run 停在 `QUEUED`；`max_concurrent_runs` 已满时同样排队，排队超时落 `FAILED`「执行排队超时」。

约定：会话与 Run 的 ID 走查询参数或请求体（无路径占位符）；`messageCount`、token 用量与 `*Ms` 时间戳按 D-018 以字符串传输；业务失败的 404（会话/Run 不存在）与 400（参数或状态不满足规则）由 `SessionError` 映射，错误体仍是统一的 `{code,message}`。

### 运行时接口（P1.1）

客户端连接与会话占用的接口定义在 `crates/lib-web/src/runtime/{api,occupancy_api,stream}.rs`，业务实现在 `manager-agent` 的 `runtime` 模块（P2-013 起；`service-core` 不再持有进程内运行时），契约类型在 `types-core` 的 `runtime` 模块。接口清单、帧格式与样例见 `docs/api/runtime.md` 与 `docs/api/occupancy.md`；决策与参数见 `.steps/p1.1/decisions.md`。

约定：服务端推送只有 `api/client/stream` 一条通道（`api/run/stream` 已移除）；Run 的执行由「会话被占用 + 消息创建」触发，未被占用的会话里 Run 停在 `QUEUED`；未注册与占用冲突一律 400 + 固定 message（过渡方案，映射集中在 `crates/lib-web/src/runtime/mod.rs` 的一个函数），客户端按 message 识别而不是按 code 猜测。

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
| 代码检查 | `make ui-lint`（内部执行 `pnpm run lint`：`vp fmt` + `vp lint --fix` + `tsc --noEmit`，会改文件）；只读检查用 `make ui-check`（`tsc --noEmit`），门禁与 CI 用后者 |
| 单元用例 | `lingting-agent-ui/tests/*.test.ts(x)`，jsdom 环境，由 `bin-tests` 的 `web.ui` 执行 `pnpm exec vp test run --maxWorkers 4`（用例从 `vite-plus/test` 导入，不额外装 vitest） |
| 端到端用例 | `lingting-agent-ui/tests/e2e/*.spec.ts`，Playwright + 真实服务端，由 `web.e2e` 执行 `pnpm exec playwright test`；`playwright.config.ts` 不启 `webServer`，服务端地址由 `WEBUI_BASE_URL` 传入（未设置时回落 `http://127.0.0.1:20102`） |
| 端到端浏览器 | `make ui-e2e-install`（`pnpm exec playwright install chromium`）；`@playwright/test` 版本与 `lingting-agent-flow-ui` 对齐 |
| 失败产物 | `tests/.artifacts/`（截图与追踪，已被 `.gitignore` 忽略） |
| SDK 调用约束 | 所有请求经生成的 `ApiClient` 子类（`src/api/client.ts`）；下行流用 fetch 流式响应自行切帧（`src/api/clientStream.ts`），path 取自 `ApiDefinitions.clientStream`，页面代码不出现 `api/…` 字面量（由 `web.sdk-only` 用例守护） |
| 缩进 | 2 空格（根 `.editorconfig` 的 `[*.{ts,tsx,js,jsx,mjs,cjs,css,html}]` 段；`[*]` 默认 4 空格只用于 Rust 等） |
| 控制台断言 | 端到端用例收集 `console.error` 与 `pageerror` 并断言为空（W-1 结案），关服场景只放行浏览器自己上报的连接失败文案；断言工具在 `tests/e2e/console.ts` |

## 生成物门禁与 CI

生成物不入库（D-006），因此门禁不做 git diff 比对，判据只有三条：`make gen` 以 0 退出、关键产物存在且非空、生成物参与编译的 `cargo check` 通过；前端静态类型检查另由 `make ui-check` 负责。

| 项 | 说明 |
|---|---|
| 门禁命令 | `make gen-check`（逻辑在 `scripts/gen-check.sh`）：清空 `packages/` → `make gen` → 校验 10 份关键产物（`sdk-rust/src/*.rs`、`sdk-ts/package.json` 与 `dist/*`）→ `cargo check --workspace --all-targets` |
| 完整门禁 | `make ci` = `gen-check` + `check`（fmt + Clippy）+ `ui-check`（`tsc --noEmit`） |
| 失败提示 | 统一为「缺少生成物或生成失败，请运行 `make gen` 重新生成」，并按失败步给出处理建议；生成物不入库，不存在“提交生成物”这一步 |
| CI | `.github/workflows/ci.yml`（ubuntu-latest，`master` 与 PR）：只做检出、Rust 工具链（按 `rust-toolchain.toml` 固定版本）与前端依赖安装，随后执行 `make gen` → `make ui-install` → `make ci` |
| 本地等价 | 本地照抄 CI 的三条命令即可复现同一结果；`make check` 保持“fmt + Clippy”的轻量语义，不承担重建 |
| framework 依赖 | workspace 目前以 path 引入同级 `lingting-rust-framework` 仓库（D-015），发布前改为 version 引入；workflow 不为该路径做检出处理 |

约定：

- 门禁失败时按提示修 API 定义或生成器，不手改 `packages/` 下的生成物。
- `gen-check` 每次清空并重建 `packages/`，下游 cargo 必然重编一次，耗时高于 `make check`；日常改动用 `make check`，改 API 定义或提交前用 `make ci`。
- 门禁不证明生成物与某个历史基线一致，只证明“生成器可重建、产物完整、可编译”。

## 待补充
