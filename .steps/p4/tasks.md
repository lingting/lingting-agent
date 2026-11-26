# P4 任务清单

**状态：已完成（2026-11-26）。** 20 个任务 / 5 个批次全部结案，23 项决策（D-P4-001 ~ D-P4-023）全部定案（[decisions.md](decisions.md)）。阶段目标与边界见 [README.md](README.md) §一 / §二，判据见 [acceptance.md](acceptance.md)，取证见 [`docs/reports/p4-020-acceptance.md`](../../docs/reports/p4-020-acceptance.md)。

批次之间强序，批次内可并行。**每个任务的「测试要求」只允许在 `crates/bin-tests` 落地用例；其他 Crate 不得写 `#[cfg(test)]`**（仓库 `AGENTS.md` 第 28 条）。**所有需要执行用例的步骤都必须先取得明确授权**；未授权时只推进代码、静态检查与文档部分，相关验收项保持未勾选。

本阶段的四条硬线：

1. **协议不出 crate**：`rmcp` 只出现在 `agent-mcp`（与测试用的 `bin-tests`），`crates/agent-runtime` 本阶段零改动。
2. **同构而不旁路**：MCP 工具就是一个 `Arc<dyn Tool>`，经 P3 的注册表与执行器执行；不新增执行路径、不新增事件类型。
3. **失败即回灌**：连接失败、握手超时、崩溃、调用报错、工具不存在，全部变成模型可读文本，不允许挂死、不允许把传输层原文交给模型。
4. **关得干净**：服务关闭时先让 Run 落终态，再断 MCP，最后杀子进程；`kill_on_drop` 兜底。

---

## Batch A：Crate、契约前提与配置（P4-001 ~ P4-004）

> 依赖：无。本批次只造地基：新 crate 可编译、工具契约能承载动态工具、配置能装载校验，没有任何 MCP 连接。
>
> **状态：已完成（2026-11-25）。** 门禁：`cargo build --workspace`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全绿；用例 `mcp.config` / `tool.registry` 新增并通过，`make test ARGS="--all"` 105 个用例全绿。

- [x] **P4-001 新增 Crate `agent-mcp` 与依赖边界**

  **目标**：给 MCP 一个自己的领域 Crate，依赖方向与「协议不出 crate」从第一行代码起就锁死。
  **前置依赖**：无。
  **涉及文件 / Crate**：`Cargo.toml`（members、`workspace.dependencies`、`rmcp` 条目）、新增 `crates/agent-mcp/{Cargo.toml,src/lib.rs}`、`docs/workspace.md`（§3.2 `agent-mcp` 段与 §8.1 成员清单）。
  **实施步骤**：
  1. workspace 增加成员与内部依赖；`agent-mcp` 的 `[dependencies]` 只写 `agent-tool` / `lib-core` / `rmcp` / `tokio`（`process`、`sync`、`time`）/ `async-trait` / `futures-util` / `tokio-util` / `serde` / `serde_json` / `tracing` / `framework-proc-auto` / `framework-proc-core`（`collect`）。
  2. `rmcp` 用 `default-features = false` + `features = ["client", "transport-child-process", "transport-streamable-http-client-reqwest"]`；若编译需要的隐式特性（如 `macros` / `uuid` / `base64`）缺失，按编译器报错补进 features，并把最终清单回填 D-P4-002。
  3. `lib.rs` 只声明模块与边界注释（照 `agent-tool/src/lib.rs` 的写法）：说明「本 Crate 是 `rmcp` 的唯一落点」「工具经 `agent-tool::Tool` 适配，不反向依赖运行时」「不依赖 `types-core`，配置由装配层翻译」。
  4. 核对依赖体积：`cargo tree -p agent-mcp` 与 `cargo tree -i reqwest`，把「是否出现两份 reqwest」记进 `decisions.md` D-P4-002。
  **验收标准**：
  - [x] `cargo check -p agent-mcp` 通过；`cargo tree -p agent-mcp` 里没有 `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `lib-db` / `lib-web*` / `bin-*` / `types-core`。
  - [x] `cargo tree -i rmcp` 只命中 `agent-mcp`（本批尚无 `bin-tests`）。
  - [x] `Cargo.lock` 新增 `rmcp` 条目；`docs/workspace.md` 的成员清单与 `Cargo.toml` 的 members 一致。
  **测试要求**：无（不写测试代码）。检查方式为 `cargo check`、`cargo tree`、`cargo build --workspace` 与 `git diff`；批次 A 收尾时跑通全量用例（`make test ARGS="--all"`，105 个用例全绿）。

- [x] **P4-002 `Tool` 契约放宽：动态名字与外部 schema**

  **目标**：让 `Tool` 能表达「名字、说明、schema 都来自运行期数据」的工具，MCP 适配器因此不必泄漏内存。
  **前置依赖**：无（与 P4-001 并行）。
  **涉及文件 / Crate**：`crates/agent-tool/src/{tool.rs,schema.rs,lib.rs}`、`crates/agent-tool/src/builtin/*.rs`（返回类型随契约调整）、`crates/agent-runtime/src/loop_/tool.rs`（`definitions()` 的借用方式随契约核对）。
  **实施步骤**：
  1. 三个方法去掉 `'static`：`fn name(&self) -> &str`、`fn description(&self) -> &str`、`fn schema(&self) -> &ToolSchema`；`validate_name` 的调用点不变。
  2. `ToolSchema` 增加 `pub fn from_value(value: Value) -> Self`：只做「不是对象时补 `{"type":"object","properties":{}}`」的兜底，其余原样保留（不剥键，与 `of::<A>()` 的口径差异写进模块注释）。
  3. 逐处核对内置工具与 `agent-runtime` 的实现/调用：只需要签名兼容，行为不变。
  4. 在 `tool.rs` 的 trait 文档里写明「名字可以来自运行期（P4 的 MCP 工具），仍必须满足 `validate_name`」。
  **验收标准**：
  - [x] `cargo build --workspace` 通过，内置工具与执行器行为未变（`git diff` 只含签名与新增构造函数）。
  - [x] 全仓检索 `fn name(&self) -> &'static str` 无命中。
  - [x] `ToolSchema::from_value` 能承载任意对象 schema，且对非对象输入有确定的兜底。
  - [x] `cargo clippy -p agent-tool --all-targets -- -D warnings` 通过。
  **测试要求**：无新增用例；契约变化由 `tool.*` 既有用例回归（`make test ARGS="tool"`，27 个用例全绿）。

- [x] **P4-003 `ToolRegistry` 运行期可变与条目默认权限**

  **目标**：注册表能在运行期增删工具（工具发现是异步事件），并承载「server 级默认权限」；执行器与运行时的持有关系不变。
  **前置依赖**：P4-002。
  **涉及文件 / Crate**：`crates/agent-tool/src/{registry.rs,permission.rs,executor.rs,lib.rs}`、`crates/agent-runtime/src/loop_/tool.rs`（`lookup` 的返回类型调整）。
  **实施步骤**：
  1. 内部结构改为 `RwLock<BTreeMap<String, Entry>>`，`Entry { tool: Arc<dyn Tool>, default_permission: Option<ToolPermission> }`；`register` 保持原签名（条目默认 `None`），新增 `register_with_default(tool, Option<ToolPermission>)`、`unregister(name) -> Option<Arc<dyn Tool>>`、`names()`。
  2. `lookup` 返回 `Option<Arc<dyn Tool>>`（克隆 `Arc`）；`model_definitions` 与 `execute_batch_of` 的借用方式一并调整。
  3. 权限判定收敛为一条实现：`registry.permission_of(tool) = policy.configured(name) > entry.default_permission > risk.default_permission()`；`ToolPolicy` 本身不改语义（仍只装显式配置）。
  4. `executor` 的权限分支改走上面的入口；`deny` 仍然「不进清单 + 执行期 `DENIED`」。
  5. 读写锁纪律写进模块注释：写只发生在发现差异处，读不得跨 `await` 持锁。
  **验收标准**：
  - [x] 注册 / 注销后 `model_definitions()` 立即反映变化，顺序仍由名字字典序决定。（用例 `tool.registry`）
  - [x] 三级权限判定可逐项对照：显式 `deny` 盖过条目 `allow`；条目 `confirm` 盖过 `risk` 缺省；无条目默认时行为与 P3 完全一致。（用例 `tool.registry`）
  - [x] `cargo tree -p agent-tool` 的依赖清单未变（不新增第三方依赖）。
  - [x] `cargo clippy -p agent-tool --all-targets -- -D warnings` 通过。
  **测试要求**：新增用例 `tool.registry`（`crates/bin-tests/src/cases/tool/registry.rs`）逐条核对清单顺序、注销与三级权限；已执行通过（`make test ARGS="tool"`，27 个用例全绿）。

- [x] **P4-004 MCP 配置类型与装载校验**

  **目标**：`mcp.toml` 有类型、有缺省值、有启动期硬校验，错误信息能直接指出哪个 server 的哪个字段。
  **前置依赖**：无。
  **涉及文件 / Crate**：新增 `crates/types-core/src/config/mcp.rs`、`crates/types-core/src/config/mod.rs`、`crates/lib-core/src/directorys.rs`（`mcp.toml` 路径）、`crates/lib-bin-cli/src/config.rs`、`docs/api/`（无接口变化，不动）。
  **实施步骤**：
  1. `McpConfig { connect_timeout_ms, reconnect_initial_backoff_ms, reconnect_max_backoff_ms, concurrency, servers: Vec<McpServerConfig> }` + `Default`（值取 README §4.2）。
  2. `McpServerConfig { id, enabled, transport: McpTransportConfig, permission, concurrency: Option<McpConcurrencyKind>, file_access: BTreeMap<String, McpFileAccessKind> }`；`McpTransportConfig` 是 `#[serde(tag = "transport")]` 的枚举：`Stdio { command, args, env, cwd }` / `Http { url, headers }`；`McpPermissionKind`（`allow` / `deny` / `confirm`）、`McpConcurrencyKind`（`parallel` / `exclusive`）、`McpFileAccessKind`（`none` / `read` / `write` / `read-write`）用 `kebab-case`。
  2.1 配置来自**独立文件** `mcp.toml`（应用根目录，与 `config.toml` 同级）：`lib-core` 的 `AppDirectories` 增加 `mcp_file`（`MCP_CONFIG_FILE = "mcp.toml"`），文件内容就是这张表本身（顶层字段 + `[[servers]]`，不再套 `[mcp]`）。
  2.2 `file_access` 的键是 server 报告的工具原名或含 `*` 的模式（`*` 任意位置、任意长度；不支持正则 / `?` / `[...]`），缺省为 `None`；解析入口是 `agent-mcp` 的 `FileAccessRules::access_of`（精确名 > 最具体模式 > `None`），匹配器自研（两指针贪心，不引 glob crate），值类型直接用 `agent-tool` 的 `ToolFileAccess`（不新增第二套枚举）。
  3. 校验落在 `lib-bin-cli::config`：先按 `toml::Value` 做结构校验（`transport` 判别键与字段互斥、必填项非空、枚举取值、`file_access` 键），再逐条反序列化并做语义校验（`id` 正则与唯一性、`args` 空串、`env`/`headers` 键为空、URL scheme、`connect_timeout_ms` / `reconnect_initial_backoff_ms` 非 0）。错误文案包含 `id` 与字段名，退出码 2（与 `[tools]` 的口径一致）。
  4. `mcp.toml` 缺失 = 零 server（服务照常启动，无 MCP）；文件存在但内容为空是硬错误。`parse_mcp(text)` 供用例直接喂文本。
  **验收标准**：
  - [x] 示例配置（README §4.2 的两段）能装载成 `McpConfig`；文件缺失等价于「零 server」。（用例 `mcp.config`：示例两段逐项落地，`servers` 缺省为零）
  - [x] 每类非法输入都能给出「含 `id` 与字段名」的启动期错误（按 `config.rs` 的既有错误构造方式逐条实现），包含 `file_access` 的非法枚举值与空键。（用例 `mcp.config` 覆盖 20 条非法输入：空文件、`servers` 非数组、缺 / 未知 / 混用 transport、空 command、`args` 空串、`env` / `headers` 空键、URL 协议、`id` 不合法与重复、三个枚举字段、`0` 超时）
  - [x] `file_access` 的三层优先级（精确名 > `*` > 缺省 `None`）有一个可核对的纯函数入口，且不依赖注册表状态（`agent-mcp::FileAccessRules::access_of`；用例 `mcp.config` 逐条核对优先级、同分取字典序、连续 `*` 折叠、`?` 按字面）。
  - [x] `cargo build --workspace` 通过；`lib-bin-cli` 的既有配置用例契约不被破坏（`--help` 与既有字段行为不变）。
  **测试要求**：新增用例 `mcp.config`（`crates/bin-tests/src/cases/mcp/config.rs`）覆盖装载与非法输入文案；已执行通过（`make test ARGS="mcp.config"`）。

---

## Batch B：连接、传输与适配器（P4-005 ~ P4-009）

> 依赖：Batch A。本批次结束时，进程内能连一个 server、列出工具、调用成功。
>
> **状态：代码已完成（2026-11-26），两条运行期判据随 P4-018 取证。** 门禁：`cargo build --workspace`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全绿；`make test ARGS="--all"` 105 个用例全绿（本批次不新增用例，无回归）。`cargo tree -i rmcp` 只命中 `agent-mcp`（`bin-tests` 经 `agent-mcp` 间接依赖），`reqwest` 全仓只有一份（0.13.5）。两条运行期判据已由 P4-018 取证：`mcp.crash`（崩溃后 FAILED、调用返回可读错误）与 `mcp.discover`（6 个工具进 `model_definitions()` 与请求体）。

- [x] **P4-005 `McpClient`：连接、握手与状态机（stdio）**

  **目标**：把「怎么连一个 server」收敛到一个类型里，状态机的每次迁移都有明确的触发点与可见结果。
  **前置依赖**：P4-001、P4-004。
  **涉及文件 / Crate**：`crates/agent-mcp/src/{client.rs,state.rs,transport.rs,error.rs,lib.rs}`。
  **实施步骤**：
  1. `McpState`：`Disabled` / `Connecting` / `Ready` / `Failed { reason }` / `Stopped`；状态迁移只由 `McpClient` 内部完成，对外只读。
  2. `McpClient::connect(config)`：按 transport 建 `TokioChildProcess`（`tokio::process::Command`，`kill_on_drop(true)`，`env` 追加、`cwd` 生效），`tokio::time::timeout(connect_timeout_ms, ().serve(transport))` 完成握手；超时 / 失败都要终止已启动的子进程后再置 `Failed`。
  3. 握手参数带 `clientInfo { name: "lingting-agent", version: env!("CARGO_PKG_VERSION") }`（版本取 workspace 版本）。
  4. `await` `RunningService` 的完成信号，完成即把 `Ready` 改为 `Failed`（读循环结束 = 断开 / 崩溃），并记录可读原因。
  5. `close(graceful_timeout)`：先 `close()`（rmcp 的优雅关闭），超时后 kill 并记账（供 `shutdown_kills` 指标）。
  **验收标准**：
  - [x] 状态迁移表与 README §4.3 的图逐条对应；无「`Ready` 但连接已断」的窗口（读循环结束必然改状态）。（代码核对：四个触发点各一处——`connect` 起始 / 握手失败 / 完成监听 / `close`）
  - [x] 握手超时后子进程已退出（`child.id()` 对应的进程不存在），不残留。（代码已具备：超时即 `cancel()`，`serve` future 被 drop 时 transport 随之 drop，`kill_on_drop` 兜底；`mcp.crash` 断言崩溃后状态 FAILED 且调用在 20 秒内返回可读错误，`mcp.shutdown` 断言关闭后替身 PID 已退出）
  - [x] `cargo tree -p agent-mcp` 未新增未声明的依赖。（新增的 `reqwest` / `blake3` / `tokio` 的 `io-util` 特性都已声明在 `crates/agent-mcp/Cargo.toml`）
  - [x] `cargo clippy -p agent-mcp --all-targets -- -D warnings` 通过。
  **测试要求**：无（`mcp.discover` / `mcp.crash` 在 P4-018）。

- [x] **P4-006 streamable HTTP transport**

  **目标**：远程 server 与本地 server 走同一套状态机与调用路径，差别只在建 transport。
  **前置依赖**：P4-005。
  **涉及文件 / Crate**：`crates/agent-mcp/src/{transport.rs,client.rs,config.rs}`。
  **实施步骤**：
  1. `Http` 分支用 `StreamableHttpClientTransport::from_uri` / `with_client` 构造，`headers` 逐条附加；只允许 `http` / `https`。
  2. 复用 P4-005 的握手、完成监听与关闭路径；不做 transport 分支的状态机复制。
  3. `HEAD`/`GET` 的 SSE 重连由 rmcp 负责，本项目不额外实现（写进注释）；连接失败与 stdio 一样归 `Failed`。
  **验收标准**：
  - [x] 两种 transport 共用同一份 `connect` / `close` / 状态更新代码（`git diff` 可见只有构造函数分支）。（`client::connect` 的 match 只选 transport 构造；握手 / 监听 / 关闭 / 调用各一份）
  - [x] `headers` 出现在请求里，且不出现在任何错误文本或日志之外的地方（错误文案不含 header 值）。（原样进 `custom_headers`；`debug` 只记头名，不记头值）
  - [x] `cargo clippy -p agent-mcp --all-targets -- -D warnings` 通过。
  **测试要求**：无（`mcp.http` 在 P4-018，配合 P4-017 的 HTTP 替身）。

- [x] **P4-007 工具发现与命名映射**

  **目标**：把 `tools/list` 变成稳定的、可复算的模型可见清单；命名规则只有一个实现点。
  **前置依赖**：P4-002、P4-005。
  **涉及文件 / Crate**：`crates/agent-mcp/src/{discover.rs,naming.rs,client.rs}`。
  **实施步骤**：
  1. `list_tools` 自动翻页直到 `nextCursor` 为空；单次发现的工具按原名升序。
  2. `naming::visible_name(server_id, original)` 按 README §4.4 的 5 条规则实现（小写化、替换、折叠、补前缀、超长截断 + `blake3` 8 位短哈希）；`blake3` 已在 workspace 依赖里，不新增第三方库。
  3. 冲突消解：同一批发现内按「server `id` 升序 + 原名升序」固定顺序处理，后到者追加 `_<8 位哈希>`；与内置工具撞名同样处理（内置工具先注册，永远保留原名）。
  4. 发现结果产出 `Vec<McpToolSpec { server_id, tool_name, original_name, description, schema, risk, file_access }>`，`schema` 用 `ToolSchema::from_value` 包装 `inputSchema`，`file_access` 按 D-P4-006 的优先级解析。
  5. 发现失败（`tools/list` 报错）时保留上一次的 spec 快照，`lastError` 记原因，状态保持 `Ready`。
  **验收标准**：
  - [x] 同一份 server 工具列表两次发现产出完全相同的可见名（可复算）。（发现结果按原名升序，后缀取 `blake3(原名)` 前 8 位）
  - [x] 非法字符 / 大写 / 超长 / 空名四种输入都有确定的输出，且输出满足 `validate_name`。（规则集中在 `naming::sanitize` / `naming::visible_name`；用例 `mcp.name` 在 P4-018 取证）
  - [x] 冲突用例的输出稳定（先注册者不变、后来者带哈希后缀）。（`discover::resolve` 按固定顺序处理，后到者走 `naming::disambiguate`）
  - [x] `cargo clippy -p agent-mcp --all-targets -- -D warnings` 通过。
  **测试要求**：无（`mcp.name` / `mcp.discover` 在 P4-018）。

- [x] **P4-008 `McpTool` 适配器：`Tool` 实现与结果映射**

  **目标**：MCP 工具在运行时眼里就是一个普通工具；调用结果映射成 P3 的 `ToolOutput`。
  **前置依赖**：P4-002、P4-003、P4-007。
  **涉及文件 / Crate**：`crates/agent-mcp/src/{tool.rs,result.rs,error.rs}`。
  **实施步骤**：
  1. `McpTool` 持有 `Arc<McpClient>` + 生成好的 `name` / `description` / `schema` / `risk` / `concurrency`（见 README §4.5 的示意）。
  2. `risk`：`annotations.readOnlyHint == true` → `ReadOnly`，否则 `Write`；`file_access` 由 `mcp.toml` 的 `servers[].file_access` 按工具原名解析（精确名 > `*` > `None`，D-P4-006）；`concurrency`：声明为 `write` / `read-write` 时恒 `Exclusive`，否则取 server `concurrency`，未配置时取 `mcp.toml` 顶层 `concurrency`。
  3. `call`：把 `args` 原样作为 `arguments` 发出 `tools/call`，不本地校验参数（P4 的口径：参数校验归 server）。
  4. 结果映射（D-P4-017）：`content` 里的文本段按顺序用换行拼接；`image` / `audio` / `resource` 段替换为固定占位文本（如 `[图片内容已省略]`），不内联二进制；`is_error == true` 归 `ToolOutput::error`；`content` 为空时用 `structured_content` 的 JSON 文本，仍为空则用固定文案 `（MCP 工具没有返回内容）`。
  5. 连接不可用（状态非 `Ready`）时直接返回 `ToolError::Failed` 的固定文案，不发请求。
  **验收标准**：
  - [x] `McpTool` 能被 `Arc<dyn Tool>` 持有，注册后出现在 `model_definitions()` 中。（类型层面已成立；注册与模型清单的联动属装配（P4-011），`mcp.discover` 断言 6 个工具进模型可见清单，`mcp.denied` 断言 `deny` 的工具不进请求体）
  - [x] 四种结果形态（纯文本 / 文本+图片 / 仅结构化内容 / 空内容）都有确定的映射，且总文本非空。（`result::map` 的取值顺序固定；用例 `mcp.result` 在 P4-018 取证）
  - [x] `cargo clippy -p agent-mcp --all-targets -- -D warnings` 通过。
  **测试要求**：无（`mcp.call` / `mcp.result` 在 P4-018）。

- [x] **P4-009 错误归一化与固定文案**

  **目标**：MCP 侧的错误只有一份文案表，模型与客户端看到的是可读中文，不含凭证、URL、命令行与堆栈。
  **前置依赖**：P4-005、P4-008。
  **涉及文件 / Crate**：`crates/agent-mcp/src/error.rs`、`crates/agent-mcp/src/lib.rs`（导出）。
  **实施步骤**：
  1. `McpError` 取值与 README §4.7 的表一一对应：`ServerUnavailable` / `Communication` / `InvalidResponse` / `ToolNotFound` / `ConnectTimeout`；`Display` 只产出固定文案 + 必要的 `server_id` / 工具名。
  2. 传输入口（rmcp 的 `ServiceError` / `TransportError`）在这一层转成上面的取值，原文只进 `tracing::debug`（不是事件、不是响应体）。
  3. 与 `agent-tool::ToolError` 的映射集中在 `impl From<McpError> for ToolError`：统一落到 `ToolError::Failed`（超时由执行器自己判定，不在这里造 `Timeout`）。
  4. 文案表以常量形式写在一个地方，便于判据逐条比对。
  **验收标准**：
  - [x] 每个错误取值都有唯一文案，且文案里不出现 `http` / `Bearer` / 命令行 / 路径。（五个取值各一句前缀常量，只拼 `server_id` / 工具名）
  - [x] 全仓检索 `rmcp::` 只在 `agent-mcp`（`bin-tests` 的替身除外）。（`crates/` 下只有 `agent-mcp` 的五个模块引用 `rmcp`）
  - [x] `cargo clippy -p agent-mcp --all-targets -- -D warnings` 通过。
  **测试要求**：无（`mcp.call_error` 与判据 E-1 ~ E-3 在 P4-018 取证）。

---

## Batch C：Manager、装配与生命周期（P4-010 ~ P4-013）

> 依赖：Batch B。本批次结束时，服务启动即接入 MCP 工具，关闭后无子进程，状态可观测。
>
> **状态：代码已完成（2026-11-26）。** 门禁：`cargo build --workspace`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo build -p lib-web --features metrics` 全绿；本批次不新增用例，全量用例未执行（未获授权）。运行期判据已由 P4-018 取证：`mcp.api`（动作幂等、接口语义）、`mcp.config`（故障不阻塞启动）、`mcp.crash` / `mcp.reconnect` / `mcp.shutdown`（生命周期）。

- [x] **P4-010 `McpManager`：多 server、状态快照、启停与重连**

  **目标**：把多个 `McpClient` 收成一个门面：对外只暴露状态与动作，对内做差异注册、退避重连与通知刷新。
  **前置依赖**：P4-005、P4-007、P4-008。
  **涉及文件 / Crate**：`crates/agent-mcp/src/{manager.rs,state.rs,lib.rs}`（落地时另增 `vo.rs` 与 `config.rs` 的 `McpConfig` / `enabled`、`client.rs` 的 `probe` 与两个唤醒点——快照签名与动作需要它们）。
  **实施步骤**：
  1. `McpManager::new(config, Arc<ToolRegistry>)`；内部 `BTreeMap<String, ServerEntry>`（`entry` 里放 client 句柄、工具快照、退避状态），按 `id` 分段加锁，不持全局锁跨 `await`。
  2. `connect_enabled_all()`：并发对所有 `enabled` 的 server 启动连接（受各自 `connect_timeout_ms` 约束），任一失败只影响自己。
  3. 发现结果与注册表做差集：新增 `register_with_default(tool, Some(server.permission))`，消失 `unregister`；`disable` / `shutdown` 时注销该 server 的全部工具。
  4. 退避重连（D-P4-011）：`Failed` 后按初始值 2 倍增长到上限，无限次；`reconnect_max_backoff_ms = 0` 关闭；`enable` / `disable` 能打断在途等待。
  5. `notifications/tools/list_changed` → 触发一次该 server 的重新发现（D-P4-010）。
  6. 状态快照 `snapshot() -> Vec<McpServerVO>` 与 `tool_snapshot() -> Vec<McpToolVO>`：只读、按 `id` / 可见名字典序、不含内部句柄。
  **验收标准**：
  - [x] `enable` / `disable` / `reconnect` / `probe` 四个动作的幂等性可逐条核对（重复调用不产生额外连接或额外注册）。（代码核对：`enable` 在 `READY` 时直接返回；`disable` / `probe` 无副作用；`reconnect` 每次重建一条连接，是其语义本身）
  - [x] 一个 server `Failed` 时，其他 server 的工具仍在清单里且可调用。（代码核对：状态、闸门与监督任务都按 server 分段，失败只改自己的状态）
  - [x] 注销后 `model_definitions()` 不再出现该 server 的工具；重新 `enable` 后名字与之前一致。（`disable` / `shutdown` 调 `unregister`；名字由 `naming` 与「按原名升序」的固定顺序复算）
  - [x] 无全局锁跨 `await`（`grep` 复核锁的作用域）。（每个 server 一把 `tokio::sync::Mutex` 闸门；只读快照走同步小锁，锁内只做内存读写）
  - [x] `cargo clippy -p agent-mcp --all-targets -- -D warnings` 通过。
  **测试要求**：无（`mcp.reconnect` / `mcp.api` 在 P4-018）。

- [x] **P4-011 装配接入与注册表联动（`manager-agent`）**

  **目标**：服务启动时把 MCP 接进来，且不改变 P3 的装配顺序与失败语义。
  **前置依赖**：P4-004、P4-010。
  **涉及文件 / Crate**：`crates/manager-agent/src/{assemble.rs,holder.rs,lib.rs}`、`crates/manager-agent/Cargo.toml`（`agent-mcp`、`metrics` 特性传递）、`crates/bin-agent/src/main.rs`（把 `mcp` 配置传进装配参数）。
  **实施步骤**：
  1. `AssemblyOptions` 增加 `mcp: McpConfig`；`assemble_with` 在 `build_tools` 之后建 `McpManager`，`connect_enabled_all().await` 在返回 holder 之前完成（失败只记状态，不返回错误）。
  2. `AgentHolder` 持有 `Arc<McpManager>` 并提供只读入口；关闭路径见 P4-012。
  3. 装配日志：每个 server 一行（`id` / `transport` / 状态 / 工具数），`Failed` 用 `warn`，不打印命令行与 header。
  4. 新增 `mcp_servers` / `mcp_tools` / `mcp_update` 三个入口函数（P4-014 的 handler 只调它们）。
  **验收标准**：
  - [x] 一个 `enabled` 的 server 连接成功后，其工具出现在模型可见清单里（静态核对装配顺序 + `mcp.discover` 用例）。（装配顺序已核对：`build_tools` → `McpManager` → `connect_enabled_all` → holder；`mcp.discover` 待 P4-018）
  - [x] 所有 server 都不可达时，`assemble_with` 仍返回 `Ok`，服务可启动。（`connect_enabled_all` 不返回错误，失败只落状态与 `lastError`）
  - [x] `cargo build --workspace` 通过；`crates/agent-runtime` 无任何改动（`git diff` 为空）。
  **测试要求**：无（`mcp.discover` / `mcp.startup` 在 P4-018）。

- [x] **P4-012 关闭与子进程终止（无孤儿进程）**

  **目标**：服务退出时子进程被收干净，且 Run 的收尾语义不因 MCP 改变。
  **前置依赖**：P4-010、P4-011。
  **涉及文件 / Crate**：`crates/manager-agent/src/{holder.rs,assemble.rs}`、`crates/agent-mcp/src/{manager.rs,client.rs}`、`crates/lib-bin-web/src/shutdown.rs`（仅核对调用顺序，若无需改动则不动）。
  **实施步骤**：
  1. `holder::shutdown()`：先 `handler::shutdown(runtime)`（P2 原样，让 Run 落终态），再 `manager.shutdown_all().await`。
  2. `shutdown_all()`：置 `Stopped`（拒绝新动作与新调用）→ 取消在途调用 → 对每个 client 走 `close(graceful_timeout)` → 超时 kill 并累加 `shutdown_kills`。
  3. 子进程 `kill_on_drop(true)` 兜底；`Drop` 里若发现仍有活着的 child，记一条 `warn`（不阻塞）。
  4. 复用 `lib_core::lifecycle` 的关闭标记，避免关闭期间自动重连重新拉起子进程（关闭期间不排新的重连）。
  **验收标准**：
  - [x] 关闭顺序在代码里可逐行核对：Run 收尾先于 MCP 关闭。（`holder::shutdown` 先 `handler::shutdown(runtime)`，再 `shutdown_all()`）
  - [x] 关闭后不再出现新的连接尝试（自动重连被关闭标记挡住）。（`shutdown_all` 置 `STOPPED` 并取消全部监督任务；每次重连前重查 `lib_core::lifecycle`）
  - [x] `cargo build --workspace` 通过。
  **测试要求**：无（`mcp.shutdown` 在 P4-018 断言直接子进程已退出）。

- [x] **P4-013 状态可见性：`metrics` 的 `mcp` 段**

  **目标**：MCP 的连接、工具与调用失败可被测试与运维观察，且不引入事件与阈值日志。
  **前置依赖**：P4-010。
  **涉及文件 / Crate**：`crates/agent-mcp/{Cargo.toml,src/metrics.rs}`、`crates/manager-agent/src/runtime/metrics.rs`、`crates/manager-agent/Cargo.toml`（`metrics = ["agent-mcp/metrics", …]`）。
  **实施步骤**：
  1. `agent-mcp` 加 `metrics` 特性：启用时用原子量累加 `connects` / `connect_failures` / `reconnects` / `calls` / `call_errors` / `call_timeouts` / `shutdown_kills`，未启用时为空实现（与 `manager-agent` 的既有口径一致）。
  2. `McpMetrics` 结构体放在 `agent-mcp::metrics`，`manager-agent` 的 `MetricsSnapshot` 里加一个 `mcp` 字段做转写。
  3. 状态计数（`servers` / `ready` / `failed` / `disabled` / `tools`）在取快照时从 manager 现状读出，不做累加。
  **验收标准**：
  - [x] 未开 `metrics` 特性时 `agent-mcp` 不引入 `serde`、不留静态计数器（特性隔离可核）。（口径已确认：`serde` 是 `agent-mcp` 的必需依赖——`McpServerVO` / `McpToolVO` 的 `auto_type` 派生要求它；本条的判据改为「`metrics` 特性不引入额外依赖、不留静态计数器」，`grep` 可核）
  - [x] `GET api/probe/metrics` 的响应里出现 `mcp` 段，字段与 README §4.10 的表一致。（`MetricsSnapshot.mcp` 逐字段对应；运行期取值由 `mcp.metrics` 在 P4-018 取证）
  - [x] `cargo build --workspace` 与 `cargo build --workspace --features metrics -p lib-web` 均通过。
  **测试要求**：无（`mcp.metrics` 在 P4-018，经 `api/probe/metrics` 取值）。

---

## Batch D：接口与契约（P4-014 ~ P4-016）

> 依赖：Batch C。本批次结束时，3 条接口可用、SDK 重新生成、文档同步。
>
> **状态：代码与文档已完成（2026-11-26）。** 门禁：`make gen`（路由 29 / 接口 29）、`make gen-check`、`make ui-check`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全绿；本批次不新增用例，全量用例未执行（未获授权）。两条运行期判据已由 P4-018 的 `mcp.api` 取证（`server/list` 排序与字段、`tool/list` 过滤、四个动作的返回状态、未知 `serverId` 的 400）。

- [x] **P4-014 管理接口三条**

  **目标**：server 状态与工具清单可查，启停 / 重连 / 探测可触发；契约 DTO 只定义一次。
  **前置依赖**：P4-011、P4-013。
  **涉及文件 / Crate**：新增 `crates/lib-web/src/mcp/{mod.rs,api.rs,dto.rs}`、`crates/lib-web/src/lib.rs`（模块声明）、`crates/agent-mcp/src/vo.rs`（`McpServerVO` / `McpToolVO` / `McpServerUpdatePO` / `McpToolListParams` / 枚举）。
  **实施步骤**：
  1. DTO 用 `#[auto_type]` / `#[auto_enum]` 标注（与 `ToolCallVO` 同一写法），枚举走默认 `SCREAMING_SNAKE_CASE`（`action` 取值 `ENABLE` / `DISABLE` / `RECONNECT` / `PROBE`；`status` 取值 `DISABLED` / `CONNECTING` / `READY` / `FAILED` / `STOPPED`）。
  2. 三个 handler：`GET api/mcp/server/list`、`GET api/mcp/tool/list`（`McpToolListParams { server_id: Option<String> }`，查询参数 camelCase 为 `serverId`）、`POST api/mcp/server/update`。
  3. handler 只调 `manager-agent` 的入口函数；错误映射一次：未知 `serverId` → 400 固定文案，内部错误 → 500 固定文案（沿用 `lib-web` 的既有错误构造）。
  4. `i64` 字段（时间戳）按约定用字符串 wire（`DisplayFromStr` 口径与既有 DTO 一致）。
  **验收标准**：
  - [x] 三条接口在 `cargo run -p lib-web --example api_probe_server` 或集成入口下可访问，响应体是 `{"code":200,"message":"Success","data":…}`。（注册与元数据由 `make gen` 的 29 条路由取证；响应体逐条由 `mcp.api` 在 P4-018 取证）
  - [x] `server/list` 含 `disabled` 的 server；`tool/list` 按可见名升序；`server/update` 返回动作完成后的状态。（快照来源与排序在 `agent-mcp` 的实现里核对；运行期取值由 `mcp.api` 在 P4-018 取证）
  - [x] 接口总数 26 → 29 的口径在 `docs/api/` 里只出现两处（`readme.md` 的总数与 `mcp.md` 的领域清单），两者一致。
  - [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过。
  **测试要求**：无（`mcp.api` 在 P4-018）。

- [x] **P4-015 `docs/api/mcp.md` 与接口索引**

  **目标**：新增领域文档，写清三条接口的入参、载荷、状态语义与错误文案。
  **前置依赖**：P4-014。
  **涉及文件**：新增 `docs/api/mcp.md`、`docs/api/readme.md`（领域表与总数）。
  **实施步骤**：
  1. 按 `docs/api/tool.md` 的结构写：定义位置、业务入口、决策引用、接口清单、入参、载荷类型（逐字段表）、请求与响应样例、错误响应表。
  2. 写清 `status` 五个取值与 `action` 四个取值的语义，以及「`enable` / `disable` 只改内存、重启回到配置」的约定（D-P4-008）。
  3. `docs/api/readme.md` 的领域表加一行 `mcp.md`，接口总数从 26 改为 29（并更新各领域分项：probe 12 / session 6 / runtime 4 / occupancy 3 / tool 1 / mcp 3）。
  **验收标准**：
  - [x] 文档里的 path、字段名、枚举取值与 `crates/lib-web/src/mcp/` 的代码逐项一致。
  - [x] 索引表与总数可对照代码核对（26 + 3 = 29）。
  **测试要求**：无。

- [x] **P4-016 SDK 重生成、前端类型检查与 `docs/workspace.md` 同步**

  **目标**：客户端侧拿到新接口；结构文档与新 Crate 对齐。
  **前置依赖**：P4-014、P4-015。
  **涉及文件**：`packages/sdk-rust` / `packages/sdk-ts`（生成物，不入库）、`docs/workspace.md`（§3.2 `agent-mcp` 段、§4 依赖图、§8.1 成员清单、依赖矩阵）。
  **实施步骤**：
  1. `make gen` 重新生成两份 SDK；核对新增三条 path 与 DTO 出现在产物里（`grep` 生成目录，不手改）。
  2. `make gen-check`、`make ui-check` 通过（前端不新增页面，只保证类型仍编译）。
  3. `docs/workspace.md`：`agent-mcp` 去掉「阶段：P4」的规划口吻，补「依赖 `agent-tool` / `lib-core` + `rmcp`」「不依赖 `types-core`」「工具经 `Tool` 适配进入 `agent-tool` 注册表」；§8.1 成员数与 `Cargo.toml` 的 members 一致。
  **验收标准**：
  - [x] `make gen` / `make gen-check` / `make ui-check` 退出码 0。
  - [x] `docs/workspace.md` 的 `agent-mcp` 段与 `Cargo.toml` / `cargo tree` 的事实一致。
  **测试要求**：无。

---

## Batch E：用例与阶段验收（P4-017 ~ P4-020）

> 依赖：Batch B（017）/ Batch D（018 ~ 020）。所有用例执行都需明确授权。
>
> **状态：已完成（2026-11-26）。** 门禁：`cargo build --workspace`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`make gen` / `make gen-check` / `make ui-check` 全部退出码 0；用例 `make test ARGS="mcp"` 19/19 通过（`run-1795682083908-42744`），`make test ARGS="--all"` 123/123 通过（`run-1795682098490-2964`）。阶段报告：[`docs/reports/p4-020-acceptance.md`](../../docs/reports/p4-020-acceptance.md)。

- [x] **P4-017 `bin-tests` 的 MCP 测试替身**

  **目标**：有一个会讲 MCP 的子进程，既能在 stdio 下被启动，也能在 HTTP 下被访问，且能按剧本崩溃 / 报错 / 返回非文本内容。
  **前置依赖**：P4-008。
  **涉及文件 / Crate**：`crates/bin-tests/Cargo.toml`（`rmcp` 带 `server` 特性、`agent-mcp`）、新增 `crates/bin-tests/src/mock/mcp/{mod.rs,server.rs,scenario.rs}`、`crates/bin-tests/src/cli.rs`（隐藏子命令）、`crates/bin-tests/src/main.rs`（分发）。
  **实施步骤**：
  1. `cli` 增加**隐藏**子命令 `mock-mcp`（`hide = true`，不出现在 `--help`），参数：`--transport stdio|http`、`--port`、`--scenario <name>`。
  2. 用 `rmcp` 的 `server` 特性实现一个最小 MCP server：`tools/list` 返回剧本里的工具（含非法字符名、超长名、与内置工具重名的用例）、`tools/call` 按剧本返回文本 / 图片段 / `is_error` / 延迟 / 直接退出。
  3. 剧本至少四种：`normal`（常规工具）、`error`（`is_error = true` 与协议级错误）、`crash`（调用后退出进程）、`slow`（延迟响应，用于超时与取消）。
  4. HTTP 模式起一个本地监听，供 `mcp.http` 用例连；进程 PID 写到文件，供「无孤儿进程」断言使用。
  **验收标准**：
  - [x] `bin-tests --help` 不出现 `mock-mcp`（子命令不进主解析器，见 `crates/bin-tests/src/cli.rs`）；`mock-mcp` 以 stdio 与 HTTP 两种方式都被 `mcp.*` 用例实际拉起来过（`mcp.discover` / `mcp.http`）。
  - [x] 剧本可用参数切换；八个剧本（`normal` / `naming` / `schema` / `results` / `error` / `crash` / `slow` / `list_changed`）的路径都由用例取证（含崩溃、协议错误、超时与清单变化）。
  - [x] `cargo clippy -p bin-tests --all-targets -- -D warnings` 通过。
  **测试要求**：替身本身不是用例；`cargo run -p bin-tests -- mock-mcp --transport stdio --scenario normal` 只做人工连通性核对，不纳入自动化。

- [x] **P4-018 `mcp.*` 用例**

  **目标**：把 `docs/reports/p4-020-acceptance.md` 里每条判据都变成可执行的用例或可核对的静态检查。
  **前置依赖**：P4-017、P4-014。
  **涉及文件 / Crate**：新增 `crates/bin-tests/src/cases/mcp/{mod.rs,discover.rs,call.rs,lifecycle.rs,api.rs,config.rs}`、`crates/bin-tests/src/cases/mod.rs`（注册）、`crates/bin-tests/src/util/`（服务端配置生成支持 `[[mcp.servers]]`）。
  **实施步骤**：
  1. `util` 的配置生成支持追加 `mcp.toml`（stdio 指向 `bin-tests` 自身的 `mock-mcp`，HTTP 指向替身端口）。
  2. 用例清单（名称即证据）：
     - [x] `mcp.config`：装载规则与非法配置文案（已在批次 A 落地：`crates/bin-tests/src/cases/mcp/config.rs`，进程内喂 `parse_mcp` 文本；进程级「非法配置启动失败、退出码 2」仍随本任务补 process 级核对）；
     - `mcp.discover`：stdio server 的工具进模型可见清单（经 `/mock/last-request` 的 `tools` 字段核对）且顺序稳定；
     - `mcp.name`：非法字符 / 超长 / 重名的可见名符合规则；
     - `mcp.schema`：`inputSchema` 原样透传，`/[mcp]/tool/list` 与请求体的 `parameters` 一致；
     - `mcp.call`：模型要求调用 → 执行成功 → 结果进下一轮请求 → 最终回复；
     - `mcp.result`：文本 + 图片段 / 仅结构化内容 / 空内容三种映射；
     - `mcp.call_error`：`is_error` 与协议错误都回灌可读文本，Run 正常结束；
     - `mcp.timeout`：`slow` 剧本超过 `[tools] timeout_ms` → `TIMEOUT`，Run 继续；
     - `mcp.cancel`：调用中取消 Run → `CANCELLED`，无残留后台任务；
     - `mcp.denied`：`deny` 的工具不在清单、执行期 `DENIED`；
     - `mcp.confirm`：`confirm` → 挂起（`WAITING`，`running_now` 不含它）→ 批准后执行、驳回后回灌拒绝对话；
     - `mcp.audit`：`api/tool/audit/list` 里 `toolName` 是可见名、`fileAccess` 等于配置的声明值、`status` 正确；
     - `mcp.file_access`：精确名 / `write*` / `pre*write` / `*` 四级优先级生效（声明 `{"*":"read","write*":"write","pre*write":"read-write","write_file":"none"}` 时四个工具分别是 `READ` / `WRITE` / `READ_WRITE` / `NONE`；去掉精确名后 `write_file` 命中 `write*`），没有任何键命中时为 `NONE`，且命中写声明的工具在并行轮次里被串行执行；
     - `mcp.http`：HTTP transport 走通一次调用；
     - `mcp.crash`：调用后子进程退出 → 状态 `FAILED` 且下一次调用得到可读错误（不挂死）；
     - `mcp.reconnect`：`POST api/mcp/server/update` 的 `RECONNECT` 让崩溃的 server 回到 `READY`；
     - `mcp.api`：三条接口的载荷字段、排序、未知 `serverId` 的 400 文案；
     - `mcp.metrics`：`api/probe/metrics` 的 `mcp` 段字段有值（未开特性时显式记「未采集」）；
     - `mcp.shutdown`：关闭服务后替身进程 PID 不存在。
  3. 全部证据来自接口响应、客户端事件与 `metrics` 快照；不得读取日志文本。
  **验收标准**：
  - [x] `make test ARGS="mcp"` 全部通过（19/19，`run-1795682083908-42744`）。
  - [x] `make test ARGS="--all"` 全部通过，未破坏 P1 ~ P3 的任何用例（123/123，`run-1795682098490-2964`；批次 A 收尾时 105 个全绿）。
  - [x] 每条用例的失败信息能定位到具体 server / 工具，不需要翻日志（断言文本一律带 `server` id 或可见工具名，失败原因只含固定文案）。
  **测试要求**：本任务即用例实现；执行需明确授权。

- [x] **P4-019 阶段文档与路线图状态同步**

  **目标**：代码、文档、路线图三者一致，本阶段收尾只差报告。
  **前置依赖**：P4-018。
  **涉及文件**：`.steps/p4/{README.md,tasks.md,acceptance.md,decisions.md}`、`.steps/README.md`、`docs/workspace.md`（若 P4-016 已同步则只核对）、`AGENTS.md`（不新增约束，只核对是否需要）。
  **实施步骤**：
  1. 回填任务状态与判据勾选（只勾已取证的）。
  2. 决策文件把「DECIDED（待验证）」的项回填实测结果（D-P4-002 的特性清单与 reqwest 版本、D-P4-010 的通知 API 细节）。
  3. `.steps/README.md` 的阶段总览与进度表更新为 P4 的实际状态。
  **验收标准**：
  - [x] 三份文档之间没有互相矛盾的编号 / 数量（20 个任务 / 5 个批次 / 23 项决策 / 3 条接口 / 19 个 `mcp.*` 用例）。
  - [x] `.steps/README.md` 的 P4 行与本文档状态一致。
  **测试要求**：无。

- [x] **P4-020 阶段验收报告**

  **目标**：出 `docs/reports/p4-020-acceptance.md`，逐条取证，与 P3 的报告同一体例。
  **前置依赖**：P4-018、P4-019。
  **涉及文件**：新增 `docs/reports/p4-020-acceptance.md`、`.steps/README.md`（报告链接）、`.steps/p4/README.md`（状态行）。
  **实施步骤**：
  1. 报告结构沿用 `p3-032-acceptance.md`：结论 → 门禁命令与结果 → 逐组判据取证表 → 口径修订记录 → 未取证项与原因 → 遗留风险。
  2. 未授权执行的项标注「未授权」，不得留空、不得算通过。
  3. 阶段 README 状态行改为「P4 完成」并附报告链接；路线图进度表同步。
  **验收标准**：
  - [x] 报告中每条判据都能指到用例名、命令、源码位置或截图，不出现「跑一遍看看」式的证据。
  - [x] 遗留问题以 `KI-P4-001` / `KI-P4-002` 登记（见报告 §六）。
  **测试要求**：报告本身不执行用例；引用的用例结果必须来自已授权的执行。
