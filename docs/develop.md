# 开发规范

## 开发命令（Makefile）

根目录 `Makefile` 是唯一命令入口，不再提供 `scripts/` 脚本；需在 Git Bash 中执行（recipe 使用 POSIX sh）。

| 命令 | 作用 |
|------|------|
| `make gen` | 由服务端 API 定义生成 Rust / TS 两份客户端 SDK（产物落 `packages/`，不入库） |
| `make build` | 编译 workspace 全部成员；`ARGS` 透传，如 `make build ARGS="--release"` |
| `make check` | `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`，不执行测试 |
| `make fmt` | `cargo fmt --all` |
| `make clean-gen` | 删除生成物目录 `packages/` |
| `make ui-install` / `make ui-dev` / `make ui-build` / `make ui-lint` | 在 `lingting-agent-ui/` 内执行对应 pnpm 脚本 |

`make help` 的文本放在 `.make/help.txt`：make 在 Windows 上按 ANSI 代码页解析 Makefile，中文写在 Makefile 的 recipe 里会变成乱码。

## 固定端口

项目各进程一律使用固定端口，不使用随机端口。

| 服务 | 端口 | 说明 |
|------|------|------|
| lingting-agent | 20100 | 服务端端口 |
| 开发环境 WebUI | 20101 | 开发环境 WebUI 端口 |
| 开发环境服务端 | 20102 | 开发环境服务端端口 |
| API 探针服务端 | 20103 | API 探针服务端端口 |

约定：

- 端口冲突时直接报错退出，不静默改用随机端口。
- 端口变更必须同时更新本表与 `.steps/p1/decisions.md` 的 D-022。

## 配置装载

服务端配置按「命令行 > 环境变量 > 配置文件 > 默认值」装载（D-012）。

| 项 | 说明 |
|---|---|
| 配置文件 | `<应用根目录>/config.toml`，内容为 `[server]` 下的 `addr` / `port` / `log_level`；文件不存在时全部取默认值 |
| 应用根目录 | `framework-core` 的 `ApplicationDirectory::user_in("agent", ".lingting")`：Windows 为 `%USERPROFILE%\.lingting\agent`；debug 构建为可执行文件同级的 `runtime` 目录 |
| 环境变量 | `AGENT_SERVER_ADDR`（接受 `host` / `host:port` / `http://host:port`）、`AGENT_SERVER_PORT`、`AGENT_LOG_LEVEL` |
| 默认值 | 监听 `127.0.0.1:20100`，日志级别 `info` |
| 目录字段 | `lib-core` 的 `AppDirectories`：`data`、`config_file`、`logs_server`、`logs_cli`、`logs_tui` |

约定：

- `--port 0` 视为参数错误：框架对 0 端口会改用随机端口，破坏客户端默认寻址（D-022）。
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

### 改 API 的完整步骤

1. 改 `crates/lib-web/src/` 下的 handler 与 DTO。
2. `make gen` 重新生成两份 SDK；生成失败按提示修定义，不手改生成物。
3. `make check`（格式 + Clippy）与 `make build`（生成代码参与编译）通过。
4. 前端侧 `make ui-install` / `make ui-lint`；生成物不入库，提交里只有 API 定义与消费方代码。

### 命名规范

| 项 | 规范 |
|---|---|
| path 前缀 | 统一 `api/` 开头，不带版本段（见下） |
| path 资源段 | 小写单数名词，多词用 `-` 连接，如 `api/probe/echo` |
| path 动作段 | 固定动词：`create` / `update` / `delete` / `get` / `list` / `cancel`，不造同义动词 |
| 元数据名 | 由 handler 函数名（snake_case）派生，两侧 SDK 的标识符同源派生，不手工改 |
| 请求 DTO | 请求体 `*PO`，查询参数 `*Params` |
| 响应 DTO | `*VO` |
| 枚举 | `<业务名>Kind`（与框架 `RCodeKind` 一致），生成侧为同名结构 + `<NAME>_ALL` / `<NAME>_MAP` 映射 |
| 字段 | camelCase；id 用 `String`（D-018）；`i64` 在 wire 上以字符串传输（D-018）；查询参数不出现 `bool`（D-021） |
| SSE 事件 | P1 尚未建模（D-007）：事件端点按 `Blob` 返回，事件类型由服务端枚举生成到两侧 SDK，事件名字段统一 `event` |

### 统一错误响应

所有失败响应由框架统一生成（`framework-web` 的 `WebResponse::from_error`），业务代码不手写错误响应体。

| 项 | 约定 |
|---|---|
| 响应结构 | `{"code": <HTTP 状态码>, "message": "<对外消息>"}`；`code` 与状态码同值，4xx 为客户端错误、5xx 为服务端错误，已分配取值只追加不复用 |
| 契约类型 | `ApiError { code, message }` 定义在 `crates/lib-web/src/error.rs`（`auto_type`），随生成进入两侧 SDK；客户端解析错误体只用它，不手写重复定义 |
| 错误分类 | 只用 `WebError::parameter` / `not_found` / `unauthorized` / `forbidden` / `internal` 表达，映射表与错误码清单见 `p1/decisions.md` D-024 |
| 内部细节 | 堆栈、SQL、文件路径、第三方错误一律放进 `WebError::internal(msg, source)` 的 `source`，只进日志；**禁止**用裸 `anyhow!` 携带内部细节（会归入 `WebErrorKind::Message` 并原样对外） |
| 探活接口 | `GET api/probe/health`（版本、协议版本、启动时间、运行时长）与 `GET api/probe/version`（版本、协议版本），均显式 `AuthRule::anonymous()` |

`ApiError` 当前落在 `lib-web` 以便随 SDK 生成导出；将来 `lib-web-core` 落实 Error Mapping 时可整体迁入（见 D-024 与 `docs/workspace.md` 的 `lib-web-core` 职责）。

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
| 缩进 | 2 空格（根 `.editorconfig` 的 `[*.{ts,tsx,js,jsx,mjs,cjs,css,html}]` 段；`[*]` 默认 4 空格只用于 Rust 等） |

## 待补充
