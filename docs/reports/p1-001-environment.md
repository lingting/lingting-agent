# P1-001 仓库与开发环境勘察报告

本报告是 P1-001 的交付物。只记录**实测事实**，并严格区分：

- **已存在**：仓库里真实存在的文件与工程状态。
- **已验证**：实际检查过（含源码阅读、registry index 查询）的结论。
- **未验证**：仅凭文档推断或本阶段未执行编译验证的内容。

勘察时间点：`.steps/p1/tasks.md` 中 P1-001 执行时。

---

## 1. 仓库现状

### 1.1 Git

| 项 | 实测值 |
|---|---|
| 分支 | `master`（跟踪 `origin/master`，工作区干净） |
| 远程 | `git@github.com:lingting/lingting-agent.git`（SSH） |
| 最新提交 | `4a140be` `:memo: 包结构确定` |
| 前一提交 | `3bd14d5` `:tada: 路线图` |
| `.gitignore` | **不存在** |

> 注意：`.steps/` 文档中「本路线图创建时为空仓库：仅有 `.git`，无提交、无源码、无文档」的描述已过期，本次已修正。

### 1.2 目录与文件（已存在）

```text
.
├── .git/
├── .steps/
│   ├── README.md
│   ├── p1/{README.md, tasks.md, acceptance.md, decisions.md}
│   └── p2..p6/README.md
├── docs/
│   ├── develop.md        （占位）
│   └── workspace.md      （架构唯一事实来源）
├── AGENTS.md
└── README.md
```

### 1.3 工程状态（不存在的内容）

以下内容**确实不存在**，不得在文档中描述为已存在：

- 无 `Cargo.toml`、无 `Cargo.lock`、无 `rust-toolchain.toml`、无 `rustfmt.toml`。
- 无 Rust 源码（无 `src/`、无任何 `.rs` 文件）。
- 无任何 Crate / Workspace（`docs/workspace.md` 中的 Crate 全部是**规划**）。
- 无前端工程（无 `package.json`、无 Vite 配置、无 `node_modules`）。
- 无构建脚本（无 `xtask`、无 `justfile`、无 CI 配置）。
- 无 `.gitignore`。

**结论**：仓库当前处于「纯文档 + 已定架构」状态，`docs/workspace.md` 描述的 8 个 agent-*、7 个 lib-*、1 个 bin-* 全部属于**目标架构**，尚无一实现。

---

## 2. 工具链现状

### 2.1 Rust

| 项 | 实测值 |
|---|---|
| rustc | `1.98.0 (88d9e12ae 2026-08-18)` |
| cargo | `1.98.0 (797e8a9bc 2026-08-05)` |
| rustfmt | `1.9.0-stable` ✅ |
| clippy | 已安装 ✅（`cargo clippy --version` 正常） |
| 默认 host | `x86_64-pc-windows-msvc` |
| rustup home | `D:\.home\rustup` |
| cargo home | `D:\.home\.cargo` |

已安装 toolchain：

```text
stable-x86_64-pc-windows-msvc   （active, default）
nightly-x86_64-pc-windows-msvc
1.84.0-x86_64-pc-windows-msvc
```

已安装 std target：

```text
wasm32-unknown-unknown
x86_64-apple-darwin
x86_64-pc-windows-msvc
x86_64-unknown-linux-gnu
x86_64-unknown-linux-musl
```

已安装组件（默认 toolchain）：`rustc`、`cargo`、`rust-std`（上述 5 个 target）、`rustfmt`、`clippy`、`rust-docs`、`rust-src`。

**edition 2024 兼容性结论**：本机 rustc 1.98.0 ≥ edition 2024 的最低要求（1.85），且与 `framework-*` 的 `rust-version = "1.98.0"` **精确匹配**，无需升级工具链。

### 2.2 附加构建工具（已存在，非安装动作）

| 工具 | 状态 |
|---|---|
| `cargo-zigbuild` | 已安装（`cargo zigbuild` 可用） |
| `sccache` | `0.18.0`，且 `~/.cargo/config.toml` 全局设置 `rustc-wrapper = "sccache"` |
| `cross` | **未安装** |
| `cargo-bloat` / `cargo-release` / `cargo-workspaces` | 已安装 |

### 2.3 Cargo Registry 配置（实测 `D:\.home\.cargo\config.toml`）

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'
[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
[net]
git-fetch-with-cli = true
[build]
rustc-wrapper = "sccache"
```

- crates.io 通过 **rsproxy.cn 稀疏镜像**访问，可用。
- 本机 registry 缓存目录：`D:\.home\.cargo\registry\cache\rsproxy.cn-e3de039b2554c837\`。
- 官方 `index.crates.io` 无本地缓存（未使用过）。

### 2.4 Node.js / 前端

| 项 | 实测值 |
|---|---|
| node | `v22.22.3`（经 volta 管理） |
| npm | `10.9.8` |
| pnpm | `11.24.0` |
| pnpm registry | `https://registry.npmmirror.com/` |
| 全局 TypeScript (`tsc`) | **未安装**（全局包仅 `@openai/codex`、`corepack`、`npm`） |
| 前端工程 | **不存在** |

**结论**：Node 运行时齐备，可支撑 Vite 构建与 TypeScript 类型检查；但缺少 `typescript` 可执行文件，需要在 P1-005 / P1-016 时作为**项目内 devDependency** 安装（属于工程内依赖，不涉及全局工具链升级）。WebUI 框架仍未定（D-010），本次未创建任何前端工程。

---

## 3. 依赖版本矩阵

> 「本地验证」一律指**本机实际编译验证**。本次未创建任何 Cargo 工程，因此全部标记为「未验证」。版本可用性来自 crates.io 稀疏索引（`index.crates.io`）实时查询，并与 rsproxy 镜像交叉核对一致。

### 3.1 framework-*（P1 主干）

| Crate | 文档版本 | Registry 可用版本 | 本地源码版本 | 本地验证 | 备注 |
|---|---|---|---|---|---|
| `framework-core` | 26.11.0 | 26.8.150 / 26.8.151 / **26.11.0** | 26.11.0 | 未验证 | 提供 logging 等基础设施 |
| `framework-proc-core` | 26.11.0 | 26.8.145 / 26.8.150 / 26.8.151 / **26.11.0** | 26.11.0 | 未验证 | 硬钉 `specta = "=2.0.0-rc.25"`、`specta-serde ^0.0.12`、`specta-typescript ^0.0.12` |
| `framework-proc-ts` | 26.11.0 | 26.8.150 / 26.8.151 / **26.11.0** | 26.11.0 | 未验证 | proc-macro；`collect` → `framework-proc-core/collect` |
| `framework-proc-web` | 26.11.0 | 26.8.145 / 26.8.150 / 26.8.151 / **26.11.0** | 26.11.0 | 未验证 | proc-macro；`collect` 为空开关（只控制 `push_web_api!`） |
| `framework-web` | 26.11.0 | 26.8.150 / 26.8.151 / **26.11.0** | 26.11.0 | 未验证 | `collect` → `dep:inventory` + `framework-proc-web/collect` |
| `framework-web-axum` | 26.11.0 | 26.8.150 / 26.8.151 / **26.11.0** | 26.11.0 | 未验证 | 默认启用 `collect` |
| `framework-proc-rust` | 规划新增 | **不存在** | **不存在** | 未验证 | 需在 P1-006 新建 |

**关键修正**：`.steps` 与 `decisions.md` 中「registry 上 web 系列只到 26.8.150」的结论来自**本机 cargo 缓存快照**，不是发布状态。实测索引显示 **26.11.0 五个 crate 全部已发布**（含 `collect` feature，与本地源码一致）。本机缓存仅对 `framework-proc-web` / `framework-web` / `framework-web-axum` 缺 26.11.0 与 26.8.151，属缓存滞后。

### 3.2 P1 其他依赖

| Crate | 文档版本 | Registry 最新 | 本地验证 | 备注 |
|---|---|---|---|---|
| `tokio` | 1.52 | **1.54.1** | 未验证 | `framework-web` / `framework-web-axum` 要求 `^1.52`，1.54.1 满足 |
| `axum` | 0.8 | **0.8.9** | 未验证 | `framework-web-axum` 要求 `^0.8` |
| `clap` | 4.6 | **4.6.7** | 未验证 | — |
| `serde` | 未定版本 | 1.0.229 | 未验证 | 随 framework 传递 |
| `serde_json` | 未定版本 | 1.0.154 | 未验证 | 随 framework 传递 |
| `reqwest` | 未定版本 | **0.13.5** | 未验证 | 注意 0.13 与 0.12 的 API 差异，P1-003 需固定 |
| `ratatui` | 未定版本 | **0.30.2** | 未验证 | 0.30 为当前主线，crossterm backend 在 Windows 需实测 |
| `specta` | `=2.0.0-rc.25` | 2.0.0-rc.28 | 未验证 | **不可单独升级**：`framework-proc-core` 硬钉 rc.25 |
| `specta-typescript` | 0.0.12 | 0.0.15 | 未验证 | 受 `framework-proc-core` 的 `^0.0.12` 约束 |
| `inventory` | 未定版本 | 0.3.25 | 未验证 | 仅在 `collect` 下启用 |
| `rust-embed` | 未定版本 | 8.13.0 | 未验证 | D-009 推荐方案，状态 OPEN |
| `include_dir` | 未定版本 | 0.7.4 | 未验证 | D-009 备选 |

### 3.3 P2–P5 依赖（仅记录初步信息，不锁版本）

| Crate | 阶段 | Registry 最新 | 备注 |
|---|---|---|---|
| `sqlx` | P2 | 0.9.0 | 0.9 为新一代主版本，P2 正式开工前再确认与 SQLite 驱动兼容性 |
| `rig` | P2 | 0.46.0 | 版本演进快（0.42→0.46 多次 minor 破坏性变更），P2 再锁 |
| `rmcp` | P4 | 3.9.0 | P4 再锁 |

### 3.4 Git Dependency 与本地 Framework

- 本地 framework 源码可直接作为 `path` 依赖（已验证目录结构、Workspace 成员与 `Cargo.toml` 可读）。
- Git dependency 可行（`git-fetch-with-cli = true`，SSH 已配置），但 P1 期内需频繁改 framework，`path` 更合适。
- 私有 registry：**不存在**，全部依赖来自 crates.io（经 rsproxy 镜像）。

---

## 4. Framework 调研（源码级）

### 4.1 位置与版本

| 项 | 实测值 |
|---|---|
| 路径 | `D:/code/lingting/lingting-rust-framework` |
| Git | `main` 分支，工作区干净，HEAD `6a5af26` `:rocket: 26.11.0` |
| workspace 版本 | `26.11.0`，`edition 2024`，`rust-version 1.98.0` |
| 成员 | `framework-bus`、`framework-core`、`framework-datetime`、`framework-region`、`framework-startup`、`framework-system`、`framework-proc-auto`、`framework-proc-core`、`framework-proc-ts`、`framework-proc-web`、`framework-timer`、`framework-proc-timer`、`framework-web`、`framework-web-axum` |

- `framework-proc-rust` **不存在**（与 D-003 的规划一致，P1-006 新建）。
- registry 上 26.11.0 与本地源码的 **版本号、feature 定义、依赖区间一致**；但**未做 crate 内容逐字节比对**（缓存缺 26.11.0 的 web 系列，未下载比对）。

### 4.2 已核实的能力与限制

| 结论 | 依据 |
|---|---|
| `web_api` / `web_api_get` / `_post` / `_put` / `_patch` / `_delete` 存在，展开为 `WebRoute` 构造函数 | `framework-proc-web/src/lib.rs` |
| `path` 只接受字符串字面量，两端 `/` 被裁掉，**不支持 `{id}` 占位符**，不做前缀拼接 | `framework-proc-web/src/options.rs::parse_path` |
| 路由查找为 `method + path` **精确字符串匹配**（path 先 `trim_matches('/')`） | `framework-web-axum/src/router.rs::find` |
| 整个 axum app 只用 `Router::new().fallback(any(dispatch))`，**全部请求经单一 dispatch** | `framework-web-axum/src/lib.rs::build_router_with` |
| `AxumServer` 公开 API 仅 `context()` / `router()` / `run(wrapper)`；`axum_builder` / `with_cors` / `bind`；**无任何 shutdown 入口** | `framework-web-axum/src/server.rs`、`lib.rs`（全文检索 `shutdown`/`graceful` 命中 0） |
| `build_router_with` 是 **`pub(crate)`**；公开的 `build_router(wrapper)` 返回框架自有 `WebRouter`，**不是 axum `Router`** | `framework-web-axum/src/lib.rs:22,27` |
| `WebResponse::stream(status, content_type, stream)` 与 `WebBody::Stream(BoxStream<'static, Result<Bytes, io::Error>>)` 存在 | `framework-web/src/response.rs` |
| `AuthRule::anonymous()` / `login()` 等存在 | `framework-web/src/auth.rs` |
| `ApiMetadata { name, namespace, method, path, parameters, return_type }`；`return_type` 仅 `Void` / `Blob` / `Type(&'static str)` | `framework-proc-core/src/api_metadata.rs` |
| `ApiMetadata` **无 `auth` 字段**；参数类型是 `&'static str`（字符串），无泛型/模块路径信息 | 同上 |
| `api_metadata_iter()` 仅在 `framework-proc-core` 的 `collect` feature 下存在 | 同上 |
| TS 侧构建器齐备：`TypescriptTypeBuilder` / `TypescriptEnumBuilder` / `TypescriptApiBuilder` / `PackageBuilder` | `framework-proc-core/src/typescript/` |
| `framework-proc-core` **无任何 Rust 代码生成能力** | 目录仅含 `api_metadata.rs`、`type_metadata.rs`、`typescript/` |

### 4.3 Feature 传递链（实测，供 P1-003 用）

```text
framework-web-axum/collect → framework-web/collect
framework-web/collect      → dep:inventory + framework-proc-web/collect
framework-proc-web/collect → （空，仅控制是否生成 push_web_api!）
framework-proc-ts/collect  → framework-proc-core/collect
framework-proc-core/collect→ dep:inventory（提供 api_metadata_iter）
```

**重要细节**：元数据注册发生在 `#[framework_proc_ts::ts_api]` 展开体内（`framework-proc-ts` 的 `collect` 才 `collect_api` 提交 `inventory`）。因此：

- **只开 `framework-web/collect` 不足以让生成器看到接口**；
- 服务端 / API crate 必须显式开启 **`framework-proc-ts/collect`**（或 `framework-proc-core/collect`）才能让 `api_metadata_iter()` 有数据。

这一条比 `.steps/p1/tasks.md` P1-003 第 3 条中「`framework-web/collect → framework-proc-web/collect`」的描述更精确，需在 P1-003 采纳。

---

## 5. P1 风险

| ID | 风险 | 影响 | 建议方案 | 阻塞 P1-002 |
|---|---|---|---|---|
| R-1 | `framework-*` 获取方式未定（D-015 OPEN） | P1-003 起所有依赖声明 | registry 26.11.0 已全量发布，可二选一；因 P1 需改 `framework-web-axum` 与新增 `framework-proc-rust`，倾向先 `path` 后切 registry | 否（P1-002 骨架不引 framework 依赖） |
| R-2 | 优雅关闭无公开入口，且 `build_router_with` 为 `pub(crate)`（D-013 OPEN） | P1-013 无法在不动上游的前提下实现；`build_router` 拿不到 axum `Router` | 向 `lingting-rust-framework` 增加 `run_with_shutdown`（或暴露 `build_router_with`）；须在 P1-008 前定 | 否 |
| R-3 | 全量请求经单一 `fallback(any(dispatch))`，静态资源/SPA fallback 必须作为框架路由存在 | P1-012 的 SPA fallback 与 API 404 语义要在同一 dispatch 内区分；axum 原生 `ServeDir` 无法直接挂 | P1-012 前确认 `WebRouter` 是否支持非 API 路径回落；必要时向上游补"未命中钩子" | 否 |
| R-4 | `ApiMetadata` 无 `auth`、参数类型只有字符串、返回值无 `Stream` | Rust SDK 生成（D-003/A-1/A-2）需自定义类型映射约定；SSE 无法类型化（D-007） | P1-006 先产出类型映射规则（`&'static str` → Rust 类型名的解析约定）；D-007 维持方案 1 | 否 |
| R-5 | `specta = "=2.0.0-rc.25"`、`specta-typescript ^0.0.12` 被 `framework-proc-core` 硬约束 | 不能单独升级；跨 crate 版本分裂会导致 `ApiMetadata` 类型不一致、元数据注册表分裂 | P1-003 锁定 workspace 级版本并禁止覆盖；升级必须同步改 framework | 否 |
| R-6 | `framework-proc-rust` 不存在，工作量与不确定性最高 | P1-006 是 P1 最大技术风险点 | P1-004 通过后即启动 P1-006 预研；先做"最小 trait + 方法"再补类型还原 | 否 |
| R-7 | WebUI 技术栈未定（D-010 OPEN），本机无全局 `tsc` | P1-016/P1-005 需要项目内安装前端依赖 | P1-005 建最小 `package.json` + devDependency `typescript`（工程内，非全局升级） | 否 |
| R-8 | Windows 下 TUI（crossterm / alternate screen）与 SSE 不缓冲（A-4）均未验证 | P1-015 / P1-011 的体验与验收 | 在 P1-011 / P1-015 中实测；必要时改用 `curl -N` + 原生事件流对照 | 否 |
| R-9 | 交叉编译可用性被高估：已装 `x86_64-apple-darwin` std，但 Windows 上无 macOS SDK；`cargo-zigbuild` 不能替代 | P1-018 的"跨平台"结论 | P1-018 采用"Windows 本机 + CI 原生 runner 矩阵"，不承诺 Windows→macOS 交叉编译（D-020） | 否 |
| R-10 | `sccache` 作为全局 `rustc-wrapper`，且 `CARGO_HOME` 在非系统盘 | 极端情况下缓存与 path 依赖的 framework 交互出现陈旧产物 | 若出现"改了 framework 但产线未生效"，用 `SCCACHE_DISABLE=1` 或 `cargo clean` 对照 | 否 |
| R-11 | 多 toolchain 并存（stable / nightly / 1.84.0） | 未固定时不同 shell 可能用到不同版本 | P1-002 用 `rust-toolchain.toml` 固定 1.98.0 + `rustfmt` + `clippy` 组件 | 否（P1-002 即解决） |

---

## 6. P1-002 前置条件

### 已解决

- 仓库现状清楚：纯文档仓库，无既有 Cargo 工程需要兼容或迁移。
- 工具链版本、edition 支持、`rustfmt` / `clippy` 齐备，且与 `rust-version = 1.98.0` 精确匹配。
- 目标平台与 std target 清单已确认（Windows 为一等公民）。
- `framework-*` 版本可用性已确认：**registry 26.11.0 全量发布**，本地源码亦为 26.11.0，两条获取路径都成立。
- framework 源码可读，`framework-proc-web` 宏能力、`WebRouter` 匹配语义、`AxumServer` 关闭缺口均已核实。
- Node / npm / pnpm 齐备，WebUI 构建链可用（缺 `typescript`，工程内安装即可）。

### 仍需人工确认（不阻塞 P1-002，阻塞后续任务）

| 项 | 决策 | 阻塞对象 |
|---|---|---|
| `framework-*` 获取方式（path / registry） | D-015 | P1-003 |
| WebUI 技术栈 | D-010 | P1-016（P1-005 需先定 TypeScript 工程骨架） |
| 优雅关闭方案（改上游 / 绕开） | D-013 | P1-008、P1-013 |
| API 版本前缀 `v1` 是否 P1 引入 | D-019 | P1-007 |
| 跨平台交付范围 | D-020 | P1-018 |
| WebUI 静态资源嵌入方案 | D-009 | P1-017 |
| SSE 类型化是否 P1 升级 | D-007 | P1-011 |

### P1-002 可直接开工的部分

`rust-toolchain.toml`、`.gitignore`、`rustfmt.toml` + workspace lints、最小可 `cargo build` 的 `Cargo.toml`、开发脚本入口 —— 这些**不需要** `framework-*` 依赖，也不受上述 OPEN 项影响。

---

## 7. P1-001 验收对照

| 验收标准 | 结论 | 依据 |
|---|---|---|
| 版本矩阵包含全部 `framework-*` crate 的确定版本与获取方式（registry 版本号 或 path 依赖路径） | **部分通过** | 版本已确定（26.11.0 全链路，registry 与本地一致）；**获取方式**属 D-015，仍为 OPEN |
| 明确写出 rustc/cargo/node/pnpm 的实测版本，并指出与 `rust-version = 1.98.0` 的匹配结论 | 通过 | 第 2 节 |
| 无任何源码或 Cargo 文件被提前创建 | 通过 | 第 1.3 节，本次仅新增文档 |
