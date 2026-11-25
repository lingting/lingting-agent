# P3 任务清单

**状态：Batch A 已完成（P3-001 ~ P3-004，仅代码 / 静态检查 / 文档）；Batch B 已验收（P3-005 ~ P3-009：代码 + 静态核对 + 全量回归 75 个用例通过，`tool.*` 用例待 Batch H）；Batch C 完成（P3-010 ~ P3-012：代码 + 静态检查；P3-012 在 Batch E 之后补齐，`tool.context` 用例待 Batch H）；Batch D 完成（P3-013 ~ P3-016：代码 + 静态检查；挂起态按 D-P3-027 改为 `WAITING`，`tool.*` 用例待 Batch H）；Batch E 完成（P3-017 ~ P3-020：代码 + 静态检查 + 两份 SDK 重新生成 + 全量回归 75 个用例通过；仅「`0001` 旧库可读」未取证）；Batch F 完成（P3-021 ~ P3-025：五个内置工具 + `builtin::register_all` 装配；代码 + 静态核对 + 全量回归 75 个用例通过，`tool.builtin.*` 用例待 Batch H）；Batch G 完成（P3-026 ~ P3-029：`api/client/post` 改名与三种 `kind`、服务端确认链路与重连补发、取消链路收尾 `PENDING` 审计、TUI 与 WebUI 的工具卡片与确认交互；`make gen` / `make ui-check` / clippy 与格式检查通过 + 全量回归 75 个用例通过（`runtime.*`、`web.ui` 等全部改走 `client/post`），工具确认相关判据（F-2 / F-4 ~ F-7、C-3）仍待 Batch H 用例）；Batch H 完成（P3-030：27 个 `tool.*` 用例 + `client.post` 落地；P3-031：接口 / 结构 / 开发规范三份文档同步；P3-032：阶段验收报告 `reports/p3-032-acceptance.md` 已出稿，`make test ARGS="--all"` 103 个用例全部通过（报告 `target/tests/run-1795605450775-43892/README.md`），唯一「部分取证」项是 C-3 的批量确认点用例）。** 28 项决策（D-P3-001 ~ D-P3-028）全部定案（[decisions.md](decisions.md) 的「定案状态」）。阶段目标与边界见 [README.md](README.md) §一 / §二，判据见 [acceptance.md](acceptance.md)。

**本轮补证（用户授权新增用例、执行用例与浏览器目测）**：先前只剩的三条判据已全部取证——P3-003 的「必填 / 可选 / 枚举 / 数字四类字段对照 schema 与 serde」由 `tool.schema` 逐类断言；P3-017 的「`0001` 旧库可读」由 `persist.migration` 的「`0001` 时代」夹具（只跑 `0001`、账本只记 version 1）取证，迁移后旧消息的工具字段为 `null`、账本 `[1, 2]`；P3-029 的视觉层级由新增的 `tool.web.e2e`（真实 Chromium + `calculator = "confirm"`）与两张截图人工目测取证。同轮还把 P3-002「请求不含 `fileAccess`」（`tool.schema`）与 P3-018「终态审计写库失败不影响 Run 结论」（`loop_/tool.rs` 的 `audit()` 静态核对）补齐。回归：`make test ARGS="--all"` 103 个用例全部通过（报告 `target/tests/run-1795599811697-44180/README.md`），`make ui-check` / `cargo fmt --check` / `cargo clippy -p bin-tests --all-targets -- -D warnings` 均通过。P3-031 / P3-032（Batch H 剩余两项）本轮不推进。
批次之间强序，批次内可并行。**每个任务的「测试要求」只允许在 `crates/bin-tests` 落地用例；其他 Crate 不得写 `#[cfg(test)]`**（仓库 `AGENTS.md` 第 28 条）。**所有需要执行用例的步骤都必须先取得明确授权**；未授权时只推进代码、静态检查与文档部分，相关验收项保持未勾选。

本阶段的四条硬线：

1. **Runtime 不认识具体工具**：执行循环只调 `ToolRegistry` / `ToolExecutor`，不出现 `match tool_name`。
2. **工具不反向依赖运行时**：`agent-tool` 的依赖里没有 `agent-runtime` / `agent-route` / `rig-core`。
3. **失败即回灌，不卡死**：参数非法、超时、执行失败、权限拒绝四种情况都必须变成一段模型可读文本并结束本次 Run（或继续下一轮），不允许 panic、不允许挂起、不允许把内部错误原文交给模型。
4. **工具轨迹先落库再继续**：进入下一轮之前，assistant `tool_calls` 与本次全部 TOOL 结果必须已提交（D-P3-024）；落库失败即本轮失败。

---

## Batch A：工具抽象、Schema 与注册（P3-001 ~ P3-004）

> 依赖：无。本批次只造「抽象 + 注册表」，没有任何执行路径，也不接 Provider。

- [x] **P3-001 新增 Crate `agent-tool` 与依赖边界**

  **目标**：让工具系统有一个自己的领域 Crate，依赖方向从一开始就锁死。
  **前置依赖**：无。
  **涉及文件 / Crate**：`Cargo.toml`（workspace members、`workspace.dependencies`）、新增 `crates/agent-tool/{Cargo.toml,src/lib.rs}`、`docs/workspace.md`（§3.2 的 `agent-tool` 段落地状态、§8.1 的成员清单）。
  **实施步骤**：
  1. workspace 增加成员与内部依赖声明；`agent-tool` 的依赖只写 `lib-core` / `agent-core` / `agent-context-core` / `serde` / `serde_json` / `schemars`（版本与 `rig-core` 传递进来的 1.2.3 对齐）/ `tokio` / `async-trait` / `futures-util` / `framework-http`（`reqwest` 特性）。
  2. `lib.rs` 只声明模块并写清依赖方向与「不依赖 agent-runtime / agent-route」的边界注释（照 `agent-context-core/src/lib.rs` 的写法）。
  3. `docs/workspace.md` 的 `agent-tool` 段：去掉「阶段：P3」的规划口吻，补「内置工具位于 `builtin` 子模块」「依赖 `framework-http` 仅为 `http_request`」「`service-core` 依赖它（审计记录与查询载荷）」三条。
  4. 全局检索 `agent-tool` 的既有引用（`.steps/p2/acceptance.md` A-7 的历史记录不改）确认没有其它文档需要同步。
  **验收标准**：
  - [x] `cargo check -p agent-tool` 通过；`cargo tree -p agent-tool` 里没有 `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `lib-web` / `bin-*`。
  - [x] `cargo tree -i agent-tool` 只有 `agent-runtime` / `service-core` / `bin-tests`，没有反向边。（Batch A 尚无消费方，当前反查只命中自身。）
  - [x] `docs/workspace.md` 的成员清单与 `Cargo.toml` 的 members 一致（两处可逐条对照）。
  **测试要求**：无（不写测试代码）。检查方式为 `cargo check`、`cargo tree` 与 `git diff`。

- [x] **P3-002 Tool 契约类型**

  **目标**：定义 Tool Trait、Definition、风险等级、本地文件操作声明、并发语义、错误与结果类型、执行上下文。
  **前置依赖**：P3-001。
  **涉及文件 / Crate**：`crates/agent-tool/src/{tool.rs,definition.rs,error.rs,output.rs,context.rs,schema.rs}`。
  **实施步骤**：
  1. `Tool` trait 按 README §4.2 的签名落地：`name` / `description` / `schema` / `risk` / `file_access` / `concurrency` / `retryable` / `call`。`call` 用 `async_trait`（需要 `Arc<dyn Tool>`）。
  2. `ToolDefinition`：`{ name, description, schema, risk, file_access }`，只放「发给模型看」与「权限判定 / 声明需要」的字段；`ToolRisk` 取 `ReadOnly` / `Network` / `Write`。
  3. `ToolFileAccess` 取 `None`（不操作本地文件）/ `Read`（只读）/ `Write`（只写）/ `ReadWrite`（读写），并提供 `default_concurrency()`：`None` / `Read` → `Parallel`，`Write` / `ReadWrite` → `Exclusive`；`ToolConcurrency` 取 `Parallel` / `Exclusive`，trait 上的 `concurrency()` 默认实现即返回 `default_concurrency(self.file_access())`（工具可覆盖）。
  4. `ToolError` 取 `InvalidArguments` / `Denied` / `ConfirmRequired` / `Timeout` / `Retryable` / `Failed`，`Display` 只产出模型可读文本（不携带路径细节、URL、栈）。
  5. `ToolOutput`：`{ text, is_error, truncated }`，附构造器 `ok` / `error`。
  6. `ToolContext`：执行期只读参数（`home` 应用根目录、`session_id`、`run_id`、`tool_call_id`、`deadline`），**不含** registry / service / provider；工具不得从上下文里拿到超出自己职责的东西。
  7. 工具名规则集中在一个 `validate_name` 函数里（`^[a-z][a-z0-9_]{1,63}$`），注册期调用。
  **验收标准**：
  - [x] `ToolFileAccess` 四个取值都有对应说明，且 `default_concurrency()` 的映射可逐项对照（`None` / `Read` → `Parallel`，`Write` / `ReadWrite` → `Exclusive`）。
  - [x] `ToolDefinition` 带 `file_access`，但发给模型的请求里不包含它（`LlmToolDefinition` 只有 name / description / parameters）。（`tool.schema` 用例取 `/mock/last-request` 的完整请求体，检索 `fileAccess` / `file_access` / `risk` 三个字段 0 命中；`LlmToolDefinition` 仍是 name / description / parameters 三字段）
  - [x] `Tool` 能被 `Arc<dyn Tool>` 持有（`Send + Sync`，无泛型参数出现在 trait 方法签名里）。
  - [x] `ToolContext` 的字段可逐项对照 README §4.4：工具拿不到 registry / 数据库 / Provider。
  - [x] `cargo clippy -p agent-tool --all-targets -- -D warnings` 通过。
  **测试要求**：无。

- [x] **P3-003 Schema 单一来源（schemars 派生 + serde 校验）**

  **目标**：参数类型是 schema 与校验的唯一来源，不存在手写 JSON Schema。
  **前置依赖**：P3-002。
  **涉及文件 / Crate**：`crates/agent-tool/src/schema.rs`、`crates/agent-tool/Cargo.toml`。
  **实施步骤**：
  1. `schema::of::<A: JsonSchema>() -> ToolSchema`：`schemars::schema_for!(A)` → 剥掉 `$schema` / `title` / `$defs` 之类模型不需要的键，产出 `{"type":"object","properties":{…},"required":[…]}`。
  2. `schema::parse::<A>(Value) -> Result<A, ToolError>`：`serde_json::from_value`；`deny_unknown_fields` 由各 `*Args` 自己声明；错误加工成 `InvalidArguments`，文本里保留字段名与原因（去掉 serde 的 `at line x column y` 噪声）。
  3. `ToolSchema` 同时导出 `auto_type` 的契约 DTO（`ToolDefinitionVO`），让「有哪些工具、参数长什么样」能被 SDK 与前端消费。
  4. 记录一条口径：`schemars` 只负责生成，不做校验；校验一律走 `parse`（README §4.2）。
  **验收标准**：
  - [x] 全仓检索不到手写的 `"type": "object"` 参数 schema（`builtin` 之外应为零命中；`builtin` 内也没有）。
  - [x] `schema::of::<A>()` 对「必填 + 可选 + 枚举 + 数字」四类字段的输出与 `serde` 的接受/拒绝行为一致（同一份 `*Args` 上对照可核）。（`tool.schema` 逐类对照：必填用 `CalculatorArgs`（`required` 只含 `expression`，缺必填 → `参数不合法：missing field \`expression\``）、可选与数字用 `FileSearchArgs`（`required` 只含 `name`，缺 `path` / `max_results` 可执行，`max_results` 被描述为 integer 且收到字符串时 serde 拒绝）、枚举用 `CurrentTimeArgs`（`zone` 只描述成 string、不列取值白名单，`utc` 接受、`Asia/Shanghai` 由工具拒绝）；`deny_unknown_fields` 两侧一致：schema 输出 `additionalProperties: false`，serde 拒绝未知字段）
  - [x] `schemars` 版本与 `Cargo.lock` 里已有的 1.2.3 一致，没有引入第二份（`cargo tree -d` 无新增重复）。
  **测试要求**：无（Batch H 的 `tool.schema` 用例在 `bin-tests` 里覆盖同一口径）。

- [x] **P3-004 `ToolRegistry` 与模型可见清单**

  **目标**：注册与查找的唯一入口；「哪些工具能发给模型」由权限过滤后的清单决定。
  **前置依赖**：P3-002、P3-003。
  **涉及文件 / Crate**：`crates/agent-tool/src/registry.rs`。
  **实施步骤**：
  1. `ToolRegistry::new(policy)` 接收 `ToolPolicy`（P3-005 的产物；本任务先用最小形态：按名字给 `Allow` / `Deny` / `Confirm`）。
  2. `register(Arc<dyn Tool>)`：校验名字规则、拒绝重名、拒绝清单里 `Deny` 的工具进入可见集（仍保留在表内供「模型凭空调用」时返回 DENIED）。
  3. `model_definitions() -> Vec<ToolDefinition>`：按名字**字典序**稳定排序（同一份配置两次运行必须产出同序清单，模型侧缓存才可复现）。
  4. **`Confirm` 的工具仍然进入可见清单**（模型可以请求，只是要等确认）；只有 `Deny` 被摘掉（D-P3-005）。
  5. `lookup(name) -> Option<&Arc<dyn Tool>>`：执行期查找。
  6. 空清单是合法状态（全部 deny 或未注册任何工具），此时请求里不带 `tools`。
  **验收标准**：
  - [x] 重名注册返回错误，不覆盖先注册的实现。
  - [x] `model_definitions()` 的顺序只由名字决定，与注册顺序无关。
  - [x] 被 `Deny` 的工具不出现在 `model_definitions()`，但 `lookup` 仍能命中（供 DENIED 回灌）；被 `Confirm` 的工具**在**清单里。
  **测试要求**：无（Batch H 用 `tool.denied` / `tool.confirm.*` 覆盖）。

---

## Batch B：权限、执行器与审计类型（P3-005 ~ P3-009）

> 依赖：Batch A。本批次的出口是「执行器能被独立驱动」，尚未接 Provider 与 Loop。
>
> **验收结论（已通过）**：`make check` / `make gen-check` / `make ui-check` 全部退出码 0；`make test ARGS="--all"` 75 个用例全绿；启动期配置校验用隔离 `AGENT_HOME` + `bin-agent server` 实测。执行器本身的端到端断言（`tool.invalid_args` / `tool.timeout` / `tool.parallel` / `tool.truncate` / `tool.audit`）仍待 Batch H 的用例落地——它们依赖 Batch D 的工具循环。

- [x] **P3-005 权限模型与 `[tools]` 配置面**

  **目标**：权限可配置、可判定、可测；配置错误在启动期就失败。
  **前置依赖**：P3-001。
  **涉及文件 / Crate**：`crates/types-core/src/config/{tools.rs,mod.rs,agent.rs}`、`crates/lib-bin-cli/src/config.rs`、`crates/agent-tool/src/permission.rs`。
  **实施步骤**：
  1. `types-core::config::ToolConfig`：`timeout_ms` / `max_result_chars` / `max_parallel_calls` / `max_retries` / `permissions: BTreeMap<String, ToolPermissionKind>` / `file: ToolFileConfig` / `http: ToolHttpConfig`，全部有缺省值（`#[serde(default)]`）。**不含确认超时**（D-P3-023）。
  2. `ToolPermissionKind`：`Allow` / `Deny` / `Confirm`（三值，小写或 `kebab-case`，与 `ProviderKind` 的写法一致）。
  3. `AgentConfig` 增加 `max_tool_rounds`（默认 8）。
  4. `ToolPolicy`（`agent-tool`）：提供 `permission_of(name, default_risk) -> ToolPermission`；判定顺序 `deny > confirm > allow > 默认`（README §4.3）。**实现口径**：`agent-tool` 不依赖 `types-core`，装配层把 `[tools.permissions]` 逐条喂进 `ToolPolicy::new`。
  5. 内置工具的默认等级由 `ToolRisk` 推导（`ToolRisk::default_permission`）：`ReadOnly` → `Allow`，`Network` / `Write` → `Deny`。因此 `http_request`（`Network`）默认拒绝，其余四个只读工具默认允许（D-P3-005）。**实现口径**：不按工具名写死名单，新增工具只需声明风险等级。
  6. `lib-bin-cli::config` 增加 `[tools]` 装载与校验：未知工具名、负数、`max_parallel_calls = 0`、`max_tool_rounds = 0`、`allow_dirs` 里的相对路径、`allow_hosts` 里的空串全部是启动期硬错误（退出码 2，文案点名 `[tools] xxx`）。
  7. `[tools]` 段整体缺省时取默认值。
  **验收标准**：
  - [x] 只写 `[tools] timeout_ms` 时其余字段取默认值，服务可启动。（实测：隔离 `AGENT_HOME` + `bin-agent server -a` 退出码 0，`api/probe/health` 200）
  - [x] 非法配置（负数 / 未知工具名 / 相对 `allow_dirs` / `max_tool_rounds = 0`）启动即失败，错误文案点名字段。（实测：六种非法配置均退出码 2，文案分别为 `[tools] max_parallel_calls 必须大于 0` / `[tools.permissions] 未知的工具名：nope` / `[tools.file] allow_dirs 必须是绝对路径` / `[tools.http] allow_hosts 不能包含空 host` / `[agent] max_tool_rounds 必须大于 0` / TOML 解析错误）
  - [x] 判定顺序可对照代码：`deny` 永远优先于 `allow`，未出现在配置里的工具按默认等级判定（`http_request` 默认 `Deny`）。（`ToolPolicy::permission_of` 只做「配置值优先，否则 `ToolRisk::default_permission()`」一次判定）
  - [x] 配置里没有任何「确认超时」字段（`git diff` 可核）。（检索 `确认超时` 只命中注释）
  **测试要求**：无（配置校验的实测在 Batch H 的 `tool.config` 用例里）。**本批次用隔离 `AGENT_HOME` + `bin-agent server` 实测过启动期校验；`tool.config` 用例仍待 Batch H 落地并补「缺省值逐字段对照」。**

- [x] **P3-006 `ToolExecutor`：校验 → 权限 → 超时 → 取消 → 归一化**

  **目标**：一次工具调用只有一个入口，各步顺序固定（README §4.4），并把「需确认」作为独立结局交回 Loop。
  **前置依赖**：P3-004、P3-005。
  **涉及文件 / Crate**：`crates/agent-tool/src/executor.rs`。
  **实施步骤**：
  1. `ToolExecutor` 持有 `Arc<ToolRegistry>` + `ToolLimits`（由 `[tools]` 派生；本 Crate 不依赖 `types-core`，故不直接持有 `ToolConfig`）+ `PathBuf home`；`execute(&self, call: ToolCall, cancel: &CancellationToken) -> ExecOutcome`。
  2. `ExecOutcome` 取三态：`Done(ToolCallRecord)` / `ConfirmRequired { record }` / `Cancelled`。**确认不是失败**，Loop 据此挂起（D-P3-004）。
  3. 固定顺序：查找（未注册 → DENIED）→ 权限（deny → DENIED，不执行；confirm → `ConfirmRequired`）→ 参数（`schema::parse` 失败 → INVALID_ARGUMENTS）→ `tokio::time::timeout` 与 `cancel.cancelled()` 并列 `select!` → 归一化（截断）→ 产出 `ToolCallRecord`。**实现口径**：参数校验发生在工具的 `call` 内部（工具才知道自己的 `*Args`），执行器把返回的 `InvalidArguments` 映射成 `INVALID_ARGUMENTS` 状态，顺序上仍在权限判定之后、真正执行之前。
  4. 重试：仅在 `tool.retryable(&error)` 且 `max_retries > 0` 时重试，重试次数计入 `duration_ms`、不额外产出事件（事件只在最终结果上发一次）。
  5. 审计写入不在本任务（`ToolCallRecord` 只产出，落库由 Batch D / E 调 service）。
  **验收标准**：
  - [x] 顺序在代码里是线性顺序（`git diff` 可核），任一步失败都短路成 `ExecOutcome`，不 panic。（`execute` 依次 lookup → permission → `run`，每个分支直接 `return`）
  - [x] 权限拒绝时不调用 `tool.call`；`confirm` 时同样不调用 `tool.call`，只返回 `ConfirmRequired`（可用只会 panic 的假工具在 `bin-tests` 里验证，见 P3-030）。（代码路径已核：两条分支都在 `tool.call` 之前返回；假工具用例待 P3-030）
  - [x] 超时与取消产生不同状态：`TIMEOUT` 记录带错误文本，`Cancelled` 不带结果。（`ToolError::Timeout { limit_ms }` → `ToolCallStatus::Timeout` 且 `result` 为固定文案；`ExecOutcome::Cancelled` 不携带记录）
  **测试要求**：无（`tool.invalid_args` / `tool.timeout` / `tool.cancel` / `tool.confirm.*` 覆盖，**待 Batch H 授权后执行**；本批次只做代码核对）。

- [x] **P3-007 一轮内的并行与串行分段**

  **目标**：同一轮的多个调用并发执行、结果按模型给出的顺序回填；副作用工具段间串行。
  **前置依赖**：P3-006。
  **涉及文件 / Crate**：`crates/agent-tool/src/executor.rs`（或拆 `executor/batch.rs`）。
  **实施步骤**：
  1. `execute_batch(&self, calls: Vec<ToolCall>, cancel) -> BatchOutcome`（`outcomes` + `cancelled`，确认点用 `confirm_point()` 取）：按 `ToolConcurrency` 把调用序列切成段（连续 `Parallel` 为一段，`Exclusive` 单独成段）；`ToolConcurrency` 取 trait 的 `concurrency()`，其默认值由 `ToolFileAccess` 推导（D-P3-021），不按工具名特判。
  2. 段内用 `futures_util::stream::iter(..).buffered(max_parallel_calls)` 而不是一次性 `join_all`：并发上限可配，且结果顺序由输入顺序决定。
  3. 段间串行：上一段全部结束才开始的下一段。
  4. **任一调用返回 `ConfirmRequired` 时停止后续段**（本轮剩余调用不再执行，等唤醒后重来）：确认点之后的调用不应先跑掉。返回结果里保留已完成的记录与「第一个确认点」。
  5. 事件发出由 Batch D 的 runtime 负责；本任务只保证结果顺序与 `tool_call_id` 一一对应。
  **验收标准**：
  - [x] 输入顺序与返回顺序一一对应（交换 `Exclusive` 工具位置后，段划分随之变化且结果顺序仍与输入一致）。（`buffered` 按输入顺序产出，段划分由 `concurrency()` 决定）
  - [x] 并发上限生效：`max_parallel_calls = 1` 时同一段内的调用不重叠。（段内用 `buffered(max_parallel_calls.max(1))`）
  - [x] 出现确认点时，其后的调用没有被执行（可用会写审计的假工具验证顺序）。（`confirm_index` 先截断输入，段内命中 `ConfirmRequired` 即停止后续段；假工具用例待 P3-030）
  - [x] 输出确定性：同一批输入两次执行的记录顺序完全相同。（记录顺序只由输入顺序决定）
  **测试要求**：无（`tool.parallel` 覆盖，**待 Batch H 授权后执行**；本批次只做代码核对）。

- [x] **P3-008 结果截断与模型可读错误**

  **目标**：工具结果永远不会撑爆 Context，且模型能看出「被截断」。
  **前置依赖**：P3-006。
  **涉及文件 / Crate**：`crates/agent-tool/src/output.rs`、`crates/agent-tool/src/error.rs`。
  **实施步骤**：
  1. `truncate(text, max_chars) -> (String, bool)`：按字符（不是字节）截断，保留前 `max_chars` 个字符，追加固定提示 `\n…（结果已截断，原文共 N 字符）`。**实现口径**：截断标志由返回值给出，审计表没有对应列，`ToolOutput.truncated` 留给构造工具产出的地方使用。
  2. 错误文本优先级：`ToolError::Display` 的固定文案 → 模型可见；serde 的错误细节只保留字段名与原因。
  3. 高危内容一律不进模型可见文本：绝对路径只写「白名单根」的相对形式、URL 只写 host + path、错误里不带 `api_key`（复用 `Secret` 的思路：不拼进字符串）。
  **验收标准**：
  - [x] 截断只按字符边界发生，中文与 emoji 不被截半（代码可核）。（`chars().take(max_chars)`，上限按字符计）
  - [x] 截断后的文本长度 ≤ `max_chars` + 固定提示长度，且提示里点明原文长度。（提示为 `\n…（结果已截断，原文共 N 字符）`，N 是截断前的字符数）
  - [x] 错误文案里不出现 `api_key` 明文与完整本地路径（`file_read` / `http_request` 的失败文本逐条对照）。（Batch F 落地后对照：`file_read` 的失败只可能是 `path::REJECTED_TEXT` / `该路径不是普通文件` / `文件超过读取上限（> N 字节）` / `二进制文件不支持读取`，都不带路径；`http_request` 的失败为 `只支持 http / https 地址` / `请求的主机不在允许的范围内` / `请求失败` / `连接失败或读取超时` / `初始化 HTTP 客户端失败`，都是固定文案（不回灌 URL、host、请求头与底层错误原文），参数脱敏另由 `redact_arguments` 在写出侧完成）
  **测试要求**：无（`tool.truncate` 覆盖，**待 Batch H 授权后执行**）。

- [x] **P3-009 审计记录类型与脱敏**

  **目标**：一次调用在内存里有一份完整、可落库、已脱敏的记录，并覆盖「等待确认」这一中间态。
  **前置依赖**：P3-006。
  **涉及文件 / Crate**：`crates/agent-tool/src/audit.rs`。
  **实施步骤**：
  1. `ToolCallRecord { tool_call_id, tool_name, file_access: ToolFileAccess, status: ToolCallStatus, arguments: String, result: Option<String>, duration_ms, tool_round, created_at_ms }`（与迁移 `0002` 的列一一对应）。**实现口径**：不设 `error` 字段——协议与审计表都只有一处文本落点，失败原因与结果同存 `result`。
  2. `ToolCallStatus`：`Ok` / `Error` / `Denied` / `Timeout` / `Cancelled` / `InvalidArguments` / **`Pending`**（挂起等待确认）。
  3. `redact_arguments(&Value) -> String`：键名命中 `api_key` / `apikey` / `token` / `secret` / `password` / `authorization` / `cookie`（大小写不敏感）的值替换 `***`；整体上限 2048 字符（超长按字符截断并追加提示）。
  4. `ToolCallStatus` 同时是 Run 事件的取值来源（Batch D 直接复用，不另造枚举）。
  5. 挂起 / 驳回的收尾构造器：`pending(...)`（状态 `Pending`，无结果）、`rejected(...)`（状态 `Denied`，结果文本「用户拒绝调用该工具」）。
  **验收标准**：
  - [x] `ToolCallRecord` 的字段与迁移 `0002` 的列一一对应（逐字段对照表，无多余字段、无缺列）。（对照 README §4.7 的 `tool_calls` 列：`id` / `run_id` / `session_id` 由 Service 插入时补齐，其余八列与记录字段逐项同名同义；迁移文件本身在 P3-017 落地）
  - [x] 脱敏是「按键名」而不是「按值猜」：键名命中即替换，未命中的长文本按长度截断。（`is_secret_key` 做大小写不敏感的键名包含匹配，仅整体文本超 2048 字符时截断）
  - [x] `Pending` 记录不带 `result` / `error`；`Denied` 记录带固定拒绝文本。（`pending()` 的 `result` 为 `None`；`rejected()` 写入 [`REJECTED_TEXT`]）
  - [x] 该结构是可序列化的纯数据，不持有 `Arc<dyn Tool>` 之类的运行时句柄。（`#[derive(Serialize, Deserialize)]`，字段全是标量 / `Option<String>`）
  **测试要求**：无。

---

## Batch C：Provider 与 Context 扩展（P3-010 ~ P3-012）

> 依赖：Batch A。本批次不碰执行循环，只把「工具」这件事表达进请求与 Context。
>
> **状态**：P3-010 / P3-011 / P3-012 已完成（代码 + 静态检查通过，未执行用例）。P3-012 依赖的 `Message` 三个工具字段随 Batch E（P3-018 / P3-019）落地，本节因此在本批次之后补齐。

- [x] **P3-010 `LlmProvider` 契约扩展**

  **目标**：抽象层能表达工具定义、工具调用与工具结果，且不泄漏 Rig 类型。
  **前置依赖**：P3-002。
  **涉及文件 / Crate**：`crates/agent-route/src/provider.rs`。
  **实施步骤**：
  1. `LlmRequest` 增加 `tools: Vec<LlmToolDefinition>`（空 = 不带工具）。
  2. `LlmToolDefinition { name, description, parameters: serde_json::Value }`（**不含** `fileAccess` / `risk`）。
  3. `LlmMessage` 从 `{ role, content }` 扩展为能表达三种形态：普通文本（system / user / assistant）、助手请求调用（`tool_calls: Vec<LlmToolCall>` + 可选文本）、工具结果（`tool_call_id` + 文本）。守卫约定：`TOOL` 消息必须带 `tool_call_id`，`ASSISTANT` 带 `tool_calls` 时 `content` 可为空。
  4. `LlmStreamEvent` 增加 `ToolCall { id, name, arguments }`（一次调用一条，arguments 是完整 JSON 文本）与 `ToolCallsDone`（本轮工具调用清单结束，可继续消费或结束）。
  5. 现有 `Delta` / `Usage` / `Done` 的语义与顺序不变。
  **验收标准**：
  - [x] `agent-route` 的公开签名里没有 `rig_core::*` 类型（`grep` 可核：`crates/agent-route/src` 下除 `src/rig/` 外无任何 `rig_core` 引用）。
  - [x] 无工具时 `LlmRequest.tools` 为空，既有调用方编译期只需补一个字段。
  - [x] `TOOL` 消息缺 `tool_call_id` 时在构造处就拒绝（返回 `LlmError::InvalidRequest`），不会把非法历史发给上游（`LlmMessage::text` 拒绝 `Tool` 角色，`LlmMessage::tool_result` 拒绝空标识）。
  - [x] `LlmToolDefinition` 的字段清单恰好是 name / description / parameters（逐字段核对）。
  **测试要求**：无。

- [x] **P3-011 Rig Adapter：工具定义翻译与工具调用聚合**

  **目标**：Rig 的工具能力被完整接进项目事件流，且 Rig 类型不出 `rig` 子模块。
  **前置依赖**：P3-010。
  **涉及文件 / Crate**：`crates/agent-route/src/rig/{openai.rs,stream.rs,error.rs}`。
  **实施步骤**：
  1. 请求翻译：`LlmToolDefinition` → `rig_core::completion::ToolDefinition`（`ToolName` 校验失败按 `InvalidRequest` 处理）；`tools` 为空时不设置，保持与 P2 逐字节等价的请求体。
  2. 消息翻译：普通消息照旧；助手 `tool_calls` → `AssistantMessage { content: [Text?, ToolCall…] }`；工具结果 → `Message::User { content: [ToolResult] }`（`rig_core::completion::message::UserContent::ToolResult`）。
  3. 流翻译：消费 `StreamEvent::Start{kind: ToolCall, name}` / `Arguments{json}` / `End` 与 `PartKind::ToolCall`，按 `part` 聚合成完整调用；`End` 缺失或 JSON 非法时产出 `LlmError::InvalidRequest`（不 panic、不猜参数）。
  4. 聚合完成后产出一条 `LlmStreamEvent::ToolCall`，全部工具调用结束后产出 `ToolCallsDone`；`Done` 仍然只在流真正结束时产出。
  5. 保持 P2 的既有行为：纯文本流不受影响，`StreamEvent::Reasoning` 仍然忽略。
  **验收标准**：
  - [x] 无工具请求的请求体与 P2 一致：`tools` 为空时根本不调用 `.tools()`，请求体里不出现 `tools` 键（`/mock/last-request` 对照用例待 Batch H 授权）。
  - [x] 分片拼接正确：`Arguments` 分片按 `part` 拼接后整体解析；缺 `End`、分片不是 JSON 对象、`End` 内容不是工具调用时返回 `InvalidRequest` 而不是半截 JSON。
  - [x] `grep -n "rig_core" crates/agent-route/src` 只命中 `src/rig/`；`crates` 里其余 `rig` 字样仅出现在文档注释与 `agent-route` 的 `pub mod rig` 声明。
  - [x] 工具结果的工具名按 `tool_call_id` 回查历史里的助手调用补齐（Rig 的 `ToolResult` 要求工具名，而结果消息只带标识）；查不到按 `InvalidRequest` 拒绝。
  **测试要求**：无（`tool.stream` 用例覆盖，**需授权**）。

- [x] **P3-012 Context：工具轨迹进历史，按组裁剪**

  **已完成**：前置数据（`Message` 的 `tool_calls` / `tool_call_id`）随 P3-018 / P3-019 落地后补齐。**实现口径**：`ContextItem` 除 `tool_call_id` 与组标识（`group_id`）外还带 `tool_calls`（助手调用清单），否则助手侧的工具调用无法进入下一轮请求；`ContextSource::Tool` 仍只标记工具结果项。
  **目标**：工具轨迹成为历史的一部分（D-P3-003），裁剪以「工具调用组」为单位（D-P3-025）。
  **前置依赖**：P3-010。
  **涉及文件 / Crate**：`crates/agent-context-core/src/{source.rs,item.rs}`、`crates/agent-context/src/{builder.rs,trim.rs}`。
  **实施步骤**：
  1. `ContextSource` 追加 `Tool`；`ContextItem` 增加 `tool_call_id: Option<String>`（`Tool` 来源使用）与组标识（例如本轮 assistant 消息的 `message_id`）。
  2. `history_item` 不再过滤 `TOOL`：assistant 带 `tool_calls` 时把调用清单带上，`TOOL` 消息带 `tool_call_id`；`SYSTEM` 仍不进请求。
  3. 裁剪改为**成组**：把历史切成「普通消息」与「工具调用组（assistant + 其全部 TOOL 结果）」，先按条数上限、再按预算，整组保留或整组丢弃，仍然只丢最旧的一端。
  4. 组内预算：每条消息照 `MESSAGE_TOKEN_OVERHEAD` + 正文估算，组整体参与 `available_tokens` 比较。
  5. 配对校验：`TOOL` 的 `tool_call_id` 找不到对应 assistant 调用时返回 `ContextError`（数据损坏）。
  6. `metadata.trimmed_messages` 语义改为「被裁掉的消息条数」，注释写清。
  **验收标准**：
  - [x] 无工具时 `build` 的输出与 P2 完全一致（同样的 Item 序列与 metadata）。（静态核对：没有工具轨迹时每个 Item 自成一段，`chunks` / `dropped_by_count` / `dropped_by_budget` 退化为 P2 的逐条算法；`trimmed_messages` 仍等于被丢弃的消息条数）
  - [x] 有工具轨迹时顺序为「系统 → 历史（含工具组）→ 本次输入」，组始终完整。（历史按传入顺序逐条映射，组内 assistant 与 TOOL 项的 `group_id` 相同，裁剪只按段整段丢弃）
  - [x] 裁剪只发生在组边界：结果里不存在「有 assistant `tool_calls` 却缺 TOOL 结果」或「孤儿 TOOL」。（段由相邻同组 Item 组成，切点只在段边界；孤儿 TOOL 在组装阶段即返回 `ContextError::ToolPairing`，不会进入裁剪）
  - [x] `SYSTEM` 仍被过滤，系统提示词仍来自 `AGENTS.md`。（`history_items` 跳过 `MessageRole::System`，系统项仍由 `SystemPrompt` 读取 `AGENTS.md`）
  **测试要求**：无（`tool.context` 覆盖，**需授权**；本批次只做代码与静态核对）。

---

## Batch D：Agent Loop、事件与挂起恢复（P3-013 ~ P3-016）

> 依赖：Batch B + Batch C。本批次是「端到端能跑」「挂起后能恢复」的出口。
>
> **状态**：P3-013 ~ P3-016 完成（代码 + 静态检查 + 全量回归）。`cargo check --workspace --all-targets` / `clippy -D warnings` / `fmt --check` / `make gen` / `make gen-check` / `make ui-check` 全部退出码 0；`make test ARGS="--all"` 75 个用例通过（含 `web.e2e` 的 6 个浏览器用例），说明「空工具清单」这条路径与 P2 完全一致。**仍未取证的是工具路径本身**：仓库里还没有一个内置工具（Batch F）也没有 `tool.*` 用例（Batch H），因此挂起 / 恢复、工具并行、取消工具、失败回灌这几条仍只能靠代码核对，相关判据保持未勾选。挂起态按 D-P3-027 改用 `WAITING`（不再回 `QUEUED`）。

- [x] **P3-013 Run 事件扩展（工具事件）**

  **目标**：工具调用的开始、完成、待确认三类事件进入既有事件流，客户端可配对与展示。
  **前置依赖**：P3-009（状态枚举）、P3-010。
  **涉及文件 / Crate**：`crates/agent-core/src/session/event.rs`。
  **实施步骤**：
  1. `RunEventKind` 追加 `ToolCallStarted` / `ToolCallCompleted` / `ToolCallConfirmRequired`（`serde(rename_all = "snake_case")` 与 `strum(serialize_all = "snake_case")` 两处同时改）。
  2. `RunEvent` 追加可选字段：`tool_call_id` / `tool_name` / `tool_arguments` / `tool_status` / `tool_duration_ms` / `tool_round`。`error` 复用既有字段。
  3. 新增 `ToolCallStatus` 枚举（`auto_enum`），取值与 `agent-tool` 的记录状态一致（含 `PENDING`）。**实现口径**：枚举定义在 `agent-core` 的事件契约里，`agent-tool` 的 `ToolCallStatus` 改为 `pub use` 转发——同名类型只能注册一次，转发同时也消掉了「审计 OK / 事件 Ok」的二次映射。
  4. 既有字段与语义一律不动；事件载荷仍用扁平结构（P2 的口径）。
  **验收标准**：
  - [x] SSE 的 `event:` 行与 JSON 的 `event` 字段取值一致（三个新事件）。（三者与既有事件同处一个枚举，`serde` 与 `strum` 属性同时作用于新取值）
  - [x] 旧字段不变：`seq` / `runId` / `sessionId` / `createdAtMs` 的位置与含义与 P2 相同。（只追加字段，既有字段的注释与语义未改）
  - [x] `cargo check --workspace` 通过（`lib-bin-tui` 的穷尽分支会在 P3-028 跟进）。（TUI / CLI / `bin-tests` 的穷尽匹配先补「忽略」分支并注明 P3-028 / P3-029 跟进；`clippy -D warnings` 与 `fmt --check` 一并通过）
  **测试要求**：无（`tool.events` 覆盖，**需授权**）。

- [x] **P3-014 多轮工具循环、每轮落库与挂起恢复**

  **目标**：`drive` 变成「最多 `max_tool_rounds` 轮」的工具循环；每轮边界落库；命中确认即挂起，唤醒后从历史恢复。
  **前置依赖**：P3-006、P3-007、P3-011、P3-012、P3-013、P3-005（`max_tool_rounds`）。
  **涉及文件 / Crate**：`crates/agent-runtime/src/{loop_.rs,port.rs,lib.rs}`（新增 `loop_/{request,tool,trace}.rs`）、`crates/manager-agent/src/{assemble.rs,holder.rs}`、`crates/lib-bin-web/src/server.rs`、`crates/bin-agent/src/main.rs`、`crates/service-core/src/{repository/{run,message}.rs,session/{driver,service}.rs}`。
  **实施步骤**：
  1. `port.rs` 增加 `tools: Arc<ToolRegistry>` 与 `ToolExecutor` 的装配字段；`new` 的签名扩展，装配点（`lib-bin-web`）同步。**实现口径**：`[tools]` 配置随 `AssemblyOptions` 进装配层，注册表与执行参数由 `manager-agent` 建好后挂到 `AgentHolder`，`lib-bin-web` 只负责转交。
  2. `loop_.rs` 抽出 `run_round(...) -> RoundOutcome`：发请求 → 消费流（文本进 `message_delta`，工具调用聚合）→ 无调用即 `Final`，有调用即 `ToolCalls`。（落地为 `loop_/request.rs` 的 `stream`）
  3. 主循环：`for round in 1..=max_tool_rounds`；`Final` 走 `complete`，`ToolCalls` 走「执行批次 → **每轮边界落库** → 追加轨迹 → 下一轮」；循环用尽落 `FAILED`「工具调用轮次超过上限」。
  4. 每轮开始时用「历史（含已落库的工具组）+ 本次输入 + 本轮新增轨迹」重建请求，不缓存上一轮请求体。
  5. **每轮边界落库**（D-P3-024）：assistant `tool_calls` 消息与全部 TOOL 结果消息在同一事务提交；失败则本轮失败并落 `FAILED`「工具轨迹落库失败」。
  6. **挂起**（D-P3-023 + **D-P3-027**）：批次返回 `ConfirmRequired` → 写 `PENDING` 审计与 `runs.pending_tool_call_id` → `RUNNING` 转 **`WAITING`** → 发 `tool_call_confirm_required` → 释放许可 → 结束 `drive`。**偏差**：不再回 `QUEUED`（D-P3-027），否则队列会立刻重取同一个 Run。
  7. **恢复**：`drive` 开始时读回本 Run 已落库的工具轨迹，若最后一个工具组有未配对的调用，说明这是唤醒后的重入——按该调用重新执行（已批准）或写拒绝结果（已驳回，`tool_calls` 状态已是 `DENIED`），然后继续推理。确认点之后的未配对调用一并执行 / 一并驳回，历史里不留「有 `tool_calls` 却缺结果」的组。
  8. 累计 token：每轮 usage 相加作为 Run 的 `input_tokens` / `output_tokens`（写进注释）。
  **验收标准**：
  - [x] 无工具调用时行为与 P2 完全一致（同一用例的事件序列与 P2 基线相同）。（空工具清单 → 请求不带 `tools` → 首轮即 `Final`；`make test ARGS="--all"` 75 个用例全绿，`chat.*` / `run.*` / `persist.*` / `web.e2e` 全部走新的多轮 `drive`，事件序列与终态断言未变）
  - [x] 一轮内有多个调用时全部执行，且下一轮请求包含全部工具结果。（批次按输入顺序产出记录，TOOL 消息逐条进轨迹并参与下一轮请求）
  - [x] 轮次用尽落 `FAILED` 且 `error` 为固定文案，助手消息不写库。（`error::TOOL_ROUNDS_EXCEEDED`，失败路径不调 `complete_run`）
  - [x] 进入下一轮之前，容器里已能查到本轮的 assistant `tool_calls` 与全部 TOOL 消息（`message/list` 可核）。（`append_tool_round` 在 `Step::Continue` 之前提交）
  - [x] 命中确认时 Run 停在 `WAITING`（按 D-P3-027 改判）、`drive` 已结束（无后台任务）、`running_now` 不含它。（`running_now` 只数 `RUNNING`，`drive` 返回 `RunOutcome::Suspended` 并 drop 许可；`tool.confirm.approve` 在挂起时取快照核到 `running_now = 0`，并核到同会话第二个 Run 在 `max_concurrent_runs = 1` 下仍能执行）
  - [x] 唤醒后不重跑已完成的轮次（Mock 侧请求次数可核）。（轮次号从已落库的工具组数继续数，已完成组只进轨迹、不再执行）
  - [x] `agent-runtime` 源码里没有具体工具名（`grep -n "calculator\|file_read\|http_request" crates/agent-runtime` 为空）。（实际检索无命中）
  **测试要求**：无（`tool.loop` / `tool.rounds` / `tool.confirm.*` 覆盖，**需授权**）。

- [x] **P3-015 工具执行期间的取消与超时收尾**

  **目标**：取消 Run 能中断正在执行的工具，且收尾与 P2 的取消路径一致。
  **前置依赖**：P3-014。
  **涉及文件 / Crate**：`crates/agent-runtime/src/loop_.rs`（含 `loop_/tool.rs`）、`crates/agent-tool/src/{executor.rs,batch.rs}`。
  **实施步骤**：
  1. 工具批次与 `cancel.cancelled()` 并列 `select!`；命中即丢弃批次 future（`ToolExecutor` 内部的单调用 future 一并 drop），工具实现不感知取消。
  2. 取消后按既有路径 `cancelled(runtime, run, feed, cancel.stop())` 收尾：不写助手消息、发 `run_cancelled` / `server_shutdown`。
  3. 已产出的 `tool_call_started` 在取消时补一条 `tool_call_completed`（`toolStatus = CANCELLED`）；`PENDING` 的审计记录同样收尾为 `CANCELLED`，不留悬挂行。（`PENDING` 行由取消链路的 `cancel_run` / `cancel_session_runs` 清空待确认列并落 `CANCELLED`）
  4. 挂起期间被取消（释放占用、断连、服务关闭）：清 `pending_tool_call_id`、按既有链路落 `CANCELLED`，不再唤醒。
  5. 超时（单工具）与取消区分：超时是工具失败，Run 继续；取消是 Run 终止。
  **验收标准**：
  - [x] 取消时不存在仍在执行的工具任务（`tool.cancel` 用慢响应服务验证：取消后服务端不再有后续事件、慢服务端连接被关闭）。（`tool.cancel`：取消后 1 秒内无新事件、假服务端记到连接被关闭、审计 0 条、`run/get` 为 `CANCELLED`）
  - [x] 每个 `tool_call_started` 都有配对的 `tool_call_completed`（含 `CANCELLED`）。（取消分支对已发出的开始事件逐条补 `CANCELLED` 完成事件；驳回分支同样补一条 `DENIED` 完成事件）
  - [x] 取消路径的终态事件类型与 P2 一致（`run_cancelled` vs `server_shutdown`）。（仍走原来的 `cancelled(...)` + `kind_of(stop)`）
  - [x] 挂起中断连后 `runs.pending_tool_call_id` 为空（无悬挂挂起态）。（`cancel_run` / `cancel_session_runs` / 启动自愈的 SQL 在落终态时一并把 `pending_*` 置空；`WAITING` 已进 `open_statuses`）
  **测试要求**：无（`tool.cancel` / `tool.confirm.disconnect` 覆盖，**需授权**）。

- [x] **P3-016 三种失败的回灌**

  **目标**：参数非法、执行超时、执行失败都变成模型可读文本，Run 继续下一轮而不是卡死。
  **前置依赖**：P3-014。
  **涉及文件 / Crate**：`crates/agent-runtime/src/{loop_.rs,loop_/tool.rs}`、`crates/agent-tool/src/{error.rs,output.rs}`。
  **实施步骤**：
  1. 三种失败在轨迹里都表现为一条 `TOOL` 消息（`is_error` 的文本），进入下一轮请求，而不是走 Run 的 `FAILED`。
  2. 只有「结构性问题」才落 `FAILED`：Provider 错误、Context 超预算、轮次用尽、工具轨迹落库失败、轨迹配对损坏。区分规则写进 `loop_.rs` 的注释。
  3. 每种失败的模型可见文本固定（便于断言）：参数非法 `参数不合法：<字段与原因>`、超时 `工具执行超时（> <ms> 毫秒）`、执行失败 `工具执行失败：<脱敏原因>`、权限拒绝 `工具未授权：<name>`、用户驳回 `用户拒绝调用该工具`。
  4. 失败文本同样受 `max_result_chars` 限制。
  **验收标准**：
  - [x] 五个固定文案在代码里各只有一个产点（`grep` 可核）。（前四条只在 `agent-tool/src/error.rs` 的 `Display`，第五条只在 `audit.rs` 的 `REJECTED_TEXT`；运行时只拼接、不再造文案）
  - [x] 三种失败之后 Run 仍能以 `COMPLETED` 结束（模型在下一轮给出最终答案）。（`tool.invalid_args` / `tool.timeout` / `tool.failed` / `tool.denied` 四条用例：Run 均 `COMPLETED`，终答里能看到对应固定文案）
  - [x] 失败不产生 panic、不产生孤儿任务、不留 `PENDING` 审计行。（四条失败用例的审计分别落 `INVALID_ARGUMENTS` / `TIMEOUT` / `ERROR` / `DENIED`，无 `PENDING`；`tool.cancel` 的审计为空、快照里 `running_now = 0`）
  **测试要求**：无（`tool.invalid_args` / `tool.timeout` / `tool.failed` / `tool.denied` 覆盖，**需授权**）。
---

## Batch E：持久化、审计与接口（P3-017 ~ P3-020）

> 依赖：Batch B（记录类型）。与 C / D 可并行，但接口发布要等 D 能产出记录。
>
> **状态**：P3-017 ~ P3-020 完成（代码 + 静态检查 + 全量回归）。`cargo check` / `clippy -D warnings` / `fmt --check` / `make gen` / `make gen-check` / `make ui-check` 全部通过；`make test ARGS="--all"` 75 个用例通过（含 `persist.migration` 的同目录二次启动、`sdk.generator`）。仍未取证的两条：`0001` 时代旧库可读（无此类夹具）、`message/list` 的工具字段取值（等 Batch D / F 产出工具轨迹）；P3-020 的 `RunEvent` 新字段部分等 P3-013 落地后再生成一次。

- [x] **P3-017 迁移 `0002` 与 Repository**

  **目标**：审计、工具轨迹与挂起状态落 SQLite，SQL 仍只在 Repository。
  **前置依赖**：P3-009。
  **涉及文件 / Crate**：`crates/lib-db/migrations/0002_tool_calls.sql`、`crates/lib-db/src/migrate.rs`（若迁移列表是显式的）、`crates/service-core/src/repository/{tool.rs,message.rs,run.rs,mod.rs}`。
  **实施步骤**：
  1. 建 `tool_calls` 表（列见 README §4.7，含 `file_access`、`status` 含 `PENDING`）与索引；枚举列存 serde wire 值，不加 `CHECK`。
  2. `ALTER TABLE messages ADD COLUMN` 三列：`tool_call_id` / `tool_name` / `tool_calls`。
  3. `ALTER TABLE runs ADD COLUMN` 两列：`pending_tool_call_id` / `pending_tool_round` + `idx_runs_pending`。
  4. `repository/tool.rs`：`insert`（`ON CONFLICT(run_id, tool_call_id) DO NOTHING`）、`update_status`（`ON CONFLICT … DO UPDATE`，挂起收尾用）、`list_by_run`（按 `created_at_ms`、`id` 升序）。
  5. `repository/message.rs`：支持写入带工具列的消息；`repository/run.rs`：挂起列的读 / 写 / 清（条件更新 `WHERE status='QUEUED'`）。
  6. 迁移可重复执行（`IF NOT EXISTS` 语义由 `ALTER TABLE` 的幂等包装承担：先查 `PRAGMA table_info` 再决定是否加列）。
  **验收标准**：
  - [x] 迁移连续执行两次不报错；外键、唯一约束与 `idx_runs_pending` 真实存在。（`persist.migration` 通过：同一 data 目录连续启动两次，第二次 `audit.error = 0`、`database.errors = 0`；用例 data 目录上核对：`schema_migrations` 有 1 / 2 两行、`tool_calls` 含 `UNIQUE (run_id, tool_call_id)` 与 `REFERENCES runs(id) ON DELETE CASCADE`、`idx_tool_calls_run` 与 `idx_runs_pending` 存在、`messages` / `runs` 的新列就位）
  - [x] `grep -rn "INSERT\|SELECT\|ALTER" crates/service-core/src --include=*.rs` 仍只命中 `repository/`。
  - [x] 删除会话后 `tool_calls` 随之清空（`ON DELETE CASCADE` 链路经 `runs` 生效）；工具消息随 `messages` 一并删除。（静态核对：`tool_calls.run_id REFERENCES runs(id) ON DELETE CASCADE` 已在库中生效，连接池开 `foreign_keys(true)`；P3 没有删除会话的接口与 Repository 方法，`grep -rn "DELETE FROM" crates/service-core crates/manager-agent` 为空，因此无法从接口侧触发）
  - [x] `0001` 既有数据可读（无工具列值为 `NULL`）。（`persist.migration` 先造「`0001` 时代」夹具——只跑 `lib-db/migrations/0001_init.sql`、账本只记 version 1，并写入一条 `0001` 结构的会话与消息——再让服务端补 `0002`；断言旧会话与旧消息仍可读、正文与角色未被改写、`toolCallId` / `toolName` / `toolCalls` 均为 `null`，迁移账本为 `[1, 2]`）
  **测试要求**：无（`tool.audit` / `persist.migration` 覆盖，已授权执行）。`persist.migration` 里新增的「`0001` 时代」夹具是本目录唯一直接读写数据库的地方（夹具必须早于服务端启动），业务断言仍只走生成的 SDK。

- [x] **P3-018 Service 层的轨迹写入、挂起与唤醒**

  **目标**：Runtime 只调 Service，不碰 Repository；挂起 / 唤醒的状态迁移有唯一入口。
  **前置依赖**：P3-017。
  **涉及文件 / Crate**：`crates/service-core/src/session/{service.rs,error.rs}`、`crates/service-core/Cargo.toml`（新增 `agent-tool` 依赖）。
  **实施步骤**：
  1. `record_tool_call(record)`：`Pending` / 终态都走这里（幂等 upsert）；失败返回 `SessionError::Internal`，调用方只记日志（`Pending` 插入失败**必须**让本轮失败，因为它是挂起的事实源）。
  2. `tool_calls_of_run(run_id) -> Vec<ToolCallRecord>`：不存在返回空列表。
  3. `append_tool_round(messages)`：一轮的 assistant `tool_calls` 与全部 TOOL 结果**同一事务**写入，`seq` 按会话递增。
  4. `suspend_run(run_id, tool_call_id, round)`：条件更新 `WHERE status='RUNNING'` → `QUEUED` + 写待确认列；返回是否命中。
  5. `resume_run(run_id, tool_call_id) -> ResumeDecision`：校验待确认列一致后清空，返回「批准 / 驳回」；驳回时同事务把审计记录置 `DENIED`。
  6. `pending_runs_of_session(session_id)`：给重连补发与「取消时清挂起」用。
  7. 依赖方向：`service-core` 新增 `agent-tool`（`agent-tool` 不依赖 `service-core`，无环）；这条边写进 `docs/workspace.md`。
  8. 审计 / 轨迹写入不得在会话锁内（P2.1 的「锁内收尾」口径）。
  **验收标准**：
  - [x] `cargo tree -p service-core` 与 `cargo tree -p agent-tool` 之间没有环。（`cargo tree -i agent-tool` 的直接消费者只有 `service-core`，另有 `agent-runtime` / `bin-tests` 经它间接引入）
  - [x] 一轮的两类消息要么都写入要么都不写（事务边界可核：`Repository::with_tool_round` 是唯一入口，逐条 `insert_message_in` 在同一事务内完成）。
  - [x] `suspend_run` / `resume_run` 都以条件更新为准：并发两次唤醒只有一次命中。（`suspend_run` 以 `status='RUNNING'` 为前提，`with_resume_run` 以 `pending_tool_call_id` 一致为前提，0 行即回滚）
  - [x] 写入失败不影响已成功 Run 的结论（代码路径可核：终态审计失败只 `warn!`）。（`agent-runtime/src/loop_/tool.rs` 的 `audit()` 逐条 `record_tool_call`，失败只 `warn!("工具审计写入失败")` 且返回 `()`；三个调用点（`tool.rs` 的 112 / 153 / 179 行）都不消费返回值，因此终态审计写库失败不改变 Run 结论——`append()` 的「写不进去即本轮失败」只作用于每轮轨迹，两者互不影响）
  **测试要求**：无。

- [x] **P3-019 审计查询接口、`Message` 契约扩展与接口文档**

  **目标**：审计可查、工具轨迹随 `message/list` 可见；接口契约只有一份来源。
  **前置依赖**：P3-018。
  **涉及文件 / Crate**：`crates/lib-web/src/tool/{api.rs,mod.rs}`、`crates/lib-web/src/lib.rs`、`crates/agent-core/src/session/dto.rs`（`Message` 三个可选字段）、`crates/agent-tool/src/audit.rs`（`ToolCallVO`）、`docs/api/{tool.md,session.md,readme.md}`。
  **实施步骤**：
  1. 契约类型 `ToolAuditListParams { run_id }`（查询参数）与 `ToolCallVO`（`auto_type`，含 `fileAccess` / `status`，`i64` 走字符串）。
  2. `GET api/tool/audit/list`：`Query(po)` → `manager_agent` 转 `SessionService::tool_calls_of_run`；错误映射沿用 `SessionError → WebError`。
  3. `Message` 追加可选字段 `tool_call_id` / `tool_name` / `tool_calls`（`tool_calls` 用 JSON 字符串还是结构体？**用结构体**：`tool_calls: Option<Vec<LlmToolCallVO>>`，`auto_type` 生成）。
  4. `docs/api/tool.md` 新建并登记到 `docs/api/readme.md`；`docs/api/session.md` 的 `Message` 段落同步字段。
  5. 接口总数从 25 变 26（不含 debug + `metrics` 限定的 `api/probe/metrics`）；`client/update → client/post` 的重命名在 P3-026 单独处理，本任务不动。
  **验收标准**：
  - [x] `make gen` 产出的两份 SDK 都包含新接口、`ToolCallVO` 与 `Message` 的新字段。
  - [x] 路径命名符合规范：`api/tool/audit/list`（资源段小写单数、动作段用 `list`）。
  - [x] 未知 `runId` 返回 `[]` 而不是 404；`runId` 缺失返回 400。（Handler 不做存在性校验，`tool_calls` 查询无记录即空列表；缺参数走框架的查询参数转换）
  - [x] `message/list` 返回的历史里，TOOL 消息带 `toolCallId`，assistant 消息带 `toolCalls`。（`tool.loop`：assistant 的 `toolCalls[0].id` 与事件同源，TOOL 消息带同一 `toolCallId` 与 `toolName`，且 assistant 不晚于它的结果）
  **测试要求**：无（`tool.audit` 覆盖，**需授权**）。

- [x] **P3-020 SDK 重新生成与前端类型跟进**（`RunEvent` 新字段部分随 P3-013 再生成一次）

  **目标**：契约变更走完 `make gen` 全链路，生成物不入库。
  **前置依赖**：P3-013、P3-019。
  **涉及文件 / Crate**：`packages/sdk-rust`、`packages/sdk-ts`（生成物，不入库）、`lingting-agent-ui/src`（类型消费）。
  **实施步骤**：
  1. `make gen` → `make gen-check`（清空重建 + 产物校验 + 生成代码参与编译）。
  2. 前端 `make ui-install` / `make ui-check` 通过；`useChat.ts` 里新增的三个事件分支先按「忽略但类型完整」处理（展示在 P3-028 / P3-029）。
  3. 确认生成物没有入库（`git status` 干净）。
  **验收标准**：
  - [x] `make gen` / `make gen-check` / `make ui-check` 均退出码 0。
  - [x] 生成物不在 `git status` 里。
  - [x] TS 侧 `RunEventKind` 含三个新取值，`RunEvent` 含六个新字段，`Message` 含三个新字段。（P3-013 落地后重新生成：`packages/sdk-ts/dist/types.d.ts` 里三个事件取值与六个事件字段、`Message` 的三个字段均在；`make gen-check` / `make ui-check` 通过）
  **测试要求**：无（门禁命令本身就是静态检查）。

---

## Batch F：五个内置工具（P3-021 ~ P3-025）

> 依赖：Batch A / Batch B 的 trait 与执行器。可与 C / D 并行。每个工具只实现 trait，不新增机制。
>
> **状态**：P3-021 ~ P3-025 完成（代码 + 静态核对 + 全量回归；`cargo check` / `clippy -D warnings` / `fmt --check` 在 workspace 全绿，装配后 `make test ARGS="--all"` 75/75 全部通过，含 `web.e2e` 的 6 个浏览器用例与 `stress.boundary`）。**装配口径**：五个工具由 `agent_tool::builtin::register_all` 一次性注册，`[tools.file]` / `[tools.http]` 经 `BuiltinOptions` 传入；装配点是 `manager-agent::assemble::build_tools`（该函数改为返回 `Result`，注册失败即启动失败）。`http_request` 是 `Network` 风险，默认 `deny`，不显式 `allow` 就不进模型可见清单。全量回归证明「五个工具进装配」不改变既有行为（会话 / Run / 持久化 / TUI / WebUI 全部照旧）；工具自身的边界断言（`tool.builtin.*`）仍待 Batch H 的 P3-030 落地，本节判据以静态核对取证。

- [x] **P3-021 `calculator`**

  **目标**：安全的四则运算工具，不引表达式求值库。
  **前置依赖**：P3-002、P3-003。
  **涉及文件 / Crate**：`crates/agent-tool/src/builtin/{calculator.rs,mod.rs}`。
  **实施步骤**：
  1. `CalculatorArgs { expression: String }`（`schemars` 派生）；`file_access()` 返回 `ToolFileAccess::None`（不操作本地文件）。
  2. 自研递归下降：`expr := term (('+'|'-') term)*`、`term := factor (('*'|'/'|'%') factor)*`、`factor := ['-'|'+'] (number | '(' expr ')')`；只接受 ASCII 数字、`.`、运算符与括号；除零与语法错误返回 `ToolError::Failed` 的固定文案。
  3. 结果格式：整数值输出整数形式，否则保留最多 10 位小数（去掉尾随零）。
  4. 输入长度上限 512 字符（防止病态嵌套），嵌套深度上限 32。
  **验收标准**：
  - [x] 不引入新的第三方依赖（`Cargo.toml` 可核）。（本 Crate 新增的 `url` / `chrono` / `anyhow` 都取自 `workspace.dependencies`，没有引入 workspace 之外的新第三方 crate）
  - [x] 非法输入（空串、`1+`、`(1+2`、`1/0`、超出长度的表达式）返回固定错误文案，不 panic。（**实现口径**：空串 / 超长 / 含不支持字符 → `InvalidArguments`，文案 `expression 不能为空` / `expression 长度不能超过 512 字符` / `expression 只支持数字、. 、+ - * / % 与括号`；`1+` / `(1+2` → `Failed(表达式语法错误)`；`1/0` → `Failed(除数不能为零)`；嵌套 > 32 层与结果溢出各有固定文案。解析器只前进、不回溯、不切片越界，没有 `unwrap` 路径）
  - [x] `2+3*4` = `14`、`(2+3)*4` = `20`、`-3%2` = `-1`（取模符号与被除数一致）。（按文法静态核对：`term` 比 `expr` 结合更紧、括号分支回到 `expr`、`%` 用 Rust 的「符号随被除数」语义；**未执行用例**）
  **测试要求**：无（`tool.builtin.calculator` 覆盖，**需授权**）。

- [x] **P3-022 `current_time`**

  **目标**：只读系统时钟，输出可被模型直接使用的两种格式。
  **前置依赖**：P3-002、P3-003。
  **涉及文件 / Crate**：`crates/agent-tool/src/builtin/current_time.rs`。
  **实施步骤**：
  1. `CurrentTimeArgs { zone: Option<String> }`，取值 `utc` / `local`，缺省 `utc`；`file_access()` 返回 `ToolFileAccess::None`（只读系统时钟，不碰文件）；其它取值返回 `InvalidArguments`（不做时区名解析）。
  2. 输出：`<ISO8601>（Unix 毫秒：<ms>，时区：<utc|local>）`；毫秒用 `lib_core::time::now_millis`，格式化用 `chrono`（工作区已有依赖）。
  **验收标准**：
  - [x] 不引入新依赖（`chrono` 已在 workspace 依赖里）。
  - [x] `zone = "Asia/Shanghai"` 返回 `InvalidArguments` 且文案点明只支持 `utc` / `local`。（Display 为 `参数不合法：zone 只支持 utc 或 local`；取值先 `to_ascii_lowercase`，`UTC` 与 `utc` 等价）
  - [x] 输出包含 ISO8601 与 Unix 毫秒两段，且毫秒与 `created_at_ms` 同源。（同一条 `lib_core::time::now_millis` 既用于 `DateTime::from_timestamp_millis` 也写进文本；ISO8601 走 `to_rfc3339_opts(SecondsFormat::Millis, …)`，`local` 保留偏移量、不换成 `Z`）
  **测试要求**：无（`tool.builtin.current_time` 覆盖，**需授权**）。

- [x] **P3-023 `file_read`**

  **目标**：在白名单内读取文本文件，越界即拒绝。
  **前置依赖**：P3-002、P3-003、P3-005（`[tools.file]`）。
  **涉及文件 / Crate**：`crates/agent-tool/src/builtin/{file_read.rs,path.rs}`。
  **实施步骤**：
  1. `FileReadArgs { path: String }`；`file_access()` 返回 `ToolFileAccess::Read`。
  2. `path::resolve(home, allow_dirs, requested) -> Result<PathBuf, ToolError>`：相对路径基于 `home`；`canonicalize` 后必须以某个白名单根为前缀（`Path::starts_with`，按组件比较而非字符串前缀）；白名单为空时只允许 `home`。拒绝 `..` 逃逸与不存在路径（统一文案，不暴露真实绝对路径）。
  3. 大小上限 `[tools.file] max_bytes`（默认 1 MiB）：先 `metadata().len()` 判断，超限返回固定文案（不读入内存）。
  4. 非 UTF-8 → 拒绝并说明「二进制文件不支持」。
  5. 结果文本带相对路径头部（相对白名单根），正文原样。
  **验收标准**：
  - [x] 白名单外的路径被拒绝，且错误文本里不含真实绝对路径。（越界与不存在共用 `path::REJECTED_TEXT`「路径不在允许的范围内或不存在」；`Resolved` 的规范化绝对路径只在工具内部使用，结果文本只写相对白名单根的路径）
  - [x] 超过 `max_bytes` 的文件不被完整读入（代码用 `metadata` 预判，可核）。（`std::fs::read` 只在长度检查之后调用，超限直接返回固定文案）
  - [x] 二进制文件返回固定文案而不是乱码。（`String::from_utf8` 失败 → `Failed(二进制文件不支持读取)`）
  **补充口径**：结果头部为 `文件：<相对白名单根路径>`；路径存在但不是普通文件（目录 / 设备）时返回 `Failed(该路径不是普通文件)`。
  **测试要求**：无（`tool.builtin.file` 覆盖，**需授权**）。

- [x] **P3-024 `file_search`**

  **目标**：在白名单内按文件名搜索，遍历期剪枝无关目录。
  **前置依赖**：P3-023（复用 `path::resolve`）。
  **涉及文件 / Crate**：`crates/agent-tool/src/builtin/{file_search.rs,path.rs}`。
  **实施步骤**：
  1. `FileSearchArgs { name: String, path: Option<String>, max_results: Option<i64> }`；`file_access()` 返回 `ToolFileAccess::Read`；`name` 为文件名子串（忽略 ASCII 大小写），不做正则与 glob（口径写进工具描述）。
  2. 遍历：`std::fs::read_dir` 的深度优先，**遍历期剪枝**固定排除目录（`target` / `node_modules` / `.git` / `.venv` / `venv` / `__pycache__` / `dist` / `build` / `out` / `.next` / `.cache` / `coverage` / `vendor` / `logs` / `tmp` / `.gradle`）；深度上限 8；结果条数上限 `max_results`（默认 50，硬上限 200）。
  3. 结果按路径字典序排序，每行「相对路径 + 大小」；到达上限即停止遍历并在文本末尾标注「已截断」。
  4. 跳过符号链接目录（不跟随），避免环。
  **验收标准**：
  - [x] 排除目录在**遍历阶段**剪枝（代码结构可核：命中即 `continue`，不做事后过滤）。（`EXCLUDED_DIRS.contains(&name)` 在入栈前判定，排除目录的 `read_dir` 一次都不会发生；符号链接同样不入栈）
  - [x] 结果条数与顺序稳定（同一目录两次执行结果相同）。（每个目录的条目先按 `file_name` 排序再匹配/入栈，最终结果再按路径字典序排序，因此「谁占满上限」也确定）
  - [x] 起点路径越界时与 `file_read` 同样被拒绝。（起点走同一个 `path::resolve`；起点不是目录时返回 `Failed(搜索起点不是目录)`）
  **补充口径**：`FileSearch::new` 只接白名单——本工具不读文件内容，不需要 `max_bytes`；`max_results <= 0` 与空 `name` 都是 `InvalidArguments`。
  **测试要求**：无（`tool.builtin.file` 覆盖，**需授权**）。

- [x] **P3-025 `http_request`**

  **目标**：受域名白名单约束的 HTTP 调用，默认拒绝。
  **前置依赖**：P3-002、P3-003、P3-005（`[tools.http]`）。
  **涉及文件 / Crate**：`crates/agent-tool/src/builtin/http_request.rs`、`crates/agent-tool/Cargo.toml`（`framework-http` 的 `reqwest` 特性）。
  **实施步骤**：
  1. `HttpRequestArgs { url: String, method: Option<String>, body: Option<String> }`，`method` 只接受 `GET` / `POST`（缺省 `GET`）；`file_access()` 返回 `ToolFileAccess::None`（只访问网络，不碰本地文件）。
  2. 校验：`url::Url` 解析（`url` 已在 workspace 依赖）；scheme 只允许 http / https；host 必须命中 `[tools.http] allow_hosts`（`host` 或 `host:port` 两者都可写）；白名单为空即拒绝。
  3. 请求：`framework-http::HttpClient`（`no_redirect()` + 超时）；响应体读取上限 `[tools.http] max_bytes`（默认 256 KiB），超限即截断并标注。
  4. 结果文本：状态码 + 内容类型 + 正文；只回灌正文与状态，不回灌请求头（避免把 `Authorization` 之类带进模型上下文）。
  5. `retryable`：仅对「连接失败 / 读超时」返回真，`max_retries = 0` 时仍不重试（D-P3-008）。
  **验收标准**：
  - [x] 默认配置下该工具为 deny：既不出现在模型可见清单（不进入上下文），执行期也返回 `未授权`。（风险等级 `Network` → `ToolRisk::default_permission()` 为 `Deny`；注册表把 deny 的工具从 `model_definitions()` 摘掉、`lookup` 仍命中，执行期由执行器回 `DENIED` 并落 `工具未授权：http_request`）
  - [x] 白名单之外的主机被拒绝时不发起连接（可用 127.0.0.1 之外的不可达地址验证「没有网络动作」）。（host 判定在 `HttpClient` 构造与任何请求之前返回 `Failed(请求的主机不在允许的范围内)`）
  - [x] 不跟随重定向（3xx 原样返回状态码）。（`HttpClientBuilder::no_redirect()` → `redirect_limit(0)`）
  - [x] 响应超限被截断并标注，不把整个响应读进内存。（`stream_get` / `stream_post` 逐块读，累积到 `max_bytes` 即停止读并追加 `…（响应超过 N 字节，已截断）`）
  **补充口径**：客户端按 `no_redirect()` 新建、**不设客户端超时**——超时只有一个拥有者（执行器的 `[tools] timeout_ms`），两处同刻倒计时会让 `TIMEOUT` 状态取不到（P3-030 修正）；因此每次调用一个新连接池。`retryable` 按错误链里的 `std::io::ErrorKind`（`TimedOut` / `ConnectionRefused` / `ConnectionReset` / …）判定，`reqwest` 类型不出框架（D-P3-018）；失败文案不带 URL 与底层错误原文。
  **测试要求**：无（`tool.builtin.http` 覆盖，**需授权**；用例本地起一个只监听 127.0.0.1 的假服务端）。

---

## Batch G：确认交互（接口改名、挂起唤醒与客户端）（P3-026 ~ P3-029）

> 依赖：Batch D（挂起点与恢复）+ Batch E（挂起列与 Service）。本批次是 G-7 的出口。

- [x] **P3-026 `api/client/update` → `api/client/post`**

  **目标**：上行通道一次改到底，同时承载心跳与工具确认（D-P3-022）。
  **前置依赖**：P3-018。
  **涉及文件 / Crate**：`crates/types-core/src/runtime/client.rs`、`crates/lib-web/src/runtime/api.rs`、`crates/manager-agent/src/runtime/{registry.rs,client.rs,confirm.rs}`、`crates/lib-bin-cli/src/run.rs`、`crates/lib-bin-tui/src/worker.rs`、`lingting-agent-ui/src/api/link.ts`、`docs/api/{runtime.md,readme.md}`、`docs/develop.md`。
  **实施步骤**：
  1. DTO 改名：`ClientUpdatePO/VO/Kind` → `ClientPostPO/VO/Kind`；`ClientPostKind` 追加 `ToolConfirm`；`ClientPostPO` 追加可空 `tool_call_id` / `decision`（`ClientDecision { Approve, Reject }`）。
  2. handler：`POST api/client/post`，字段组合校验（`PING` / `INQUIRY_ACK` 不得带确认字段；`TOOL_CONFIRM` 必须两个都带，缺一 400 固定文案）。
  3. `manager-agent`：`heartbeat(kind)` 拆成 `refresh_heartbeat(at_ms)` 与 `submit_confirm(client_id, tool_call_id, decision)`，按 `kind` 分派；确认时同时刷新 `lastSeen` 并返回 pong。
  4. 客户端调用点改名（CLI `run`、TUI `worker`、WebUI `link.ts`）；不保留旧方法名。
  5. 文档：`docs/api/runtime.md`（接口清单、参数表、`ClientPostKind` 取值表、样例）、`docs/api/readme.md` 的固定 message 表、`docs/develop.md` 的「运行时接口」段落。
  **验收标准**：
  - [x] `grep -rn "client/update\|ClientUpdate" crates lingting-agent-ui docs` 为空（历史决策文档除外）。（只剩 `docs/reports/` 的两份历史报告与 `docs/api/runtime.md` 的一处改名说明；`packages/*` 为生成物，已由 `make gen` 刷新）
  - [x] 四条 runtime 接口仍是 4 条（`create` / `stream` / `post` / `delete`），路径列表逐条对照。（`make gen` 输出里 runtime 领域仍是 4 条路由）
  - [x] 字段组合校验的四种情形各有固定 400 文案（缺 `toolCallId`、缺 `decision`、`PING` 带确认字段、未知 `kind`）。（前三种是 `runtime/mod.rs` 的三个常量 + `invalid_fields`；未知 `kind` 由 serde 反序列化失败走框架固定文案 `请求体不是有效的 JSON`，已写进 `docs/api/runtime.md` 的错误表）
  - [x] `make gen` / `make ui-check` 通过；生成物不入库。（两条命令退出码 0）
  **补充口径**：`kind` 的分派在 handler（`client_post`），`manager-agent` 只暴露 `refresh_heartbeat` / `answer_inquiry` / `submit_confirm` 三个语义入口；确认链路的编排放在新模块 `runtime/confirm.rs`，`registry.rs` 保持「只做内存记账」。回归取证：`make test ARGS="--all"` 75 个用例全部通过，其中 `runtime.contract` / `runtime.heartbeat` / `runtime.inquiry` / `runtime.unregister` / `run.disconnect` 等已全部改走 `client/post`（PING / INQUIRY_ACK 行为与改名前等价）；前端 `tests/link.test.ts` 同步改名为 `clientPost` 后 `web.ui` 通过。
  **测试要求**：无（`client.*` 用例改名与覆盖在 P3-030，**需授权**）。

- [x] **P3-027 服务端挂起与唤醒链路**（代码在 Batch G 落地，判据在 Batch H 的 `tool.confirm.*` 取证）

  **目标**：确认事件下发 → Run 挂起 → 客户端决定 → 重新排队执行，全链路可用，含重连补发（D-P3-023）。
  **前置依赖**：P3-014、P3-018、P3-026。
  **涉及文件 / Crate**：`crates/manager-agent/src/{runner/queue.rs,runtime/session.rs,runtime/registry.rs,holder.rs}`、`crates/lib-web/src/runtime/{api.rs,stream.rs}`、`crates/agent-runtime/src/loop_.rs`（配合）。
  **实施步骤**：
  1. 确认请求的鉴权：`clientId` 必须已注册且是当前占用该会话的客户端；`toolCallId` 必须等于该 Run 的 `pending_tool_call_id`；不满足即 400 固定文案（与 P1.1 的固定 message 同风格）。
  2. 决定落库：`resume_run`（P3-018）清待确认列并写决定；驳回时同事务把审计记录置 `DENIED`。
  3. 重新排队：唤醒会话队列（`RunExecutor::notify`），Run 已回 `QUEUED`，由队列按 `created_at_ms` 重新取；唤醒后 `drive` 走恢复路径。
  4. 重连补发：`api/client/stream` 建立后，对该客户端占用会话下所有 `pending_tool_call_id` 非空的 Run 各补发一次 `tool_call_confirm_required`（走同一条事件生产路径，`seq` 连续）。
  5. 取消路径联动：取消 / 释放占用 / 断连 / 服务关闭时清 `pending_tool_call_id`，`PENDING` 审计收尾为 `CANCELLED`。
  6. 不占用并发位：挂起时 `drive` 已返回，`Semaphore` 许可已释放（P3-014 保证）；本任务用 `metrics` 快照核对。
  **验收标准**：
  - [x] 挂起后 `runs` 行为 `status=WAITING`（D-P3-027；`pending_tool_call_id` 非空，用例以 `tool_call_confirm_required` 的 `toolCallId` 与审计里的 `PENDING` 记录为准）；`running_now` 不含它（`tool.confirm.approve` 取快照核到 `running_now = 0`）。
  - [x] 同一会话后续 Run 能在挂起期间执行（`tool.confirm.approve`：`max_concurrent_runs = 1` 下第二个 Run 仍 `COMPLETED`，且只多一次模型请求）。
  - [x] 批准后 Run 重新执行并 `COMPLETED`，工具确实被执行（`tool.confirm.approve`：假服务端命中 1 次、审计转 `OK`、请求总数 3 说明首轮未重跑）。
  - [x] 驳回后 Run `COMPLETED` 且模型收到「用户拒绝调用该工具」，审计 `status=DENIED`（`tool.confirm.reject`）。
  - [x] 断连重连后收到补发的确认事件（同一 `toolCallId`），再次确认成功（`tool.confirm.reconnect`：建流前先挂起，建流后收到补发并批准成功）。
  - [x] 重复确认 / 过期 `toolCallId` 返回 400 固定文案，不产生第二条审计记录（`tool.confirm.approve` 重复确认 → 400 `待确认的工具调用不存在或已处理`，审计仍 1 条；`tool.confirm.disconnect` 断连后 → 400 `客户端未注册，请重新连接`）。
  **补充口径**：链路落在 `manager-agent/src/runtime/confirm.rs` 与 `lib-web/src/runtime/api.rs`——确认鉴权三层（已注册 → 占用该会话 → `tool_call_id` 命中挂起列）、`resume_run` 条件更新、`notify_run` 唤醒、`api/client/stream` 建立后补发确认事件；「清挂起列 + `PENDING` 审计收尾为 `CANCELLED`」放在 `Repository::{cancel_run,cancel_session_runs}` 的同一事务里。回归取证：`make test ARGS="--all"` 75 个用例全部通过（`run.cancel` / `run.disconnect` / `run.shutdown` / `persist.*` 覆盖了取消链路的既有语义，确认链路的改动没有破坏它们）。判据已由 Batch H（P3-030）取证：`tool.confirm.approve` / `.reject` / `.reconnect` / `.disconnect` 四条用例覆盖 F-2 / F-4 ~ F-7。挂起态口径按 D-P3-027 记为 `WAITING`。已知口径缺口：补发只在「同一 `clientId` 重新建立下行流」时有效——完整断连会按 D-P3-023 把该会话的 Run 收尾为 `CANCELLED`，`tool.confirm.reconnect` 用例即按「不断连、先挂起、再建流」构造。
  **测试要求**：无（`tool.confirm.*` 覆盖，**需授权**）。

- [x] **P3-028 TUI：工具事件与确认交互**（代码在 Batch G 落地，判据在 Batch H 的 `tool.tui` 取证）

  **目标**：TUI 里能看到工具调用状态、能对确认请求应答（D-P3-026）。
  **前置依赖**：P3-013、P3-020、P3-026。
  **涉及文件 / Crate**：`crates/lib-bin-tui/src/{event.rs,worker.rs,ui.rs,state.rs}`。
  **实施步骤**：
  1. `Update` 增加 `ToolStarted { name }` / `ToolFinished { name, status, duration_ms }` / `ToolConfirmRequired { tool_call_id, name, arguments }`。
  2. `worker.rs` 的 `RunEventKind` 分支补三个新取值（穷尽匹配强制）；确认上行走 `api/client/post { kind = TOOL_CONFIRM, ... }`。
  3. `ui.rs`：工具调用一行一条（`→ calculator` / `✓ calculator（12ms）` / `✗ http_request：工具未授权`）；确认提示单独一行 `⚠ 需要确认：http_request(…) [y] 允许 / [n] 拒绝`。
  4. `state.rs`：待确认项随 Run 绑定；`y` / `n` 只在有 pending 时生效；发出后进入「已提交，等待结果」态，避免重复上行。
  **验收标准**：
  - [x] `cargo clippy -p lib-bin-tui --all-targets -- -D warnings` 通过。（无告警）
  - [x] 三个事件分支都有对应 `Update`，无 `_ => {}` 兜底。（`dispatch_run_event` 逐个 `RunEventKind` 匹配；`Heartbeat` 是显式空分支，不是兜底）
  - [x] 没有 pending 时 `y` / `n` 不产生任何上行请求（代码可核 + 用例）。（`App::handle_key` 只在 `pending.is_some()` 时把 `y` / `n` 当应答键；用例部分未授权，待 `tool.tui`）
  - [x] 工具行不打断既有增量文本的拼接顺序；样式沿用既有主题，不新增颜色常量。（工具行作为 `MessageRole::Tool` 追加在消息区末尾，增量仍写在助手占位消息里；颜色只用既有的 `Green` / `Red` / `Yellow` / `DarkGray` / `Blue`）
  **补充口径**：确认态落在 `App::pending`（`PendingConfirm { tool_call_id, summary, submitted }`），确认行与结果行都以 `MessageRole::Tool` 消息承载并按 `toolCallId` 原地改写（一次调用只占一行）；上行失败时 `Notice` 分支把这一行恢复成可应答（`reset_confirm`），用户不必重连。回归取证：`make test ARGS="--all"` 中 `tui.*` 全部通过（`tui.stream` / `tui.send` / `tui.interrupted` / `tui.reconnect` 等既有交互未被工具行打断）；`tool.tui`（Batch H）逐条断言三个事件转 `Update`、`y` / `n` 各产生一条 `Confirm`、已提交后重复按键不上行。
  **测试要求**：无（`tool.tui` 覆盖，**需授权**）。

- [x] **P3-029 WebUI：工具卡片与确认按钮**（代码 + `tool.web` 用例 + `tool.web.e2e` 的浏览器侧验收与截图目测）

  **目标**：WebUI 对话流里能看到工具调用与确认请求（D-P3-026）。
  **前置依赖**：P3-013、P3-020、P3-026。
  **涉及文件 / Crate**：`lingting-agent-ui/src/pages/chat/useChat.ts`、`lingting-agent-ui/src/api/link.ts`、相关组件与类型。
  **实施步骤**：
  1. `useChat.ts` 的 `switch (event.event)` 增加 `tool_call_started` / `tool_call_completed` / `tool_call_confirm_required` 三个分支，按 `toolCallId` 配对；未配对的 started 在 Run 结束时标「未完成」。
  2. 组件：紧凑工具卡片（工具名 / 状态 / 耗时），确认卡片（工具名 + 脱敏参数摘要 + 「允许」「拒绝」两个按钮）。
  3. 按钮点击 → `clientPost({ kind: "TOOL_CONFIRM", toolCallId, decision })`；点击后进入不可重入的等待态。
  4. 复用既有主题与间距，不新增设计 token。
  **验收标准**：
  - [x] `make ui-check` 退出码 0；三个事件分支都有类型收窄，无 `any`。（`make ui-check` 退出码 0；三个分支在 `applyToolEvent` 里按 `event.event` 收窄，`ChatItem` 判别式联合无 `any`）
  - [x] 确认按钮点击后不可重复点击（状态机可核）。（`confirmingRef` 屏蔽请求在途的重复点击；点击即置 `submitted`，按钮 `disabled={!confirmable}`，只有上行失败才退回 `confirm`）
  - [x] 卡片与既有消息气泡的视觉层级一致（沿用现有间距与字号）。（`tool.web.e2e`：真实 Chromium + 真实服务端（`[tools.permissions] calculator = "confirm"`）走完「待确认 → 允许 → 完成」，断言卡片正文与用户气泡的 `font-size` 相同、卡片 `max-width` 为 `560px`；截图 `lingting-agent-ui/tests/.artifacts/tool-card-confirm.png` 与 `tool-card-done.png` 人工目测：卡片坐在 `.ant-bubble-start` 同一列、与「Mock 回复」气泡同字号同左边距，状态标签走 antd 语义预设类（待确认 `ant-tag-warning`、完成 `ant-tag-success`，色值由主题给出，本项目 `colorSuccess` 是琥珀色系而非 antd 默认绿），两个按钮为 antd `small` 尺寸，未新增设计 token）
  **补充口径**：对话流改成一个判别式联合（`ChatItem = ChatMessage | ToolCard`），按到达顺序同列渲染，工具卡片的位置因此等于它真正发生的位置；`useChat` 仍导出只含消息气泡的 `messages`（既有用例与调用点不变），新组件 `ToolCard.tsx` 承担卡片与两个按钮。回归取证：`make test ARGS="--all"` 中 `web.ui` 前端用例全部通过（`useChat` / `MessageList` / `ChatLink` 的既有行为未变）。工具事件的客户端展示由两层取证：`tool.web`（jsdom，`tests/tool-events.test.tsx` 覆盖三个事件分支、确认不可重复点击与未完成标记）与 `tool.web.e2e`（真实 Chromium，见上一条判据；两个状态的截图落在 `lingting-agent-ui/tests/.artifacts/` 供人工复看）。
  **测试要求**：无（`tool.web` / `tool.web.e2e` 覆盖，已授权执行）。

---

## Batch H：用例、文档与阶段验收（P3-030 ~ P3-032）

> 依赖：A ~ G 全部批次。

- [x] **P3-030 `bin-tests` 的 `tool.*` / `client.*` 用例与 Mock 扩展**（27 个 `tool.*` + `client.post`，`make test ARGS="--all"` 103 个用例全部通过）

  **目标**：七条验收方向各有可执行的证据。
  **前置依赖**：P3-014 ~ P3-029。
  **涉及文件 / Crate**：`crates/bin-tests/src/mock/llm/{mod.rs,reply.rs,handler.rs,tools.rs}`、`crates/bin-tests/src/cases/tool/{mod.rs,loop_.rs,executor.rs,context.rs,builtin.rs,confirm.rs,config.rs,client.rs,events.rs}`、`crates/bin-tests/src/cases/{mod.rs,persist.rs}`、`crates/bin-tests/src/util/frontend.rs`、`crates/lib-bin-tui/src/testing.rs`、`lingting-agent-ui/tests/tool-events.test.tsx`、`lingting-agent-ui/tests/e2e/tool.spec.ts`、`lingting-agent-ui/playwright.config.ts`。（`client.post` 落在 `cases/tool/client.rs`，原有 `client.errors` 仍留在 `cases/client.rs`；`tool.events` / `tool.tui` / `tool.web` / `tool.web.e2e` 集中在 `cases/tool/events.rs`；「`0001` 时代」夹具落在 `cases/persist.rs` 的 `persist.migration`）
  **实施步骤**：
  1. Mock 扩展：请求的最后一条消息含 `tool:{…}` 关键字时按脚本返回 `tool_calls`（流式帧，支持参数分片 `fragments`、缺结束帧 `missingEnd`、每轮重复 `repeat`）；本轮脚本所在的用户消息之后一旦出现 assistant `tool_calls`，后续轮改回纯文本（多轮会话里每轮各带自己的脚本，因此不看整段历史）。`/mock/last-request` 返回完整请求体，`tools` 与 `messages` 都在里面，用例据此断言清单与多轮历史。
  2. 用例清单（全部 `SelfServed`，私有服务端 + 私有 Mock）：
     - `tool.schema`：工具清单按名字稳定排序；`deny` 的工具不在清单里、`confirm` 的在；请求里不出现 `fileAccess`；「必填 / 可选 / 枚举 / 数字」四类字段的 schema 输出与 serde 的接受 / 拒绝逐类对照。
     - `tool.loop`：calculator 两轮闭环，事件序列含 `tool_call_started` / `tool_call_completed` / `run_completed`，`seq` 连续；`message/list` 里能看到中间轮的 assistant `toolCalls` 与 TOOL 消息。
     - `tool.context`：工具组整体保留 / 整体丢弃（把预算压到只够一组），不出现孤儿 TOOL 或缺结果的 tool_calls。
     - `tool.parallel`：一轮两个 calculator 调用，结果顺序与调用顺序一致；`max_parallel_calls = 1` 时串行。
     - `tool.stream`：Rig 流式工具调用的聚合——分片 `Arguments` 拼成完整 JSON、缺 `End` 时落 `FAILED`（不猜参数）、纯文本流与 P2 行为一致。
     - `tool.invalid_args` / `tool.timeout` / `tool.failed` / `tool.denied`：四种失败都回灌固定文案，Run 最终 `COMPLETED`。
     - `tool.rounds`：`max_tool_rounds = 1` 时第二轮请求即落 `FAILED`（固定文案）。
     - `tool.cancel`：用例内起慢响应 HTTP 服务 + `http_request`，取消后断言连接被关闭、无后续事件、`tool_call_completed` 为 `CANCELLED`。
     - `tool.confirm.approve` / `tool.confirm.reject`：挂起后 Run 回 `QUEUED`、`pendingToolCallId` 非空、`running_now` 不含它；同会话后续 Run 可执行；`api/client/post` 决定后重新排队并 `COMPLETED`。
     - `tool.confirm.reconnect`：断连重连后收到补发的确认事件（同一 `toolCallId`）。
     - `tool.confirm.disconnect`：挂起中断连 → Run `CANCELLED`、`pending_tool_call_id` 清空、审计 `CANCELLED`。
     - `tool.audit`：按 Run 查 `api/tool/audit/list`，条数、状态、`durationMs`、`fileAccess` 与事件一致；参数里的 `api_key` 已脱敏。
     - `tool.truncate`：`max_result_chars` 调小后结果被截断并带提示。
     - `tool.builtin.*`：五个工具的边界（白名单外拒绝、除零、二进制、排除目录剪枝、默认 deny）。
     - `tool.config`：非法 `[tools]` 配置启动失败且文案点名字段。
     - `tool.decl`：五个内置工具的 `fileAccess` = `None` / `None` / `Read` / `Read` / `None`；`default_concurrency()` 四条映射逐项正确。
     - `tool.events` / `tool.tui` / `tool.web`：客户端侧展示与确认的状态机（事件消费函数的可调用性，不做界面快照）。
     - `tool.web.e2e`：真实 Chromium（Playwright 的 `tool` 项目）里走完「待确认 → 允许 → 完成」，断言卡片与气泡同字号 / 卡片宽度上限 / 状态标签的 antd 语义类，并把两个状态的截图留作人工目测。
     - `client.post`：改名前后的行为等价（`PING` / `INQUIRY_ACK`）+ 四种确认字段组合校验。
  3. 所有断言的数据来源：客户端收到的 `RunEvent`、接口响应、`message/list`、`metrics` 快照；**不读日志**。
  **验收标准**：
  - [x] `make test ARGS="--list"` 里能查到上述全部用例。（27 个 `tool.*` + `client.post` 全部在列）
  - [x] 每个用例失败时的信息点明「哪条断言、期望与实际」，不依赖日志排查。（首轮 6 条失败的原因都直接给出断言文案与实测值，例如「保留下来的应是第 2 轮与第 3 轮的结果：[\"2\"]」）
  - [x] 用例不占用共享服务端（`SelfServed`），不修改全局配置。（有服务端交互的用例一律 `SelfServed` 自起私有服务端与私有 Mock；`tool.builtin.*` / `tool.decl` / `tool.config` / `tool.tui` 走进程内断言，不碰服务端；`client.post` 用 `Isolated` 以免污染共享注册表）
  - [x] 全仓检索 `read_to_string(` 在 `crates/bin-tests` 中与日志无关（不新增日志读取）。（命中项只有 PID 文件、事件文件与源码读取；`cases/tool/` 下 0 处）
  **测试要求**：**需要授权后执行**；未授权时只落地用例代码与清单，判据保持未勾选。（本轮已授权：`make test ARGS="--all"` 103 个用例全部通过，报告 `target/tests/run-1795599811697-44180/README.md`；`web.e2e` 只跑 `chat` / `shutdown` 两个 Playwright 项目，`tool` 项目由 `tool.web.e2e` 单独跑并各自指定 `--output`，避免并发时产物互相覆盖）

- [x] **P3-031 文档同步与已知限制落稿**

  **目标**：接口、结构与纪律文档与实现一致，已知限制有出处。
  **前置依赖**：P3-026、P3-027、P3-030。
  **涉及文件 / Crate**：`docs/api/{readme.md,runtime.md,session.md,tool.md}`、`docs/develop.md`（运行时接口与会话业务接口两段）、`docs/workspace.md`（`agent-tool` 落地状态与 `service-core → agent-tool` 依赖）。
  **实施步骤**：
  1. 核对四份 `docs/api/*`：接口清单、参数表、载荷字段、固定 message 表、接口总数（26，不含 `api/probe/metrics`）。
  2. `docs/develop.md`：运行时接口段落改为 `api/client/post`；会话业务接口段落补「工具轨迹进历史 + 每轮边界落库 + 挂起语义」三条变化。
  3. `docs/workspace.md`：`agent-tool` 已落地、`service-core` 的新依赖、`tool_calls` / `messages` / `runs` 的三处 schema 变化在对应 Crate 段落下点一句。
  4. 只在用户明确要求的范围内同步，其余同步建议列入报告。
  **验收标准**：
  - [x] `docs/api/readme.md` 的接口总数、领域索引与实际路由一致（逐条对照 `grep -rn "path = \"api/"`）。（路由 27 条；去掉仅 `debug` + `metrics` 注册的 `api/probe/metrics` 为 26 条，与 `readme.md` 新增的「当前接口总数 26 条」及领域计数逐条对上）
  - [x] 文档里不出现 `client/update`。（现行文档 `docs/api` / `docs/develop.md` / `docs/workspace.md` 0 命中；`docs/reports/p1.1-022-acceptance.md` 与 `p2.1-008.2-db-read-cache.md` 是历史记录，按用户决定保留原文，口径见 `reports/p3-032-acceptance.md` §四）
  - [x] 已知限制（README §七 第 1~5 条）在报告中重述，并有对应判据编号。（`reports/p3-032-acceptance.md` §六 逐条重述并挂 F-7 / P-7、F-3、R-2、R-6、L-7 / F-4）
  **测试要求**：无。
  **落地记录**：`docs/api/session.md`（`WAITING`、三个工具事件、`RunEvent` 的 6 个工具字段、「工具轮次与挂起」一节）、`docs/api/readme.md`（接口总数 26 条与领域计数）、`docs/api/runtime.md`（改名说明不再写旧路径字面量）、`docs/develop.md`（会话业务接口段补 P3 的三处变化）、`docs/workspace.md`（`service-core` 的审计职责与依赖表述、`lib-db` 的 `0001` / `0002` 迁移清单、`agent-tool` 段已落地）。
- [x] **P3-032 阶段验收与报告**

  **目标**：七条验收方向逐条取证，出阶段报告。
  **前置依赖**：P3-030、P3-031 与全部前置批次。
  **涉及文件 / Crate**：`docs/reports/p3-032-acceptance.md`、`.steps/README.md`（P3 行）、`.steps/p3/*.md`（状态回填）。
  **实施步骤**：
  1. 按 [acceptance.md](acceptance.md) 的分组逐条执行 / 核对，记录「判据 → 证据 → 结论」。
  2. 报告结构照 `docs/reports/p2.2-016-acceptance.md`：结论、逐组判据、偏差与修订、遗留问题（含未授权而未执行的条目，必须显式写明）。
  3. 回填 `.steps/README.md` 的 P3 行与本目录各文档的状态行；任务勾选只对「验收标准逐条可验证且已通过」的任务生效。
  4. 与 P2 决策的冲突处（D-P2-008 的修订、同会话串行的例外、P1.1 接口改名）在报告里单列一节说明。
  5. 若出现与规划不符之处，写进 [decisions.md](decisions.md) 的新条目，不改历史条目。
  **验收标准**：
  - [x] 报告中每条判据都有证据类型（命令 / 用例 / 静态核对 / 人工）与结论，未执行项显式标注「未授权」。（B ~ R 各组逐条给出证据类型与结论；无「未授权」留空项——本轮全量回归已授权；C-3 标注「部分取证」并在 §八 给出补证方式）
  - [x] `.steps/README.md` 与 `.steps/p3/README.md` 的状态一致。（两处均为「P3 完成（8 个批次 / 32 个任务），Batch H 已验收，报告 `docs/reports/p3-032-acceptance.md`」）
  - [x] 没有把「代码写完」当成「验收通过」的勾选。（C-3 缺批量确认点用例，保持「部分取证」；`acceptance.md` 的 C-3 行同步标注，其余判据均有用例 / 静态 / 人工证据）
  **测试要求**：**需要授权后执行**（`make test ARGS="tool"`、`ARGS="client"` 与 `--all` 回归）。（本轮经授权执行 `make test ARGS="--all"`：103 个用例全部通过，报告 `target/tests/run-1795605450775-43892/README.md`；`tool` / `client` 两组包含在其中）
