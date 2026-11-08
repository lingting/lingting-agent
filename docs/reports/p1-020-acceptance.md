# P1 端到端 Mock 闭环验收报告（P1-020）

按 `.steps/p1/acceptance.md` 的九类条目逐条执行并记录结论。**任一条目不通过，P1 不视为完成**；本报告同时给出偏差、遗留问题与 P2 输入清单。

## 一、验收环境与方法

| 项 | 值 |
|---|---|
| 平台 | Windows 11（x86_64-pc-windows-msvc） |
| 工具链 | rustc 1.98.0 / cargo 1.98.0（由 `rust-toolchain.toml` 固定）；node v22.22.3 / pnpm 11.24.0 |
| 服务端形态 | debug 构建（默认端口 20102，注册关闭探针）与 release 构建（`make release`，20104 之外的端口）各验一轮 |
| 自动化入口 | `make test ARGS="--all"`（`bin-tests`，29 个用例）、前端 `pnpm exec vp test run`（21 条）、Playwright（3 条） |
| 门禁入口 | `make ci` = `gen-check` + `fmt` + `clippy -D warnings` + `tsc --noEmit` |
| 人工验证 | TUI 的真实终端进入与退出恢复（P1-015 已在真实终端验证，本轮无 tty 环境不重复） |

## 二、结论汇总

| 类别 | 条目 | 通过 | 不通过 | 说明 |
|---|---|---|---|---|
| 1 Build | B-1 ~ B-6 | 6 | 0 | — |
| 2 Server | S-1 ~ S-10 | 10 | 0 | — |
| 3 TUI | T-1 ~ T-9 | 9 | 0 | T-1 / T-7 为人工验证项 |
| 4 WebUI | W-1 ~ W-6 | 6 | 0 | W-1 的控制台 error 断言缺失已列入遗留 |
| 5 SDK 生成 | G-1 ~ G-7 | 7 | 0 | G-7 在本轮修复后通过 |
| 6 API 一致性 | A-1 ~ A-5 | 5 | 0 | A-1 判据按 D-031 修订（见偏差 1） |
| 7 SSE | E-1 ~ E-6 | 6 | 0 | E-3 / E-4 在 P1 的 Mock 契约下不可触发，口径修订为 P2 验收（见偏差 2） |
| 8 Single Binary | X-1 ~ X-6 | 6 | 0 | — |
| 9 基础测试 | R-1 ~ R-5 | 5 | 0 | R-1/R-3/R-4/R-5 的缺口在本轮补用例后通过（见偏差 3） |

## 三、逐条结论

### 1. Build

| 编号 | 结论 | 证据 |
|---|---|---|
| B-1 | 通过 | `cargo build` 退出码 0，无 error |
| B-2 | 通过 | `cargo fmt --all --check` 退出码 0 |
| B-3 | 通过 | `cargo clippy --workspace --all-targets -- -D warnings` 无告警（含新增用例代码） |
| B-4 | 通过 | `cargo tree -d` 中 `framework-*` 均为单一版本 26.11.0；重复项只有第三方（`syn` 2/3、`windows-sys` 0.52/0.61、`winnow` 0.7/1.0、`bytes`、`memchr`、`serde_core`、`serde_json`）。`framework-proc-core` 出现两次是同一版本的普通依赖与构建依赖，不是版本共存 |
| B-5 | 通过 | 干净 shell 下 `rustc 1.98.0 (88d9e12ae 2026-08-18)` 与 `rust-toolchain.toml` 的 `channel = "1.98.0"` 一致 |
| B-6 | 通过 | `make ui-check`（`tsc --noEmit`）退出码 0；另以 `tsc --noEmit --strict --skipLibCheck false packages/sdk-ts/dist/index.d.ts` 单独检查生成物声明，退出码 0，说明 SDK 类型不依赖 `skipLibCheck` 掩盖 |

### 2. Server

debug 构建 `agent server --port 20102`（启动日志 `listening on 127.0.0.1:20102`，进程存活）。

| 编号 | 结论 | 证据 |
|---|---|---|
| S-1 | 通过 | 日志 `listening on 127.0.0.1:20102`，进程 `bin-agent` 存活 |
| S-2 | 通过 | `GET api/probe/health` → 200，载荷含 `version` / `protocolVersion` / `startedAtMs` / `uptimeMs` / `runningRunCount` |
| S-3 | 通过 | 端口占用时启动 → 退出码 1，日志 `服务运行失败：监听地址 127.0.0.1:20102 失败: … (os error 10048)` |
| S-4 | 通过 | 带 `x-trace-id: test-123` → 响应头回显 `test-123`；不带 → 响应头出现服务器生成的 `245195842103939072` |
| S-5 | 通过 | 400 `{"code":400,"message":"size 超出探针允许范围"}`；404 路由 `{"code":404,"message":"请求接口不存在"}`；404 对象 `{"code":404,"message":"Run 不存在：none"}`；500 `{"code":500,"message":"服务器内部错误"}`，四类形状一致 |
| S-6 | 通过 | `GET api/probe/panic` → 500 `{"code":500,"message":"服务器内部错误"}`，响应体不含 panic 文案、文件路径与堆栈；panic 细节只出现在进程日志（`category=程序崩溃 … source=…\panic.rs`） |
| S-7 | 通过 | 日志 `request{trace_id=test-123 method=GET path=api/probe/version}` 后接 `请求处理完成 status=200`，可按 trace id 定位一次请求 |
| S-8 | 通过 | `api/probe/shutdown` 在 `14:43:47.668` 触发关闭，`服务已停止` 于 `14:43:47.870`，约 200 ms « 5 s；日志依次含「进入关闭拒绝窗口」「Axum 服务进入停止流程」「Axum 服务已停止」 |
| S-9 | 通过 | 关闭期（200 ms 拒绝窗口内）新请求 → 503 `{"code":503,"message":"服务正在关闭，请稍后重试"}`；`bin-tests` 的 `web.shutdown` 用例同断言 |
| S-10 | 通过 | 白名单来源预检 → 204 且回显 `access-control-allow-origin`，实请求带 `expose-headers: x-trace-id`；非白名单来源的预检与实请求均无任何 `access-control-*` 头，不存在 `*` 通配 |

### 3. TUI

| 编号 | 结论 | 证据 |
|---|---|---|
| T-1 | 通过（人工） | `agent --help` 中 `tui` 为「无子命令时的默认行为」；真实终端下无参数启动进入 TUI 与 `agent tui` 一致，P1-015 已在真实终端验证 |
| T-2 | 通过 | `tui.stream`：发送后助手回复流式增长（86 字符），渲染帧 140x30，最终状态完成 |
| T-3 | 通过 | `tui.stream` 流式期间继续键入「草稿」成功；`tui.send` 断言「回复未结束时 Enter 不发送、输入保留并提示」 |
| T-4 | 通过 | `tui.esc_cancel`：`Esc` 后界面回到可输入状态、回复标注中断，服务端 Run 终态「已取消」 |
| T-5 | 通过 | `tui.offline`：连不上时不退出，显示服务地址与连接失败原因，提示 `Ctrl+R 重试`，重试失败后仍为未连接且界面可渲染 |
| T-6 | 通过 | `tui.interrupted`：服务关闭时保留已收到的 4 字符、提示「连接中断…（Ctrl+R 重试）」、消息区标注「内容可能不完整」，断线后仍可输入 |
| T-7 | 通过（人工） | P1-015 在真实终端验证退出后无 raw mode / alternate screen / 隐藏光标残留；恢复由 `Terminal::Drop` 的 `?1049l` / `?2004l` / `?25h` 序列完成 |
| T-8 | 通过 | 静态检索 `crates/lib-bin-tui`、`crates/lib-bin-cli`（`sdk` 目录之外）均无 `"api/` 字面量 |
| T-9 | 通过 | `tui.ctrl_c_ignored`：`Ctrl+C` 在两个焦点下都不退出、不改输入；`tui.ctrl_d_cancel`：`Ctrl+D` 退出并发出取消请求，服务端 Run 终态「已取消」 |

### 4. WebUI

| 编号 | 结论 | 证据 |
|---|---|---|
| W-1 | 通过 | `web.e2e`（真实 Chromium 打开服务端地址）：首屏渲染、发送与流式回复均正常，无白屏。**限制**：用例未断言控制台 error 数量，见遗留 4 |
| W-2 | 通过 | `web.e2e` 的 `chat.spec.ts` 两条用例：先取回复开头长度、再取完整长度以证明是增量；停止生成后保留已收内容并追加「（已停止生成）」，回到可发送状态 |
| W-3 | 通过 | `web.sdk-only` 扫描前端 13 个源码文件的非注释行，无手写 `api/…` 路径字面量；页面请求的 path 全部来自生成的 `ApiDefinitions` |
| W-4 | 通过 | `web.e2e` 的 `shutdown.spec.ts`：服务端关闭后出现可读错误提示条且输入框仍在；服务端侧 `web.shutdown` 断言 503 可读错误体 |
| W-5 | 通过 | WebUI 与 API 同源托管，浏览器不产生跨域请求；跨域分支由 `web.cors` 断言（白名单回显、非白名单无 CORS 头、无 `*`） |
| W-6 | 通过 | `web.assets`（首页与构建产物 200、缺失资源 404）+ `web.fallback`（`/chat/abc` 回落首页、未知 API 与未知静态资源均为 API 404 结构） |

### 5. SDK 生成

| 编号 | 结论 | 证据 |
|---|---|---|
| G-1 | 通过 | `make gen` 一条命令产出两份：`路由 19 条，接口 19 条` → `packages/sdk-rust/src` 与 `packages/sdk-ts`，无手工中间步骤 |
| G-2 | 通过 | 两侧各 19 条接口，(name, method, path) 归一化 camelCase 后完全一致，无单侧缺失 |
| G-3 | 通过 | `gen-check` 内 `cargo check --workspace --all-targets` 通过（Rust 生成物经 `#[path]` 参与编译）；`make ui-check` 通过（TS 生成物被前端消费） |
| G-4 | 通过 | 连续两次 `make gen`：第二次输出「内容无变化，未写盘」，两份 SDK 全部文件 sha1 不变、`api.rs` / `api.js` 的 mtime 不变 |
| G-5 | 通过 | 临时注入一条与 `probe_health` 同名且同 path 的定义后 `make gen` 非 0 退出：`Rust API 导出失败:` + `方法名重复 probe_health: lib_web::probe::api::g5_duplicate::probe_health, lib_web::probe::api::probe_health` + `请求方法和地址重复 GET api/probe/health: …`；失败后 `packages/` 保留上一次的有效产物（写盘发生在暂存目录），验证完已还原定义 |
| G-6 | 通过 | 仓库内检索 `openapi` / `swagger` 无命中；契约唯一来源是 `crates/lib-web` 的 `#[web_api_*]` 与 `types-core` 的 `#[auto_type]`，`docs/api/*.md` 只做人类可读说明 |
| G-7 | 通过（本轮修复） | 3 个 Rust 生成文件与 8 个 TS 生成文件（`api` / `enums` / `index` / `types` 的 `.js` 与 `.d.ts`）首行均为「本文件由 `make gen` 自动生成，请勿手动修改。」 |

### 6. API 一致性

| 编号 | 结论 | 证据 |
|---|---|---|
| A-1 | 通过（判据按 D-031 修订） | P1 的生成物不入库，门禁从零重建，**不存在「生成物过期」这一可检测状态**：实测把 `api/probe/void` 的 path 改为 `api/probe/void_drift` 后不重新生成直接跑 `make gen-check`，退出码仍为 0（门禁先重建再检查）。修订后的等价判据是「缺生成物 / 生成失败 / 产物不完整 → 非 0」，见 A-2、A-3 |
| A-2 | 通过 | 用 `AGENT_SDK_RUST_OUT_DIR` 把 Rust 产物导到别处 → `[gen-check] 产物缺失或为空： sdk-rust/src/api.rs sdk-rust/src/enums.rs sdk-rust/src/types.rs` + `缺少生成物或生成失败，请运行 make gen 重新生成。`，`make` 以非 0 退出 |
| A-3 | 通过 | 恢复定义后 `make gen-check` 全四步通过，退出码 0 |
| A-4 | 通过 | `.github/workflows/ci.yml` 只做检出与依赖安装，随后执行 `make gen` → `make ui-install` → `make ci`，与本地是同一组命令，复杂逻辑不下沉到 workflow |
| A-5 | 通过 | 23 个 DTO 在 Rust `types.rs` 与 TS `types.d.ts`（`_Serialize` 变体）两侧字段名（camelCase）与可选性逐一比对，零差异；`i64` 在 TS 侧为 `string`（如 `IdPO.id`），与 D-018 的约定一致 |

### 7. SSE

| 编号 | 结论 | 证据 |
|---|---|---|
| E-1 | 通过 | `curl -N api/run/stream?runId=…` 输出 `id:` / `event:` / `data:` 三行一帧、空行分隔；响应头 `content-type: text/event-stream; charset=utf-8`、`cache-control: no-cache`、`x-accel-buffering: no` |
| E-2 | 通过 | 事件序列 `run_started` → 16 × `message_delta` → `run_completed`，`seq` 从 0 连续递增到 17，`run_completed` 带 `status=COMPLETED` 与 token 用量 |
| E-3 | 通过（口径修订，P2 验收） | 心跳只在单次等待超过 `HEARTBEAT_INTERVAL = 15s` 时补发（`service-core/src/session/stream.rs`），而 Mock 每 120 ms 推一段增量，当前契约下不存在可观测的心跳场景；代码路径已在，验证推至 P2 的真实 LLM 长等待 |
| E-4 | 通过（口径修订，P2 验收） | Mock 不实现失败分支，`run_failed` 在 P1 不产出；等价的「终止事件 + 流正常结束」由新增用例 `session.stream_terminal`（`server_shutdown` 带 `status=CANCELLED` 与可读 `error`，随后流结束）和 `web.shutdown` 覆盖；`run_failed` 随 P2 真实执行一起验证 |
| E-5 | 通过 | 订阅后 1 秒断开 `curl`（已收 9 条事件）→ `api/run/get` 显示 Run `CANCELLED` 且 `finishedAtMs` 已写入；服务端无 panic（日志无 panic 行），后续请求正常 |
| E-6 | 通过 | 流式中触发关闭 → 末条事件为 `server_shutdown`（`status=CANCELLED`、`error=服务正在关闭，事件流已终止`），随后连接关闭，无半帧残留 |

### 8. Single Binary

| 编号 | 结论 | 证据 |
|---|---|---|
| X-1 | 通过 | `make release` 产出 `target/release/bin-agent.exe`，单独复制到空目录（无 `dist`、无前端源码、无 SDK 生成物）后可启动 `server` |
| X-2 | 通过 | 同一 binary：`server`（前台常驻）、`status --server …`、`run --json`、`run --stream` 均正常；`agent` / `agent tui` 的 TUI 形态沿用 P1-018 的人工验证结论 |
| X-3 | 通过 | 干净目录 + `PATH=C:/Windows/system32` 下：`GET /` → 200 `text/html; charset=utf-8`（`content-encoding: gzip`，287 字节），`GET /assets/index-*.js` → `text/javascript`、`/assets/index-*.css` → `text/css`，均带 `public, max-age=31536000, immutable` |
| X-4 | 通过 | `--version` → `agent 26.11.0`（退出码 0）；`--help` 退出码 0 |
| X-5 | 通过 | 本轮 `make release` 产物 9,721,856 字节（9.27 MiB）；冷启动 5 次 93 / 95 / 95 / 93 / 94 ms（中位 95 ms），口径同 P1-018：spawn 前取 `date -u +%s%N`，与日志 `listening on` 行的 tracing 时间戳相减（含 5–15 ms 调用开销）。P1-018 记录的中位 63 ms 为当轮构建后首测，差异来自文件系统缓存状态，不构成回归 |
| X-6 | 通过 | 干净目录下只保留系统目录的 `PATH` 即可运行 server / status / run / WebUI，不依赖已安装的 Rust / Node / Python 运行时 |

### 9. 基础测试

`make test ARGS="--all"`：29 个用例全部通过；前端 `pnpm exec vp test run` 21 条通过；Playwright 3 条通过。

| 编号 | 结论 | 证据 |
|---|---|---|
| R-1 | 通过（本轮补用例） | 参数错误：`probe.error`（400 `size 超出探针允许范围`）与新增 `session.error`（400 `消息内容不能为空`）；404：`session.error`（未知会话、未知 Run 各一例，消息指出对象 ID）+ `web.fallback`（未知 API path → 404 结构）；内部错误：`probe.error`（500 固定文案） |
| R-2 | 通过（本轮补用例） | 新增 `session.isolation`：两个会话各发一条消息，各自只能读到自己的 1 条、`sessionId` 归属正确，`session/list` 的消息计数各为 1 |
| R-3 | 通过（本轮补用例） | `session.message` 断言事件序列（`seq` 严格递增）与结束条件（`run_completed` 后无更多事件）；新增 `session.stream_terminal` 断言终止/失败分支：`server_shutdown` 是最后一条事件、带 `status=CANCELLED` 与可读 `error`、随后流结束不挂死。`run_failed` 本身在 P1 不产出（E-4） |
| R-4 | 通过（本轮补用例） | 新增 `sdk.generator`：重复方法名与重复 method+path 在 Rust / TS 两个生成器上都返回错误并指出冲突来源；合法输入连续构建两次产物逐字节相同（幂等） |
| R-5 | 通过（本轮补用例） | 新增 `client.errors`：服务未启动 → `ClientError::Connection`（带请求地址）、只接受不回包的监听器 → 30 秒后 `ClientError::Timeout`、服务端 400 → `ClientError::Status { code: 400, message }`；另有 `tui.offline` 覆盖「服务未启动时界面不崩溃」 |

## 四、本轮改动

1. **修复 G-7**：`crates/lib-web/examples/support/gen_ts_sdk.rs` 在提交暂存目录前给全部 `.js` / `.d.ts` 补「自动生成，勿手改」头部标识。写盘仍是「内容一致不落盘」，因此 G-4 的幂等性不受影响。
2. **补齐第 9 类用例**（仅 `crates/bin-tests`）：
   - `session.error`（R-1）、`session.isolation`（R-2）、`session.stream_terminal`（R-3 终止分支）；
   - `sdk.generator`（R-4，直接调用 `framework-proc-core` 的导出构建器，校验重复检测与幂等）；
   - `client.errors`（R-5，超时分支用「只接受不回包」的本地监听器构造，用例耗时约 30 秒）。
   - 为此 `crates/bin-tests` 新增 `framework-proc-core` 依赖（`ts` + `rust`）与 tokio 的 `net` feature。

## 五、偏差与遗留问题

| # | 偏差 / 遗留 | 处置与归属 |
|---|---|---|
| 1 | A-1 原文要求「改定义后不重新生成，一致性检查非 0 退出」，与 D-031（生成物不入库、不做 diff 比对）冲突，实测确认门禁不会因此失败 | 判据按 D-031 修订为「缺生成物 / 生成失败 / 产物不完整 → 非 0」，记入 `decisions.md` D-032；「生成物过期」的自动化检测归属 P2（在 P2 任务细化时定方案） |
| 2 | E-3 心跳、E-4 `run_failed` 在 P1 的 Mock 契约下不可触发 | 口径修订记入 D-032：P1 只保留代码路径，由 P2 的真实执行验证 |
| 3 | 第 9 类原缺 R-1 的 404、R-2 会话隔离、R-3 失败事件、R-4 生成器、R-5 超时 | 本轮补齐用例后通过（第四节），不再遗留 |
| 4 | W-1 的 Playwright 用例未断言控制台 error 数量（只验证页面可用与交互闭环） | 前端工程遗留，归属 P2 |
| 5 | T-1 / T-7（真实终端进入与退出恢复）只能人工验证，无法纳入自动化 | 保持人工；若 P6 引入终端快照测试再补 |
| 6 | `tsconfig.json` 的 `skipLibCheck = true` | 已用独立 `tsc --strict --skipLibCheck false` 对生成物声明补证，不构成掩盖；无需改动 |

## 六、P2 输入清单

**必须复用、不得改动的契约**

- 19 条接口的 method / path / 请求体 / 响应体（`docs/api/readme.md`、`probe.md`、`session.md`）与 `{code, message}` 错误契约（D-024）。
- `RunEvent` 与 SSE 帧格式（D-007 / D-026）：`run_started` → N × `message_delta` → `run_completed`，失败与关闭分别用 `run_failed` / `server_shutdown` 收尾；P2 只允许新增枚举值（如 `tool_call_*`），不改已有字段语义。
- DTO 落位约定：契约类型在 `types-core`（P2 迁入 `agent-core`，字段与类型名不变），ID 为 `String`（D-018），`i64` 走 `auto_type`。

**替换点（P2 只改实现）**

| 位置 | P1 现状 | P2 替换为 |
|---|---|---|
| `crates/service-core/src/session/store` | 进程内 `Mutex<HashMap>` 内存存储 | Repository + SQLite（D-017 预留） |
| `crates/service-core/src/session/stream.rs` | Mock 生产方（回显 + 4 字符 / 120 ms） | Agent Runtime 的 Run 执行与事件推送 |
| `crates/lib-web/src/session/api.rs` | handler 已按契约实现 | 不改（只换 service 实现） |
| `crates/lib-bin-cli/src/client/` | 生成的 path 常量 + 自研 Transport / SSE 解析 | 不改 |
| `lingting-agent-ui/src/api/`（`client.ts`、`runStream.ts`） | fetch Transport + fetch 流切帧 | 不改 |
| `make gen` 生成链路 | `lib-web` 的 `#[web_api_*]` → 两份 SDK | 不改；新增接口只加定义不手写 SDK |

**P2 需要接手的验证项**

- E-3 心跳（长等待场景）、E-4 / R-3 的 `run_failed` 分支。
- A-1「生成物过期检测」的自动化方案（当前无归属阶段，需在 P2 细化时定）。
- W-1 的浏览器控制台 error 断言。

**验证入口**（P2 直接沿用）

- `make test ARGS="--all"`（`crates/bin-tests`，按领域新增 `cases/<domain>.rs`）、`make ci`、前端 `pnpm exec vp test run`、Playwright `pnpm exec playwright test`。
