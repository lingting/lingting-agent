# P4 技术决策记录

**状态：已定稿（可开工）。** 本文件记录 P4 的阶段级决策，编号 `D-P4-xxx`；被否决与被修订的方案保留记录，不删除。

任务分解见 [tasks.md](tasks.md)，判据见 [acceptance.md](acceptance.md)，阶段边界见 [README.md](README.md) §二。

每项记录包含：问题 / 背景与证据 / 可选方案 / 决策 / 理由 / 约束 / 承接任务。

状态定义：

| 状态 | 含义 |
|------|------|
| `DECIDED` | 已确定，可直接执行 |
| `DECIDED (待验证)` | 方向已定，但依赖某个实现任务确认 API 细节 |
| `OPEN` | 未确定，动手前必须讨论 |
| `REJECTED` | 已否决（保留记录，不删除） |
| `DEFERRED` | 明确推迟到后续阶段 |

---

## 决策索引

| ID | 主题 | 状态 |
|---|---|---|
| D-P4-001 | 新增 `agent-mcp`，MCP 协议类型只在该 Crate；不依赖 `types-core` | DECIDED |
| D-P4-002 | `rmcp` 3.11.0 与客户端特性集 | DECIDED (待验证) |
| D-P4-003 | `Tool` 契约放宽：`&str` + `ToolSchema::from_value` | DECIDED |
| D-P4-004 | `ToolRegistry` 运行期可变（`RwLock` + 条目默认权限） | DECIDED |
| D-P4-005 | 模型可见工具名 `mcp__<server>__<tool>` 与规范化规则 | DECIDED |
| D-P4-006 | `file_access` 由配置声明（缺省 `none`；精确名 + `*` 通配，支持 `write*` / `pre*write`），并发按声明推导；`ToolRisk` 由 `readOnlyHint` 推导 | DECIDED |
| D-P4-007 | MCP 工具默认权限按 server 配置，缺省 `allow` | DECIDED |
| D-P4-008 | 配置持久化：独立 `mcp.toml` 唯一来源，启停只在内存 | DECIDED |
| D-P4-009 | 连接时机：启动期并发连接，失败不阻塞启动 | DECIDED |
| D-P4-010 | 工具快照差量刷新与 `tools/list_changed` | DECIDED (待验证) |
| D-P4-011 | 退避重连策略与关闭开关 | DECIDED |
| D-P4-012 | `tools/list` 自动翻页与单 server 工具数处理 | DECIDED |
| D-P4-013 | stdio 安全边界：显式 `enabled`，不做可执行路径白名单 | DECIDED |
| D-P4-014 | HTTP transport 只支持静态请求头 | DECIDED |
| D-P4-015 | 审计与事件：不新增事件、`tool_calls` 不加列 | DECIDED |
| D-P4-016 | 管理接口形态：3 条，动作段沿用 `list` / `update` | DECIDED |
| D-P4-017 | 调用结果映射：文本拼接、`is_error`、非文本降级 | DECIDED |
| D-P4-018 | 超时与取消：复用执行器，不发 `notifications/cancelled` | DECIDED |
| D-P4-019 | 状态可见性：接口 + `metrics` 的 `mcp` 段 | DECIDED |
| D-P4-020 | 关闭顺序与子进程终止 | DECIDED |
| D-P4-021 | 测试替身：`bin-tests` 隐藏子命令扮演 MCP server | DECIDED |
| D-P4-022 | 错误文案单一产点与脱敏边界 | DECIDED |
| D-P4-023 | 本阶段不改 `agent-runtime` | DECIDED |

---

## D-P4-001 新增 `agent-mcp`，MCP 协议类型只在该 Crate

- **状态**：`DECIDED`
- **问题**：MCP 客户端与适配器放在哪里？要不要让它认识 `types-core` 的配置类型？
- **背景与证据**：
  - `docs/workspace.md` §3.2 已规划 `agent-mcp`（职责：MCP Client / 配置 / 生命周期 / 发现 / 适配 / 两种 transport / 错误处理），并写明「MCP 只是工具的一种来源」「不得让 MCP 协议类型渗透到 Agent Runtime 或更上层」。
  - `AGENTS.md` 第 17 条：第三方框架与协议通过 Adapter 隔离，不得让协议类型渗透 Agent Runtime。
  - P3 的先例：`agent-tool` 不依赖 `types-core`，`[tools]` 配置由装配层翻译成 `ToolLimits` / `ToolPolicy` 再喂进来（`manager-agent::build_tools`）。
- **可选方案**：
  1. 新增 `agent-mcp`，只依赖 `agent-tool` / `lib-core`（不依赖 `types-core`），配置由装配层翻译；
  2. 新增 `agent-mcp` 并直接依赖 `types-core`，读 `McpConfig`；
  3. 把 MCP 塞进 `agent-tool`（一个 crate 管「工具」与「工具来源」）；
  4. 在 `manager-agent` 里直接写 MCP 客户端（不新建 crate）。
- **决策**：①。
- **理由**：
  - ③会让 `agent-tool` 依赖 `rmcp`，所有消费方都背上 MCP 的编译与依赖面，且「工具抽象」与「协议接入」在同一个 crate 里互相污染，与「工具层不认识 MCP」（判据 A-3）直接冲突。
  - ④把领域能力放进装配层，`manager-agent` 会同时承担进程装配与协议实现，违反「装配层只做装配」的既有口径。
  - ②少一次配置翻译，但会让配置类型（`types-core`）成为领域 crate 的依赖，等于把「配置」当成领域概念；P3 已经用「装配层翻译」解决了同类问题，继续沿用可以少一套约定。
- **约束**：
  - `agent-mcp` 的依赖清单里不得出现 `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `lib-db` / `types-core` / `lib-web*` / `bin-*`（判据 A-1）。
  - `rmcp` 只允许出现在 `agent-mcp` 与 `bin-tests`（判据 A-2）。
  - `docs/workspace.md` 原规划里 `agent-mcp` 依赖 `agent-context-core`，本阶段不成立（只接 `tools`，不参与 Context 组装）；P4-016 负责把结构文档改成实际依赖。
- **承接任务**：P4-001。

---

## D-P4-002 `rmcp` 3.11.0 与客户端特性集

- **状态**：`DECIDED`（P4-001 编译期已验证，见下方「实测结果」）
- **问题**：用哪个版本的 `rmcp`，开哪些特性，stdio 与 streamable HTTP 两种 transport 的可用性如何？
- **背景与证据**：
  - `cargo add rmcp --dry-run -p agent-tool`（2026-11-25）解析到 **3.11.0**，可用特性包含 `client` / `transport-child-process` / `transport-io` / `transport-streamable-http-client` / `transport-streamable-http-client-reqwest` / `server`。
  - docs.rs 3.11.0 已确认：`transport::child_process::TokioChildProcess`（含 `kill_on_drop`、带超时的优雅关闭）、`transport::streamable_http_client::StreamableHttpClientTransport`（支持 `from_uri`，reqwest 后端）、`service::{RunningService, Peer}`、`model::Tool { name, description, input_schema: Arc<JsonObject> }`。
  - 仓库当前没有 `rmcp` 依赖，`Cargo.lock` 需联网拉取（rsproxy 镜像已配置）。
- **可选方案**：
  1. `rmcp 3.11`，`default-features = false` + `client` + `transport-child-process` + `transport-streamable-http-client-reqwest`；
  2. 用默认特性（会带上 `server` / `schemars` / `base64` / `uuid` 等）再加客户端特性；
  3. 固定到 `rmcp 2.x`（旧 API）；
  4. 自研 MCP 协议实现（JSON-RPC + 两种 transport）。
- **决策**：①，特性清单按编译器报错补齐后回填本项。
- **理由**：
  - ③没有收益：3.11 的 client 与 transport 都齐全，降版本只会拿旧 API。
  - ④是重造一个协议栈（JSON-RPC 帧、初始化协商、SSE、会话），与本阶段目标（接入生态）相反。
  - ②会让正式构建也编 server 侧代码与 `schemars`，与「不实现 MCP Server」（Out of Scope）矛盾，且拉长编译时间。
  - ①的代价是需要处理隐式特性缺失（如 `macros`），用编译器报错驱动补齐即可，属实现细节。
- **约束**：
  - 版本必须固定在 workspace 的一处（`[workspace.dependencies]`），成员 crate 只写 `workspace = true`。
  - 出现两份 `reqwest`（rmcp 传递 0.12 与 workspace 0.13）时**不强行统一**：统一版本会动 P2 的 Provider 链路，收益（编译时间）小于风险。核对结果记进本项。
- **承接任务**：P4-001（特性与依赖核对）、P4-005 / P4-006（transport 实测）。
- **实测结果（P4-001，2026-11-25）**：
  - `Cargo.lock` 解析到 **3.11.0**，`cargo check -p agent-mcp` 通过：三项特性（`client` / `transport-child-process` / `transport-streamable-http-client-reqwest`）**足够**，不需要按报错补 `macros` / `uuid` / `base64` 之类的隐式特性。
  - `cargo tree -i rmcp` 只命中 `agent-mcp`（本批尚无 `bin-tests` 的 `server` 特性）。
  - `cargo tree -i reqwest` 只有 **0.13.5 一份**（`rmcp` 与 `framework-http` 共用 workspace 版本），没有「两份 reqwest」的构建代价。
  - `cargo tree -p agent-mcp` 里不出现 `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `types-core` / `lib-db` / `lib-web*` / `bin-*`。

---

## D-P4-003 `Tool` 契约放宽：`&str` + `ToolSchema::from_value`

- **状态**：`DECIDED`
- **问题**：MCP 的工具名与 schema 都来自运行期的 server 响应，而 P3 的 `Tool` 契约要求 `&'static str` 与「由 `schemars` 派生的 `ToolSchema`」。怎么让动态工具进得来？
- **背景与证据**：
  - `crates/agent-tool/src/tool.rs`：`fn name(&self) -> &'static str`；`fn description(&self) -> &'static str`；`fn schema(&self) -> &'static ToolSchema`。
  - `crates/agent-tool/src/schema.rs`：`ToolSchema` 只有 `of::<A: JsonSchema>()` 一个构造入口，且会剥 `$schema` / `title` / `$defs`。
  - MCP 的 `inputSchema` 是外部 JSON Schema，字段与 `schemars` 的产出风格不同，不能保证能被某个 `*Args` 类型表达。
- **可选方案**：
  1. 三个方法去掉 `'static`（`&str` / `&ToolSchema`），`ToolSchema` 增加 `from_value`；
  2. 保留 `&'static`，MCP 适配器用 `Box::leak` 把字符串泄漏成 `'static`；
  3. 保留 `&'static`，为每个 MCP 工具生成一个静态代理（`mcp_tool_1`、`mcp_tool_2`…），名字与 schema 走另一张表；
  4. 放弃 `Tool` 抽象，给 MCP 工具开一条独立执行路径。
- **决策**：①。
- **理由**：
  - ②的泄漏量与「server 数量 × 工具数量 × 重连次数」成正比，长期运行必然增长，且没有任何回收点——这是必须避免的设计。
  - ③把「工具的身份」拆成两份（静态代理名 + 运行期真名），审计、事件、模型清单会出现两套名字，可读性崩溃。
  - ④直接违反 P3 的核心目标（G-1：MCP 工具与内置工具同构），也是本阶段要消除的问题本身。
  - ①只是去掉生命周期约束，内置工具的实现一行都不用改（返回 `&str` 是字面量的子类型转换）。
- **约束**：
  - `validate_name` 仍是唯一的名字校验点；动态名字同样要过。
  - `ToolSchema::from_value` 不剥键（与 `of::<A>()` 的口径差异写在注释里），MCP 的 schema 原样透传（判据 S-2）。
  - 不引入 `jsonschema` 之类的校验库：MCP 参数校验归 server（D-P4-017）。
- **承接任务**：P4-002。

---

## D-P4-004 `ToolRegistry` 运行期可变（`RwLock` + 条目默认权限）

- **状态**：`DECIDED`
- **问题**：工具发现是运行期事件（连接成功、`list_changed`、重连），注册表怎么支持增删？server 级权限放在哪里？
- **背景与证据**：
  - `ToolRegistry` 现在是 `BTreeMap<String, Arc<dyn Tool>>`，`register` 只在 `manager-agent::build_tools` 里调用；`AgentRuntime.tools` 与 `ToolExecutor` 各持一份 `Arc<ToolRegistry>`（同一个实例）。
  - 权限只有两档来源：`ToolPolicy`（`[tools.permissions]`）与 `ToolRisk` 缺省（`ToolPolicy::permission_of`）。MCP 的 server 级权限在配置装载时还没有对应的工具名。
- **可选方案**：
  1. 内部 `RwLock<BTreeMap<String, Entry>>`，`Entry { tool, default_permission }`；`lookup` 克隆 `Arc`；
  2. 保持不可变，用 `ArcSwap<ToolRegistry>` 在发现后整表替换（`AgentRuntime` 与 `ToolExecutor` 都要重新指向新表）；
  3. 保持不可变，启动时用「每个 server 一个代理工具」的方式固定注册；
  4. 把 `ToolPolicy` 扩展成「前缀规则 + 显式规则」（`agent-tool` 认识 `mcp__` 前缀）。
- **决策**：①。
- **理由**：
  - ②需要同时替换两处持有者，且在替换窗口里「模型清单」与「执行期查找」可能看到不同的表；一次 Run 跨越多轮，窗口内的不一致会以「未知工具」形式暴露出来。①的读锁是原子的，任何时刻都只有一份真实表。
  - ③把发现结果压成一个代理工具，模型看不到真实的工具 schema，等于放弃 G-3。
  - ④让 `agent-tool` 认识 MCP 的命名约定（判据 A-3 不允许）。
  - ①的代价是每次 `lookup` 克隆一次 `Arc`（原子自增），与 P3 的借用相比可以忽略；写在发现差异上的写锁只发生在低频路径。
- **约束**：
  - 三级判定顺序固定：`[tools.permissions]` 显式 > 条目默认 > `ToolRisk` 缺省（`ToolPolicy` 语义不变）。
  - 读写锁不跨 `await` 持有；写只发生在工具快照差异处。
  - `model_definitions()` 的顺序仍由名字字典序决定（与 P3 一致，判据 S-3 依赖这一点）。
- **承接任务**：P4-003、P4-010。

---

## D-P4-005 模型可见工具名 `mcp__<server>__<tool>` 与规范化规则

- **状态**：`DECIDED`（需求方确认前缀形式）
- **问题**：不同 server 可能提供同名工具，MCP 的工具名也可能不符合 `^[a-z][a-z0-9_]{1,63}$`（含点、连字符、大写、超长）。模型可见名怎么生成？
- **背景与证据**：
  - `agent-tool::validate_name`：`^[a-z][a-z0-9_]{1,63}$`，注册期拒绝非法名；重名拒绝注册。
  - 内置工具已占用 `calculator` / `current_time` / `file_read` / `file_search` / `http_request` 五个名字。
  - 阶段 README 把「工具命名冲突」列为风险项，未定策略。
- **可选方案**：
  1. `mcp__<server>__<tool>` 双下划线三段式；
  2. `<server>_<tool>` 单下划线两段式；
  3. 直接用 MCP 原名，冲突拒绝；
  4. 由用户为每个 server 配置前缀。
- **决策**：①，并按 README §4.4 的 5 条规则规范化（小写化、非法字符替换、折叠下划线、补前缀、超长截断 + `blake3` 8 位短哈希）；冲突时保留先注册者，后来者追加 `_<8 位哈希>`。
- **理由**：
  - ③在生态里几乎必然冲突（多个 server 都提供 `read_file`），而且 MCP 原名不保证满足 `validate_name`。
  - ②的歧义在 server 名本身含下划线时出现（`my_server` + `read_file` 与 `my` + `server_read_file` 无法区分），三段式用双下划线消歧。
  - ④多一个配置面，且用户配错前缀同样会冲突；命名是可以由系统确定的规则，不需要人工输入。
  - 超长截断加哈希是为了「可复算 + 可区分」：确定性哈希让同一份工具列表两次发现得到同一个名字（判据 S-3）。
- **约束**：
  - 规范化只在一个函数里实现（`agent-mcp/src/naming.rs`），不允许第二处拼接工具名。
  - 原名必须可查（`McpToolVO.originalName`），否则配置 `[tools.permissions]` 与排查会失去依据。
  - 冲突消解的顺序固定为「server `id` 升序 + 原名升序」，保证跨进程可复现。
- **承接任务**：P4-007。

---

## D-P4-006 `file_access` 由配置声明（缺省 `None`，精确名 + `*` 通配模式），并发按声明推导

- **状态**：`DECIDED`（需求方补充：支持按工具名声明、`*` 批量声明，以及 `write*` / `pre*write` 这类通配模式）
- **问题**：MCP 工具的本地文件操作行为在 server 里，本进程无法自述。`ToolFileAccess` 怎么来？`ToolRisk` / `ToolConcurrency` 又怎么定？
- **背景与证据**：
  - `ToolFileAccess`（`None` / `Read` / `Write` / `ReadWrite`）的唯一用途是推导并发默认值 + 记录审计（D-P3-021），它明确是「工具的自述声明」，不是授权依据，也不参与权限判定。
  - `ToolRisk`（`ReadOnly` / `Network` / `Write`）目前只用于推导权限缺省等级；P4 起还会被 `[tools.permissions]` 与 server 级 `permission` 覆盖。
  - MCP 工具的 `annotations` 里有 `readOnlyHint` / `destructiveHint`（server 可选提供，不可靠）。
  - 需求方要求：默认 `None`，允许为特定工具声明其它值，并支持 `*` 批量声明。
- **可选方案**：
  1. `file_access` 由 `[mcp.servers].file_access`（`BTreeMap<工具原名或 `*` 模式, 枚举>`）声明：精确名 > 最具体模式 > `*` > 缺省 `None`；声明写操作的工具恒串行，其余并发按 server / 全局配置；`risk` 由 `readOnlyHint` 推导；
  2. `file_access` 恒 `None`（P4 旧稿），并发只由 server 级 `concurrency` 决定；
  3. `file_access` 一律 `ReadWrite`（保守串行）；
  4. 把每个工具的声明也做成 `[tools.*]` 那样的独立表（与 `[mcp.servers]` 同级）。
- **决策**：①。
- **理由**：
  - ②无法表达「这个 server 里有写文件的工具」，只能整台 server 一起串行或一起并行，粒度太粗。
  - ③让所有 MCP 调用串行，直接放弃 P3 的并行能力。
  - ④把「某个 server 的某个工具」的声明搬到全局段，配置里出现两份 id 交叉引用，容易写错且无法原地复制一个 server 配置。
  - ①的四级优先级（精确名 > 最具体模式 > `*` > 缺省 `None`）足以覆盖真实场景：常见 server 的写工具集中在少数命名族（`write_*` / `edit_*`），一条 `write*` 就能覆盖，其余用 `*` 整体声明。
  - `*` 只是通配字符，不引入正则：正则的能力远超需要（还带来灾难性回溯与转义歧义），而 `*` 足以表达前缀（`write*`）、后缀（`*_write`）与中缀（`pre*write`）。匹配器自研（两指针贪心），不新增第三方依赖。
- **约束**：
  - 键是 **server 报告的工具原名或含 `*` 的模式**（不是模型可见名），与 `McpToolVO.originalName` 对齐；`*` 可出现在任意位置、匹配任意长度，连续 `*` 折叠为一个；不支持正则、`?`、`[...]`（按字面处理）。
  - 缺省（没有任何键命中）为 `None`；装载期校验枚举值与空键，连续 `*` 折叠为一个。
  - 同分规则：命中的模式按「非通配字符数」降序，同分按字典序升序取第一个——保证同一份配置在任何进程里给出同一结果（判据 S-7 依赖）。
  - 并发语义：声明 `Write` / `ReadWrite` 的工具恒为 `Exclusive`（写声明是串行下限，不能被 `concurrency` 放松）；其余工具取 server 级 `concurrency` > `mcp.toml` 顶层 `concurrency` > `Parallel`。
  - 审计里 `file_access` 记录声明值；文档写明「这是自述，不是校验」（README §七 风险 4）。
- **承接任务**：P4-004（配置类型与校验）、P4-007（发现时解析声明）、P4-008（适配器字段）、P4-018（`mcp.file_access` 用例）。

---

## D-P4-007 MCP 工具默认权限按 server 配置，缺省 `allow`

- **状态**：`DECIDED`（需求方确认）
- **问题**：MCP 工具默认走 `allow` / `confirm` / `deny` 哪一档？怎么和 `[tools.permissions]` 共处？
- **背景与证据**：
  - P3 的判定顺序：`deny` > `confirm` > `allow` > 工具 `ToolRisk` 缺省；`deny` 的工具不进模型可见清单。
  - 配置装载时 MCP 工具名还不存在（名字由发现产生），无法在 `[tools.permissions]` 里逐条预先列出——但可以事后加（配置是静态的，名字是确定的规则产物）。
- **可选方案**：
  1. server 级 `permission` 字段（缺省 `allow`）作为工具默认；`[tools.permissions]` 可按可见名逐工具覆盖；
  2. 缺省 `confirm`（每次调用都问）；
  3. 缺省 `deny`（必须逐工具 allow）；
  4. 不引入 server 级配置，全部依赖 `[tools.permissions]`。
- **决策**：①。
- **理由**：
  - 「配置了 server 就等于授权它」（需求方判断）：`enabled = true` 是显式动作，再叠加一次逐工具授权属于重复确认。
  - ②对高频工具（读取文件、查询时间）会造成明显的交互噪声，与 P3 「确认只用于危险工具」的口径相悖。
  - ③在工具数量多时配置量爆炸，且没有 UI 辅助，实际效果是「大规模关闭 MCP」。
  - ④缺少一个「整台 server 走 confirm」的粗粒度开关，逐个工具写 `confirm` 更繁琐。
- **约束**：
  - 判定顺序仍是「显式配置 > 条目默认 > `ToolRisk` 缺省」，与 D-P4-004 一致。
  - `confirm` 复用 P3 的挂起链路，不新增确认机制。
  - `deny` 的工具既不进清单也不执行（判据 S-5 / C-6）。
- **承接任务**：P4-003（条目默认权限）、P4-010（写入默认值）、P4-018（用例）。

---

## D-P4-008 配置持久化：`mcp.toml` 唯一来源，启停只在内存

- **状态**：`DECIDED`（需求方确认）
- **问题**：MCP server 的配置与「启用/禁用」状态存在哪里？要不要落 SQLite？配置放在 `config.toml` 还是独立文件？
- **背景与证据**：
  - 阶段 README（旧稿）§三写过「MCP Server 配置与连接状态持久化（复用 P2 的存储层）」。
  - P2 的既有口径：配置文件只读，Provider / Model / Agent / Tools 配置都没有 CRUD 接口，`lib-db` 只存会话业务数据（`sessions` / `runs` / `messages` / `tool_calls`）。
  - `docs/api/readme.md` 没有配置类接口的先例。
  - **需求方补充（2026-11-25）**：MCP 的配置走**单独的 `mcp.toml`**（应用根目录，与 `config.toml` 同级），不与主配置合并。
- **可选方案**：
  1. `mcp.toml` 是唯一事实源；管理接口的 `enable` / `disable` 只改进程内状态，重启回到配置值；
  2. 新增 SQLite 表（迁移 `0003`）持久化启用状态与最近连接结果；
  3. 不提供启停接口，只提供列表 / 重连 / 探测；
  4. 管理接口直接改写 `mcp.toml`。
- **决策**：①，并**修订**阶段 README 旧稿的「落库」说法；配置文件从「`config.toml` 的一段」改为独立的 `mcp.toml`。
- **理由**：
  - 配置属于「部署事实」，不是业务数据：落库会出现「配置文件的 enabled 与数据库的 enabled 谁优先」的二义，以及两份事实源的迁移问题。
  - ④会让服务端对配置文件有写权限，破坏「配置文件只读」这条贯穿 P1 ~ P3 的约束，且并发写需要额外的原子替换与备份逻辑。
  - ③让「运行期临时关掉一个吵闹的 server」变成「改配置 + 重启」，实用价值低。
  - ①的代价是重启后回到配置值，属于可接受的语义，写进文档与接口说明（D-P4-016）。
  - **独立成文件**而非 `config.toml` 的一段：`[[servers]]` 随使用增长（每个 server 十几行），塞进主配置会把 Provider / Agent / Tools 淹没；装载口径也不同——`mcp.toml` 缺失即零 server，且只有服务端读它（客户端命令不必因 MCP 配置写错而失败）。
- **约束**：
  - 不新增迁移；`lib-db` 本阶段零改动。
  - 连接状态是进程内的易失事实（`status` / `lastError` / `nextRetryAtMs`），不持久化。
  - 配置文件只读：`mcp.toml` 是唯一持久化来源，管理接口只改进程内状态（与 README §4.8 一致）。
  - 文档里必须写清「重启回到配置值」。
- **承接任务**：P4-010、P4-014、P4-015。

---

## D-P4-009 连接时机：启动期并发连接，失败不阻塞启动

- **状态**：`DECIDED`
- **问题**：什么时候连 MCP server？连不上怎么办？
- **背景与证据**：
  - `assemble_with` 是异步函数，启动路径里已经有「迁库 → 自愈 → 建 Repository / Service」的固定顺序，失败即退出。
  - `[tools]` 的装配是同步的（五个内置工具），MCP 的连接是异步且可能很慢的（`npx` 首次拉包）。
  - 阶段 README 的验收方向第 3 条要求「崩溃或断开时给出明确状态」，要求状态在启动后即可见。
- **可选方案**：
  1. 启动期对所有 `enabled` 的 server 并发连接（各自 `connect_timeout_ms`），失败只记状态；运行期仅按退避重连或手动重连；
  2. 完全懒连接（第一次调用或第一次查接口时连）；
  3. 串行连接，任一失败即启动失败。
- **决策**：①。
- **理由**：
  - ③把「MCP 可用性」变成「服务可用性」的前置条件，与 G-4 的「不阻塞启动」相反。
  - ②让第一次 Run 承担连接延迟（可能在工具执行超时里失败），且「启动后状态可见」需要额外触发点。
  - ①的启动代价上界是「最慢 server 的握手超时」（默认 10s，可配置），比服务起不来好得多。
- **约束**：
  - 任一 server 失败都不让 `assemble_with` 返回 `Err`（判据 L-2）。
  - 并发连接不做全局并发上限（server 数量由用户配置控制；单个连接的资源开销有界）。
  - 启动日志每个 server 一行（`id` / `transport` / 状态 / 工具数），不打印命令行与 header。
- **承接任务**：P4-010、P4-011。

---

## D-P4-010 工具快照差量刷新与 `tools/list_changed`

- **状态**：`DECIDED (待验证)`
- **问题**：发现到的工具怎么进注册表？server 侧工具变化了怎么办？
- **背景与证据**：
  - MCP 规范有 `notifications/tools/list_changed` 通知；`rmcp` 的 `ClientHandler` 提供通知回调（具体方法名在 3.11 的 docs 里以 `on_*` 形式出现，需 P4-005 实测确认）。
  - 注册表可变后（D-P4-004），「增删」需要一份可比对的全量快照。
- **可选方案**：
  1. 每次发现产出一份全量快照，与上一份做差集：新增注册、消失注销；`list_changed` 触发一次重新发现；
  2. 只做「追加注册」，不注销（工具消失后仍可被调用，直到重启）；
  3. 每次发现前清空该 server 的全部工具再注册（窗口内模型清单为空）。
- **决策**：①。
- **理由**：
  - ②会让模型看到一个 server 已经不提供的工具，调用只能得到错误，属于「状态与现实不一致」。
  - ③在「清空 → 注册」之间有窗口，正在构造请求的 Run 可能拿到空清单或半份清单；差集没有这个窗口。
  - ①要求「规范化后的名字可复算」（D-P4-005），这样才能用名字做差集键。
- **约束**：
  - 差集以模型可见名为键；被注销的工具从注册表移除后，执行期查找得到「未知工具」→ `DENIED`（P3 既有语义）。
  - 发现失败时保留上一份快照，不清空（避免一次抖动让工具消失）。
  - 刷新与启停/重连共用同一把按 server 分段的锁，不持全局锁。
- **承接任务**：P4-007、P4-010。

---

## D-P4-011 退避重连策略与关闭开关

- **状态**：`DECIDED`
- **问题**：server 挂了要不要自动重连？退避怎么算？能不能关掉？
- **背景与证据**：
  - 阶段 README 的风险项 7 要求「单个 MCP Server 卡死不能拖垮整个 Run 或 Server 进程」。
  - P1.1 的客户端心跳/问询有既定的定时任务模式（`framework_timer`），但那是按客户端；MCP 的重连是「按 server、状态驱动」，不适合复用全局定时扫描。
  - 配置面已经有 `reconnect_initial_backoff_ms` / `reconnect_max_backoff_ms`。
- **可选方案**：
  1. 每个 `Failed` 的 server 起一个按 server 计的重连任务：初始退避 2 倍增长到上限，无限次；上限配 0 关闭自动重连；
  2. 固定间隔重连（无退避）；
  3. 只手动重连（完全不做自动重连）；
  4. 由全局定时任务每 N 秒检查一遍所有 `Failed` 的 server。
- **决策**：①。
- **理由**：
  - ②在 server 持续不可用时会产生稳定的连接风暴（每个间隔一次 `npx` 启动尝试）。
  - ③最常见的使用场景（远程 server 短暂抖动）需要人工干预，体验差。
  - ④与既有定时任务混在一起会让「重连时机」依赖扫描周期，退避语义反而更难保证；按 server 的独立任务更直观，且天然隔离。
- **约束**：
  - 重连任务必须能被 `enable` / `disable` / `shutdown` 打断（取消 token）。
  - 关闭流程置位后不再排新的重连（复用 `lib_core::lifecycle` 的关闭标记）。
  - 每次尝试都计入 `metrics.mcp.reconnects` / `connects` / `connect_failures`。
- **承接任务**：P4-010、P4-012。

---

## D-P4-012 `tools/list` 自动翻页与单 server 工具数处理

- **状态**：`DECIDED`
- **问题**：`tools/list` 是分页的，要翻到底吗？单个 server 工具特别多怎么办？
- **背景与证据**：
  - MCP 的 `tools/list` 用 `nextCursor` 分页（`rmcp::service::MergedList` 提供了聚合能力）。
  - 工具数量直接影响每轮请求的 `tools` 字段大小（token 成本）与模型选择难度。
- **可选方案**：
  1. 翻到底（自动翻页），不做上限；
  2. 翻到底但设硬上限（如 256 个），超出截断并记 `lastError`；
  3. 只取第一页。
- **决策**：②，上限常量集中在 `agent-mcp`（`MAX_TOOLS_PER_SERVER = 256`）。
- **理由**：
  - ③会让「工具多」的 server 表现不完整且无提示。
  - ①在异常 server 上可能无限翻页（无界资源消耗），与「故障隔离」冲突。
  - ②的截断是显式可见的（`lastError` 记一条固定文案），不会被误认为「server 只有这些工具」。
- **约束**：
  - 截断不影响已注册工具的可用性。
  - 翻页的单次请求受 `connect_timeout_ms` 之外的执行器超时保护（调用的超时由执行器管；发现阶段的整体耗时以「页数上限 32」兜底，超出按截断处理）。
- **承接任务**：P4-007。

---

## D-P4-013 stdio 安全边界：显式 `enabled`，不做可执行路径白名单

- **状态**：`DECIDED`（需求方确认）
- **问题**：stdio server 是「启动任意可执行程序」，要不要限制可执行路径？默认启用还是默认禁用？
- **背景与证据**：
  - 阶段 README 的风险项 5：stdio MCP Server 启动即代表授权执行；是否需要显式确认、是否限制可执行路径。
  - `config.toml` 只读（D-P4-008），修改它需要文件系统写权限，本身就是一次人工动作。
  - P3 的安全边界类比：`http_request` 默认 `deny`、白名单为空即拒绝（D-P3-014），是「默认关闭 + 显式开启」的模式。
- **可选方案**：
  1. `enabled` 缺省 `false`（写了配置不启动），不做路径白名单；
  2. `enabled` 缺省 `false` + `mcp.toml` 的可执行路径白名单；
  3. 配置即启动（`enabled` 缺省 `true`）。
- **决策**：①。
- **理由**：
  - ③让「复制一段示例配置」就等于授权执行任意程序，风险与便利不匹配。
  - ②的收益有限：白名单本身也在同一份配置文件里，能改配置的人同样能改白名单；它只能挡住「配置被注入但白名单未变」的场景，而本项目的配置是本地人工维护、无远程写入路径。
  - ①保留最关键的一道显式动作（`enabled = true`），并且与 D-P4-007 的「配置了 server 就授权它的工具」配套：不启用就没有工具。
- **约束**：
  - `enabled` 缺省 `false`，stdio 与 http 一致（判据 E-4）。
  - 环境变量只追加不覆盖；`cwd` 只允许应用根目录下的相对路径或绝对路径（装载期校验，不越权）。
  - 不做沙箱；不是安全隔离，只是「默认不执行」（写进 §七 风险 9）。
- **承接任务**：P4-004（装载校验）、P4-005（子进程启动）。

---

## D-P4-014 HTTP transport 只支持静态请求头

- **状态**：`DECIDED`
- **问题**：远程 MCP server 的鉴权怎么配？
- **背景与证据**：
  - MCP 规范的鉴权走 OAuth 2.1（发现 + 动态注册 + 令牌刷新），实现量大且需要回调地址。
  - 本项目的客户端是本地服务，没有浏览器回调流程。
  - `rmcp` 3.11 提供了 `auth` 系列特性，但启用它们会把令牌管理带进本项目。
- **可选方案**：
  1. 只支持静态 `headers`（含 `Authorization`），由用户自行管理令牌有效期；
  2. 实现 OAuth（带浏览器回调）；
  3. 不支持鉴权，只连内网无鉴权 server。
- **决策**：①。
- **理由**：
  - ②的复杂度（回调服务、令牌存储与刷新、与本地单用户的交互）远超本阶段范围，且 SPA / 桌面场景下 OAuth 的用户体验设计需要单独决策。
  - ③把最常见的企业内网场景挡在门外。
  - ①的代价是令牌过期后调用失败，用户需要更新配置并重启——与 D-P4-008 的「配置只读、重启生效」一致。
- **约束**：
  - header 值不得出现在事件、审计、接口响应与日志里（判据 E-2）。
  - 只允许 `http` / `https`，不做重定向跟随。
- **承接任务**：P4-004、P4-006。

---

## D-P4-015 审计与事件：不新增事件、`tool_calls` 不加列

- **状态**：`DECIDED`
- **问题**：MCP 调用要不要新事件类型？审计表要不要记 `server_id`？
- **背景与证据**：
  - P3 的 `tool_call_started` / `tool_call_completed` / `tool_call_confirm_required` 已经带 `toolCallId` / `toolName` / 状态 / 耗时 / 错误文本。
  - `tool_calls` 表的列与 `ToolCallRecord` 一一对应；模型可见名已经带 `mcp__<server>__` 前缀。
  - `lib-web/src/tool/api.rs` 的查询载荷 `ToolCallVO` 是契约快照，加字段会牵动 SDK 与前端。
- **可选方案**：
  1. 复用既有事件与审计表，靠工具名前缀区分来源；
  2. 新增 `mcp_call_started` / `mcp_call_completed` 与 `tool_calls.server_id` 列；
  3. 新增事件但审计表不改。
- **决策**：①。
- **理由**：
  - ②会让客户端为「同一个语义」写两套渲染（工具调用卡片 vs MCP 调用卡片），而两者对用户是同一件事。
  - ③一半的改动换来一半的收益：事件里多了字段，审计里还是查不到 server，排查仍要靠名字解析。
  - ①的前提是命名规则稳定（D-P4-005），因此把「前缀是事实源」写进约束与用例（判据 C-8）。
- **约束**：
  - `RunEventKind` 的取值集合不变（判据 R-3 静态核对）。
  - `file_access` 记录配置的声明值（缺省 `NONE`，见 D-P4-006）。
  - 不新增迁移。
- **承接任务**：P4-008、P4-018。

---

## D-P4-016 管理接口形态：3 条，动作段沿用 `list` / `update`

- **状态**：`DECIDED`
- **问题**：管理接口要几条、路径怎么起、启停/重连/探测怎么表达？
- **背景与证据**：
  - `docs/api/readme.md`：path 统一 `api/` 开头，资源段小写单数，动作段只用 `create` / `update` / `delete` / `get` / `list` / `cancel` / `stream` / `post`；无路径参数（ID 只能走查询参数或请求体）。
  - P3 的先例：`api/client/post` 用「一个 POST + `kind` 枚举」承载心跳、问询应答、工具确认三种语义。
- **可选方案**：
  1. 3 条：`GET api/mcp/server/list`、`GET api/mcp/tool/list`、`POST api/mcp/server/update`（`action` 枚举含 4 个动作）；
  2. 5 条：把 enable / disable / reconnect / probe 各开一条 `POST api/mcp/server/<动作>`；
  3. 1 条大接口：`POST api/mcp/post`，用 `kind` 区分查询与动作。
- **决策**：①。
- **理由**：
  - ②需要新增 4 个动作段（`enable` / `disable` / `reconnect` / `probe`），违反动作词表；若要合规就得写成 `post` 之类的语义空洞的名字。
  - ③把读与写塞进一个 POST，丢掉 GET 的可缓存与幂等语义，也不符合现有领域划分（每个领域一组资源）。
  - ①用 `action` 枚举把「同一资源上的运行期动作」收敛到一个入口，与 P3 的 `kind` 先例一致，且接口面最小。
- **约束**：
  - 4 个动作都返回**动作完成后**的 `McpServerVO`，客户端不需要二次查询。
  - 未知 `serverId` → 400 固定文案；对 `DISABLED` 的 server 发 `PROBE` → 200 + 状态（不是错误）。
  - 接口总数 26 → 29；`docs/api/mcp.md` 与索引同步（判据 I-5）。
- **承接任务**：P4-014、P4-015。

---

## D-P4-017 调用结果映射：文本拼接、`is_error`、非文本降级

- **状态**：`DECIDED`
- **问题**：MCP 的 `CallToolResult` 有文本、图片、音频、资源、结构化内容多种形态，怎么变成 P3 的 `ToolOutput { text, is_error, truncated }`？
- **背景与证据**：
  - OpenAI 兼容协议的工具结果只有文本（没有 error 字段），P3 因此把错误也表达成文本（D-P3-016 的口径）。
  - `ToolOutput` 有 `is_error` 与 `truncated`，截断由执行器按 `[tools] max_result_chars` 统一处理。
  - 模型看不到二进制；内联 base64 图片只会浪费 token。
- **可选方案**：
  1. 文本段按顺序换行拼接；图片/音频/资源替换成固定占位文本；`is_error` 透传；无文本时退到 `structured_content` 的 JSON；
  2. 把整个 `CallToolResult` 序列化成 JSON 文本；
  3. 只取第一个文本段，其余丢弃。
- **决策**：①。
- **理由**：
  - ②会把协议结构（`type` / `mimeType` 等）塞进模型上下文，噪声大且随 rmcp 版本变化。
  - ③丢失多段文本（分页结果、多文件摘要等常见形态）。
  - ①的占位文本让模型知道「这里有非文本内容」，比静默丢弃或内联 base64 都好。
- **约束**：
  - 结果文本非空（判据 C-2）；空内容用固定文案。
  - 脱敏与截断沿用 P3 的写出侧规则，不在这里二次实现。
  - 参数不做本地校验（校验归 server），非法参数由 server 报错，按失败回灌。
- **承接任务**：P4-008、P4-018。

---

## D-P4-018 超时与取消：复用执行器，不发 `notifications/cancelled`

- **状态**：`DECIDED`
- **问题**：MCP 调用的超时与取消谁来管？要不要向 server 发取消通知？
- **背景与证据**：
  - P3 的执行器：`tokio::time::timeout([tools] timeout_ms, tool.call(..))` 与 `cancel.cancelled()` 并列 `select!`，取消即 drop future（D-P3-019）。
  - MCP 规范有 `notifications/cancelled`，但 server 可以不理会；`rmcp` 的客户端在 drop 掉请求后是否自动发送该通知需要在 P4-005 实测（不依赖它）。
  - 已知限制：drop future 不会中断 server 侧的执行。
- **可选方案**：
  1. 完全复用执行器的超时与取消；MCP 侧不额外设超时、不发取消通知；
  2. 执行器 + MCP 侧再设一层请求超时；
  3. 取消时显式发 `notifications/cancelled` 再 drop。
- **决策**：①。
- **理由**：
  - ②会出现「两层超时」的竞态：外层先超时时，内层还在等，错误文案与状态可能不一致。
  - ③在 server 不理会的情况下与 ①等价，却多一条协议路径与一处失败分支；如果后续实测确认 `rmcp` 已自动发送，③就更没必要。
  - ①的语义与内置工具完全一致（超时是一次工具失败、取消是 Run 终止），这正是 G-5 要的同构。
- **约束**：
  - 取消后不残留完成事件（P3 的 `emit_cancelled` 已在执行器/工具轮里处理）。
  - 已知副作用写进 §七 风险 7：server 可能继续把调用执行完。
- **承接任务**：P4-008、P4-018。

---

## D-P4-019 状态可见性：接口 + `metrics` 的 `mcp` 段

- **状态**：`DECIDED`
- **问题**：server 的连接状态怎么让用户与测试看到？要不要走事件流？
- **背景与证据**：
  - `api/client/stream` 是 Run 事件的唯一通道，事件顺序与 Run 绑定；MCP 连接状态与某个 Run 无关。
  - P2.2 的纪律：测试数据只经 `metrics` 快照与接口输出，不打阈值日志、不读日志。
  - `GET api/mcp/server/list` 已经提供状态。
- **可选方案**：
  1. 接口（D-P4-016）+ `metrics` 的 `mcp` 段；不进事件流、不打阈值日志；
  2. 在 `api/client/stream` 新增 MCP 状态事件；
  3. 只在 `metrics` 里给数字，不给接口。
- **决策**：①。
- **理由**：
  - ②把管理面状态塞进 Run 事件流，客户端必须为「与 Run 无关的事件」做特殊处理，且事件文件的语义会被污染。
  - ③让人工排查只能看数字，看不到 `lastError`；而 `lastError` 恰恰是「为什么连不上」的唯一可读信息。
  - ①的口径与 P3 的审计接口一致：状态查询是只读接口，测试与运维共用。
- **约束**：
  - `metrics` 段只在 `debug` + `metrics` 特性构建下采集（与既有口径一致）。
  - 不打阈值日志；状态变化的日志只有启动/重连时的 `info` / `warn` 各一行。
- **承接任务**：P4-013、P4-014、P4-018。

---

## D-P4-020 关闭顺序与子进程终止

- **状态**：`DECIDED`
- **问题**：服务关闭时 MCP 与 Run 的收尾谁先？子进程怎么保证退出？
- **背景与证据**：
  - `manager-agent::holder::shutdown` 现在只调 `handler::shutdown(runtime)`（停客户端与运行时会话）。
  - `lib-bin-web` 的关闭钩子先进入 200ms 拒绝窗口，再停止监听并排空请求，最后调 holder 的关闭入口。
  - `rmcp::transport::child_process::TokioChildProcess` 自带「关 stdin → 等退出 → `SIGTERM` → kill」的优雅关闭序列。
  - Windows 没有 job object，孙进程不保证回收。
- **可选方案**：
  1. 先停运行时（Run 落终态）→ 再 `manager.shutdown_all()`（优雅关闭 + 超时 kill）；`kill_on_drop(true)` 兜底；
  2. 先关 MCP → 再停运行时；
  3. 只依赖 `kill_on_drop`，不做显式关闭。
- **决策**：①。
- **理由**：
  - ②会让正在执行的工具调用以「服务器通信失败」收尾，Run 因此落 `FAILED` 而不是 `CANCELLED`，改变既有语义（判据 L-8 直接检查这一点）。
  - ③在正常关闭路径里依赖 Drop 的时机（进程退出、runtime 回收），既没有超时保证，也无法记账 `shutdown_kills`。
  - ①的顺序让「Run 的终态」与「MCP 的终止」各归其位。
- **约束**：
  - 关闭期间不排新的重连（D-P4-011）。
  - 优雅关闭超时（`graceful_timeout`）用固定常量（如 5s），不新增配置项。
  - 只断言直接子进程（判据 L-7），孙进程残留写进风险 5。
- **承接任务**：P4-012、P4-018。

---

## D-P4-021 测试替身：`bin-tests` 隐藏子命令扮演 MCP server

- **状态**：`DECIDED`
- **问题**：stdio transport 需要一个「会讲 MCP 的子进程」，从哪来？
- **背景与证据**：
  - 仓库禁止新增 `bin-mock-server` 之类的 binary（`AGENTS.md` 第 26 条：未经规划不得创建 `bin-tui` / `bin-web` / `bin-mock-server`）。
  - `bin-tests` 已经是唯一的测试入口二进制，且 `cli` 已有子命令解析；它只在测试构建里使用，不参与发布。
  - `mock/llm` 的先例：测试替身放在 `bin-tests` 里，由用例控制剧本。
- **可选方案**：
  1. `bin-tests` 增加隐藏子命令 `mock-mcp`（`hide = true`），用 `rmcp` 的 `server` 特性实现最小 MCP server；stdio 与 HTTP 两种模式；
  2. 用 `node` / `python` 脚本当替身（依赖外部运行时）；
  3. 新建 `bin-mock-server`；
  4. 只测 HTTP transport，stdio 靠人工。
- **决策**：①。
- **理由**：
  - ②引入不受控的外部依赖（CI 上不一定有 node / python），且剧本要跨语言维护。
  - ③违反仓库约束（新增 binary 需先规划）。
  - ④放弃 G-2 的一半（stdio 是本阶段的主要场景）。
  - ①的代价是 `bin-tests` 编入 `rmcp` 的 `server` 特性（只影响测试构建），换来「一个二进制既是测试驱动又是被测 server」的可控剧本。
- **约束**：
  - `mock-mcp` 不出现在 `--help`（隐藏），也不作为公开入口记录在文档里。
  - `rmcp` 的 `server` 特性只在 `bin-tests` 启用（`agent-mcp` 与正式构建不含）。
  - 剧本通过参数选择，不读「测试专用环境变量」以外的隐式状态。
- **承接任务**：P4-017、P4-018。

---

## D-P4-022 错误文案单一产点与脱敏边界

- **状态**：`DECIDED`
- **问题**：传输层错误有大量原文（URL、header、命令行、JSON-RPC 错误体），哪些能给模型、哪些只能进日志？
- **背景与证据**：
  - P3 的口径：`ToolError` 的 `Display` 只产出模型可读文本，不携带路径细节、URL、栈（`agent-tool/src/error.rs`）。
  - `AGENTS.md` 的测试数据采集条款要求「敏感信息不得写进任何输出」。
  - HTTP transport 的 `headers` 里可能有 `Authorization`。
- **可选方案**：
  1. 所有 MCP 错误文案集中在 `agent-mcp/src/error.rs`，只带 `server_id` / 工具名；原文只进 `tracing::debug`；
  2. 直接把 rmcp 的 `ServiceError` 文本透给模型；
  3. 按错误类型分别透传「安全的部分」（如 HTTP 状态码）。
- **决策**：①。
- **理由**：
  - ②会把 header、URL、内部栈交给模型，既泄漏也可能被模型复述给用户。
  - ③的收益（多一个状态码）不值得为每类错误单独设计脱敏规则。
  - ①的代价是排查时需要看 debug 日志，但这是唯一不会泄漏的边界。
- **约束**：
  - 文案表以常量集中定义，判据 E-1 / E-3 逐条比对。
  - 事件、审计、接口响应里不得出现 header 值、命令行、URL（判据 E-2）。
  - `McpError` → `ToolError` 只落 `Failed`（超时由执行器判定）。
- **承接任务**：P4-009、P4-018。

---

## D-P4-023 本阶段不改 `agent-runtime`

- **状态**：`DECIDED`
- **问题**：MCP 工具进入执行循环后，`agent-runtime` 要不要改？
- **背景与证据**：
  - 运行时的工具入口只有三处：`loop_/tool.rs::definitions` 调 `registry.model_definitions()`、`execute_batch_of` 调执行器、`file_access` 调 `registry.lookup`。
  - 三处都以 `Arc<ToolRegistry>` 为入口，注册表内部可变（D-P4-004）后语义不变。
  - P3 的硬线是「Runtime 不认识具体工具」，MCP 工具只是另一种 `Arc<dyn Tool>`。
- **可选方案**：
  1. `agent-runtime` 零改动（只允许因 `lookup` 返回类型变化而调整一行借用写法）；
  2. 在运行时里加 MCP 专属分支（如「MCP 工具的超时另算」）；
  3. 在运行时里缓存工具清单。
- **决策**：①。
- **理由**：
  - ②违反 P3 的核心约束，且 MCP 的差异（超时、取消、截断）已经由执行器统一处理，没有需要运行时特判的地方。
  - ③与注册表的实时快照冲突（缓存会延迟工具变更的可见性），且 P3 每轮都要重新取清单，没有性能问题。
- **约束**：
  - `grep -rniE "mcp|rmcp" crates/agent-runtime/src` 为空（判据 A-3 的配套检查）。
  - 若因返回类型调整必须改一行，需在验收报告中单列并说明「仅签名适配」（判据 A-5 的口径）。
- **承接任务**：P4-003（返回类型调整）、P4-011（装配接入）、P4-018（回归）。
