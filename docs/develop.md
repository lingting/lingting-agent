# 开发规范

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

## SDK 生成

客户端 SDK（Rust / TypeScript）全部由服务端 API 定义生成，**不允许手写**。

| 项 | 说明 |
|---|---|
| 生成命令 | `scripts/gen.sh`（内部依次执行 `cargo run -p lib-web --example gen_rust_sdk` 与 `gen_ts_sdk`） |
| Rust 产物 | `packages/sdk-rust/src/{types,enums,api}.rs`，由 `lib-bin-cli` 用 `#[path]` 作为自身模块引入 |
| TS 产物 | `packages/sdk-ts/`（npm ESM 包），由 `lingting-agent-ui/pnpm-workspace.yaml` 的 `../packages/*` 引入 |
| 是否入库 | **不入库**（`packages/` 被 `.gitignore` 忽略，D-006 已修订） |
| 生成文件头 | 自动生成标记 + `#![allow(warnings)]`，禁止手改 |

执行顺序约束（重要）：

1. 干净 checkout 后**先**跑 `scripts/gen.sh`，再 `cargo build`；否则 `lib-bin-cli` 编译期找不到生成文件，报
   `couldn't read .../packages/sdk-rust/src/api.rs`。
2. 前端侧同样先 `scripts/gen.sh`，再在 `lingting-agent-ui/` 内执行 `pnpm install`；否则 `workspace:*` 解析不到 `packages/sdk-ts`。
3. 改了服务端 API 定义（`crates/lib-web` 下的 DTO / handler）后必须重新生成，不得手改生成物。

生成器当前是 `lib-web` 的 example（临时形态），命令与产物的最终形态由 `.steps/p1/tasks.md` P1-007 固化。

## 前端工程

WebUI 工程位于 `lingting-agent-ui/`，同时是 pnpm workspace 根。

| 项 | 说明 |
|---|---|
| 工程目录 | `lingting-agent-ui/`（`pnpm-workspace.yaml` 在该目录内，`packages` 指向 `../packages/*`） |
| 技术栈 | React + Vite + TypeScript，UI 组件用 antd 与 `lingting-react-ui`（LRI） |
| 依赖安装 | 先跑 `scripts/gen.sh`，再在 `lingting-agent-ui/` 内执行 `pnpm install` |
| 开发启动 | 在 `lingting-agent-ui/` 内执行 `pnpm dev`（固定端口 20101，见本文件端口表） |
| 后端代理 | 开发服务器把 `^/api/` 代理到 `http://127.0.0.1:20102`，不做路径重写（后端路由自带 `api/` 前缀） |
| SDK 引入 | 依赖项 `"@lingting/agent-api": "workspace:*"`，指向生成的 `packages/sdk-ts` |
| 代码检查 | `pnpm run lint`（`vp fmt` + `vp lint --fix` + `tsc --noEmit`，全部在前端目录内执行） |
| 缩进 | 2 空格（根 `.editorconfig` 的 `[*.{ts,tsx,js,jsx,mjs,cjs,css,html}]` 段；`[*]` 默认 4 空格只用于 Rust 等） |

## 待补充
