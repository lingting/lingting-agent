# P3 技术决策记录

**状态：规划期草稿（开工前定稿）。** 本文件记录 P3 的阶段级决策，编号 `D-P3-xxx`；被否决与被修订的方案保留记录，不删除。

任务分解见 [tasks.md](tasks.md)，判据见 [acceptance.md](acceptance.md)，阶段边界见 [README.md](README.md) §二。

每项记录包含：问题 / 背景与证据 / 可选方案 / 决策 / 理由 / 约束 / 承接任务。

状态定义：

| 状态 | 含义 |
|------|------|
| `DECIDED` | 已确定，可直接执行 |
| `DECIDED (待验证)` | 方向已定，但依赖某个验证任务确认细节 |
| `OPEN` | 未确定，动手前必须讨论 |
| `REJECTED` | 已否决（保留记录，不删除） |
| `DEFERRED` | 明确推迟到后续阶段 |

---

## 决策索引

| ID | 主题 | 状态 |
|---|---|---|
| D-P3-001 | 内置工具与 Tool 抽象同处 `agent-tool`，不新建 crate | DECIDED |
| D-P3-002 | 参数 schema 的单一来源：`schemars` 派生 + serde 反序列化校验 | DECIDED |
| D-P3-003 | 工具调用与结果进 `messages` 表与对话历史，裁剪按工具调用组成对丢弃 | DECIDED |
| D-P3-004 | `confirm` 走完整确认交互：`api/client/post` + 挂起 + 唤醒 | DECIDED |
| D-P3-005 | 内置工具的默认权限等级与「拒绝的工具不进上下文」 | DECIDED |
| D-P3-006 | 一轮内的并行语义与结果顺序 | DECIDED |
| D-P3-007 | 工具结果的截断口径 | DECIDED |
| D-P3-008 | 重试：默认 0 次，只有工具自己声明可重试 | DECIDED |
| D-P3-009 | 工具轮次上限 `max_tool_rounds` 与超限结局 | DECIDED |
| D-P3-010 | Run 事件扩展：三个事件 + 六个可选字段 | DECIDED |
| D-P3-011 | 审计表结构、脱敏规则与 `service-core` 对 `agent-tool` 的依赖 | DECIDED |
| D-P3-012 | 审计查询接口的形态与路径 | DECIDED |
| D-P3-013 | `file_read` / `file_search` 的安全边界 | DECIDED |
| D-P3-014 | `http_request` 的安全边界 | DECIDED |
| D-P3-015 | `calculator` 的实现口径（自研递归下降） | DECIDED |
| D-P3-016 | `current_time` 的实现口径（`utc` / `local`） | DECIDED |
| D-P3-017 | 客户端展示口径（TUI 一行 / WebUI 卡片） | DECIDED |
| D-P3-018 | HTTP 工具走 `framework-http`（`reqwest` 特性） | DECIDED |
| D-P3-019 | 取消的传递方式：`select!` + drop future | DECIDED |
| D-P3-020 | 工具项在 Context 里的裁剪口径 | DECIDED |
| D-P3-021 | 工具类型声明本地文件操作行为（只读 / 只写 / 读写 / 不操作），用于推导并发默认值与审计 | DECIDED |
| D-P3-022 | `api/client/update` 重命名为 `api/client/post`，并承载工具确认 | DECIDED |
| D-P3-023 | 挂起的落地口径：回 `QUEUED` + 新列、让出许可、不阻塞同会话、不设超时、重连补发 | DECIDED |
| D-P3-024 | 每轮边界落库（修订 D-P2-008 的「流式期间不写库」） | DECIDED |
| D-P3-025 | 工具轨迹在 `messages` 里的配对与裁剪口径 | DECIDED |
| D-P3-026 | 确认交互的客户端形态（TUI 一行 + `y`/`n`；WebUI 卡片 + 两个按钮） | DECIDED |
| D-P3-027 | 挂起态改用 `WAITING` 状态（修订 D-P3-023 的「回 `QUEUED`」） | DECIDED |
| D-P3-028 | 回归判据 R-1 的口径：请求体默认携带工具定义 | DECIDED |

---

## D-P3-001 内置工具与 Tool 抽象同处 `agent-tool`

- **状态**：`DECIDED`
- **问题**：五个内置工具放在哪里？与 Tool 抽象同一个 crate，还是单独一个 `agent-tool-builtin`？
- **背景与证据**：
  - `docs/workspace.md` §3.2 已规划 `agent-tool`（Tool Trait / Definition / Schema / Registry / Executor / Result / Permission / Execution Context / Error），没有规划「内置工具」的落点。
  - `AGENTS.md` 第 16 / 17 条要求新增依赖必须考虑职责；新增 crate 属于结构变更，必须同步 `docs/workspace.md`。
  - P2 的 A-7 判据把「不存在 `agent-tool`」当成 P2 时点的结论，本阶段新增该 crate 是规划内的。
- **可选方案**：
  1. 全部放 `agent-tool`（`builtin` 子模块），HTTP 依赖 `framework-http`；
  2. 抽象放 `agent-tool`，五个内置工具放新 `agent-tool-builtin`；
  3. 抽象放 `agent-tool`，内置工具直接放装配层 `lib-bin-web`。
- **决策**：①。
- **理由**：
  - ②的收益是「抽象 crate 不拉 reqwest」，但本项目所有消费方（`bin-agent` 经 `agent-route`）本来就有 reqwest，收益接近于零，代价是多一个 crate、多一条文档约束、多一次依赖方向核对。
  - ③会让内置工具离领域层太远（工具实现属于领域能力，不属于装配），P4 的 MCP 适配还需要一个「参考实现」在旁边。
  - 子模块边界已经给出足够的隔离：`tool/` 放抽象，`builtin/` 放实现，`builtin` 之外不出现 `std::fs` / HTTP 调用。
- **约束**：
  - `agent-tool` 的依赖清单里不得出现 `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `lib-web*` / `bin-*`（判据 A-1）。
  - 若 P4 出现「工具数量膨胀到需要分 crate」的实证，再拆一次并同步 `docs/workspace.md`。
- **承接任务**：P3-001。

---

## D-P3-002 参数 schema 的单一来源：`schemars` 派生 + serde 反序列化校验

- **状态**：`DECIDED`（需求方定案：采用方案①）
- **问题**：给模型看的工具参数 JSON Schema 从哪里来？校验用什么？
- **背景与证据**：
  - P3 README 的要求是「从 Rust 类型生成参数 schema，避免手写 JSON Schema」「Serde + JSON Schema：参数与 schema 的单一来源（具体库在阶段内选型）」。
  - `Cargo.lock` 里已有 `schemars 1.2.3`（`rig-core` 的传递依赖），引入它不新增第三方依赖面。
  - 现有代码里没有任何 JSON Schema 表达式；`specta` 只用于 SDK 生成，不产出 JSON Schema。
- **可选方案**：
  1. `schemars` 1.2 派生 `JsonSchema` + `serde_json::from_value` 校验（不引 `jsonschema`）；
  2. `schemars` 派生 + `jsonschema` crate 做正式校验；
  3. 自研一套「参数描述宏」生成 schema 与解析代码；
  4. 手写 JSON Schema + 手写 serde 结构（两处维护）。
- **决策**：①。
- **理由**：
  - ②会引入第二套校验规则：同一份参数既有 serde 的类型约束、又有 JSON Schema 的约束（`oneOf` / `format` / 边界），两者一旦不一致就会出现「schema 说可以、执行说不行」，模型侧无从判断。参数校验只需要「能不能反序列化」，serde 已经足够。
  - ③是自造轮子，维护成本高于直接用 `schemars`。
  - ④直接违反「单一来源」，是本次要消除的问题本身。
  - ①的代价是 schema 与 serde 的宽松度差异（如 `Option` 与 `default`），这一点写进 `schema.rs` 的注释并用同一条用例两头对照（判据 S-2）。
- **约束**：
  - `schemars` 只负责生成，不参与校验；全仓不得出现「用 JSON Schema 校验参数」的第二条路径。
  - schema 输出面向模型，需要剥掉 `$schema` / `title` / `$defs` 之类与「让模型填参数」无关的键。
- **承接任务**：P3-003（实现）、P3-030（`tool.schema` 用例）。

---

## D-P3-003 工具调用与结果进 `messages` 表与对话历史

- **状态**：`DECIDED`（需求方定案：采用方案②）
- **问题**：一次 Run 里的工具调用、工具结果要不要写进 `messages` 表、要不要在后续轮次的历史里出现？
- **背景与证据**：
  - P2 的 `ContextBuilder::history_item` 显式过滤 `SYSTEM` / `TOOL`（D-P2-013），裁剪规则是「整条消息丢弃最旧」。
  - P2 的终态边界是「流式期间不写库，助手消息与 `COMPLETED` 同事务提交」（D-P2-008）。
  - `0001_init.sql` 的 `messages` 表没有工具相关列；`MessageRole::Tool` 取值已存在但没有写入路径。
- **可选方案**：
  1. 不落 `messages`：本轮工具轨迹只存在于内存 Context 与审计表；
  2. **落 `messages`**：新增工具列，`TOOL` 角色消息进入历史，Context 裁剪改为「按工具调用组成对丢弃」；
  3. 折中：只存「最终助手消息的工具摘要」，不存 tool 结果。
- **决策**：②。
  - `messages` 追加三列：`tool_call_id`（TOOL 消息必有）、`tool_name`（TOOL 消息必有）、`tool_calls`（仅 ASSISTANT 消息，存本轮请求的调用 JSON）。
  - `Message` 契约追加三个可选字段（`toolCallId` / `toolName` / `toolCalls`），随 SDK 生成同步；P2 的既有字段与语义不变。
  - `ContextBuilder` 不再过滤 `TOOL`：工具轨迹按「assistant `tool_calls` → 各 TOOL 结果」的顺序进入请求（D-P3-025 定义配对与裁剪）。
  - 落库时机：**每轮边界**（进入下一轮之前）——见 D-P3-024，它修订了 D-P2-008 的「流式期间不写库」。
- **理由**：
  - 只有落库，挂起 / 唤醒后的推理恢复才有事实源（D-P3-023 的恢复依赖它）；否则要么重跑已执行的工具，要么在服务端另存一份轨迹。
  - 跨 Run 的历史里保留工具痕迹，模型在下一轮对话中能看到「上次查过什么」，这是用户选择方案②的直接收益。
  - 代价是 P2 最精细的两处逻辑（终态事务边界与历史裁剪）都要动，因此 D-P3-024 / D-P3-025 必须同时定稿，且 R-3 回归判据要覆盖「成功 Run 必有 1 条助手消息」这类既有断言的新形态。
- **约束**：
  - `TOOL` 消息必须带 `tool_call_id`，且该 id 必须能在同一会话的历史里找到对应的 assistant `tool_calls`（配对不成立即数据损坏，按 `FAILED` 处理）。
  - assistant 消息带 `tool_calls` 时 `content` 允许为空串；空内容 + 无 tool_calls 的消息不允许写入。
  - 裁剪、配对、顺序的全部规则集中在 `agent-context` 一处，Repository 与 `agent-runtime` 不重复实现。
- **承接任务**：P3-012（Context 配对与裁剪）、P3-014（每轮边界落库）、P3-017（迁移）、P3-018（写入与查询）、P3-020（SDK 同步）。

---

## D-P3-004 `confirm` 走完整确认交互：`api/client/post` + 挂起 + 唤醒

- **状态**：`DECIDED`（需求方定案：采用方案③，并追加两条方案调整）
- **问题**：「需确认」的权限在 P3 怎么落地？
- **背景与证据**：
  - P3 阶段级 README 曾把它列为「隐性复杂度来源」：需要新的 SSE 事件 + 客户端交互 + 服务端等待应答（落地口径见 README §4.9 与 §七 第 1 条）。
  - 现有下行通道是单向的 `api/client/stream`；上行只有 `POST api/client/update`（`PING` / `INQUIRY_ACK`），没有「按请求应答」的能力。
  - 服务端等待客户端确认时，Run 若继续持有全局并发许可，会直接吃掉并发位（`max_concurrent_runs` 默认 `3C`）。
- **可选方案**：
  1. 三值枚举，`Confirm` 落为「拒绝 + 固定文案」；
  2. P3 只支持 `Allow` / `Deny`，配 `confirm` 即启动期报错；
  3. **完整实现确认交互**（新事件 + 新上行接口 + 挂起与唤醒）。
- **决策**：③，并按需求方给出的两条调整落地：
  1. **上行接口改名为 `POST api/client/post`**（原 `api/client/update`），同时承载心跳与工具确认；`ClientUpdateKind` → `ClientPostKind`，追加 `TOOL_CONFIRM` 取值，请求体追加可空的 `toolCallId` 与 `decision`（`APPROVE` / `REJECT`）。
  2. **Run 下发确认事件后挂起，不占用 Run 位**：Run 回 `QUEUED`（`runs.pending_tool_call_id` 非空），释放全局并发许可、结束本次 `drive`；客户端经 `api/client/post` 回传决定后清空待确认列并重新排队，恢复推理并执行 / 回灌拒绝。
- **理由**：
  - 需求方明确要真正的确认闭环，而不是「退化为拒绝」；这一条同时决定了 P3 必须改 `Run` 的生命周期（多一个「挂起」的语义）与上行接口。
  - 「不占用 Run 位」把挂起的代价从「吃掉一个并发位直到有人点按钮」变成「只是数据库里多一条待唤醒记录」，是这套机制能默认打开的前提。
  - 接口改名把「心跳」与「确认」收进同一条上行通道，避免为确认再开一条接口（接口总数不增）。
- **约束**：
  - 挂起 / 唤醒的全部细则（状态、许可、会话队列、超时、重连补发、审计中间态）见 D-P3-023；接口改名的影响面见 D-P3-022。
  - 挂起期间不发 `tool_call_completed`：这次调用还没有结果，客户端只收到了 `tool_call_confirm_required`。
  - 拒绝文案固定：「用户拒绝调用该工具」，模型可读、不含内部细节。
- **承接任务**：P3-005（三值枚举）、P3-006（执行器返回「需确认」信号）、P3-013（事件）、P3-014（挂起点）、P3-017 / P3-018（挂起列与唤醒）、P3-027（服务端挂起与唤醒）、P3-028 / P3-029（客户端确认）。

---

## D-P3-005 内置工具的默认权限等级

- **状态**：`DECIDED`（需求方定案）
- **问题**：五个内置工具默认允许还是默认拒绝？`FileRead` / `FileSearch` 的默认可读范围到哪里？被拒绝的工具要不要出现在模型上下文里？
- **背景与证据**：
  - 现有配置面只有 `[server]` / `[runtime.events]` / `[provider]` / `[agent]`，没有任何「危险能力开关」的先例。
  - `AGENTS.md` 的搜索纪律（禁止无范围递归遍历、遍历期剪枝）与 `file_search` 的实现口径同源。
- **决策**：
  - **`http_request` 默认拒绝**（唯一默认关闭的内置工具）；`calculator` / `current_time` / `file_read` / `file_search` 默认开启。
  - `file_read` / `file_search` 的白名单为空时只允许应用根目录（`AGENT_HOME`）。
  - **被拒绝的工具不进入上下文**：`deny` 的工具不出现在 `model_definitions()`（模型可见清单），因此也不会进入任何一次 Completion 请求；执行期仍保留二次判定（模型凭空构造名字 → `DENIED`）。
- **理由**：
  - `http_request` 是唯一会主动向外部发起请求的能力（SSRF 面最大），默认关闭符合「危险能力显式授权」。
  - 其余四个只读或纯函数，且文件类工具受白名单约束，默认开启能让「自主选择工具」的验收与开箱体验都成立。
  - 「拒绝的工具不进上下文」把权限前置到清单生成：模型不会尝试一个它看不见的工具，减少一轮无意义的工具失败。
- **约束**：
  - `confirm` 的工具**仍然进上下文**（模型可以请求，只是要等确认）；只有 `deny` 才被摘掉。
  - 默认值不是安全承诺：README §七 第 8、9 条写明白名单不是沙箱，`http_request` 不是 SSRF 防护。
  - `[tools.permissions]` 显式写了 `allow` 才覆盖默认 deny；显式 `deny` 永远优先（判定顺序见 README §4.3）。
- **承接任务**：P3-005、P3-023、P3-024、P3-025。

---

## D-P3-006 一轮内的并行语义与结果顺序

- **状态**：`DECIDED`
- **问题**：模型一轮内给出多个工具调用时，怎么执行、怎么排序、副作用工具怎么办？
- **背景与证据**：
  - 验收方向 3 要求「一轮内多个工具可并行执行，结果顺序稳定且可复现」。
  - README §4.4 需要把「同一工具并发调用是否允许」「结果如何排序」「副作用工具如何串行化」定下来，否则执行器无法实现。
  - 五个内置工具全是只读或纯函数，没有真正的副作用工具——但 `ToolConcurrency` 的语义要现在定下来，否则 P4 的 MCP 工具没有位置。
- **决策**：
  - 并发：一轮内的 `Parallel` 调用并发执行，并发上限 `[tools] max_parallel_calls`（默认 8），用 `buffered(n)` 而不是一次性 `join_all`。
  - 排序：结果**永远**按模型给出的调用顺序回填，与完成顺序无关。
  - 串行：`Exclusive` 调用把这一轮切成段，段间串行、段内并行。
  - 同一工具并发调用：允许（并发性由工具自己声明，不按名字特判）。
- **理由**：
  - 顺序由输入决定是唯一可复现的口径；按完成顺序回填会让「模型看到的结果顺序」随机器负载漂移，用例无法稳定断言。
  - `buffered` 比 `join_all` 多一个上限，代价可忽略，收益是「8 个 HTTP 调用」不会瞬间打出几十个连接。
  - 段划分比「全局串行」更细：只读工具照常并行，副作用工具不被别的工具夹在中间。
- **约束**：
  - `tool_call_started` 按调用顺序发（在执行前），`tool_call_completed` 按**完成**顺序发（这是事实），客户端按 `toolCallId` 配对。
  - 审计记录的写入顺序不做保证，查询按 `created_at_ms`、`id` 排序。
- **承接任务**：P3-007、P3-015、P3-030。

---

## D-P3-007 工具结果的截断口径

- **状态**：`DECIDED`
- **问题**：大文件 / 大响应怎么截断？模型怎么知道被截断了？
- **背景与证据**：Context 预算按字符估算（`HeuristicEstimator`），一个 1 MiB 的文件足以吃掉整个窗口；README §4.4 要求「大文件 / 大响应的截断策略与提示方式」有明确口径。
- **可选方案**：
  1. 按字符截断 + 固定提示「（结果已截断，原文共 N 字符）」，审计里存同样的截断文本；
  2. 按 token 截断（需要 tokenizer，P2 已明确不引入）；
  3. 让工具自己决定（每个工具一套口径）；
  4. 截断后不留提示，静默丢弃。
- **决策**：①。
- **理由**：
  - ②在 P2 已被推迟（D-P2-012 第 4 条），本阶段不为截断引入分词器。
  - ③会让「模型知道不知道被截断」取决于工具作者，无法统一断言。
  - ④会让模型把半截内容当全量，是最糟的失败模式。
  - ①的固定提示可以放进单一产点，用例按文案断言。
- **约束**：
  - 截断按**字符**而非字节，中文与 emoji 不被截半。
  - 提示文本里带原文长度，让模型知道「还差多少」。
  - 审计存同样的截断文本，不留全量（README §七 第 7 条）。
  - 上限 `[tools] max_result_chars`（默认 8000），按字符计。
- **承接任务**：P3-008、P3-030。

---

## D-P3-008 重试：默认 0 次，只有工具自己声明可重试

- **状态**：`DECIDED`
- **问题**：工具失败要不要重试？谁决定？
- **背景与证据**：P3 README §二「Timeout / Retry」要求「单工具超时、可重试错误的重试策略、整体上限」；`agent-route` 的 Provider 重试在 P2 也没有实现（失败即回灌）。
- **可选方案**：
  1. 统一在 Executor 里按错误类别重试（例如所有网络错误重试 2 次）；
  2. 工具声明 `retryable(&error)`，Executor 按 `[tools] max_retries` 执行，默认 0；
  3. 完全不重试。
- **决策**：②，默认 `max_retries = 0`（等于③的默认行为）。
- **理由**：
  - ①会让「同一个参数非法」这类确定性失败也被重试，纯属浪费 token 与时间。
  - ③把「重试」这件事彻底排除，将来真实需要时还得改 Executor 的签名。
  - ②把判断交给最了解错误语义的一方（工具），Executor 只负责计数与上限；默认 0 保证行为与③一致，不改变任何既有语义。
- **约束**：
  - 重试次数计入 `duration_ms`，不额外产出事件（事件只在最终结果上发一次）。
  - 超时不重试（超时已经是「等太久」的信号）。
  - P3 内置工具里只有 `http_request` 对「连接失败 / 读超时」声明可重试。
- **承接任务**：P3-006、P3-025。

---

## D-P3-009 工具轮次上限 `max_tool_rounds` 与超限结局

- **状态**：`DECIDED`
- **问题**：多轮工具循环的终止条件是什么？超限怎么收尾？
- **背景与证据**：现有 `[agent]` 段已有 `run_queue_timeout_ms` / `llm_idle_timeout_ms` 两个「防挂起」参数；模型可能陷入「一直调工具」的循环。
- **决策**：
  - 新增 `[agent] max_tool_rounds`（默认 8），语义是「一次 Run 内最多几轮 Provider 请求」，第一轮也算一轮。
  - 用尽轮次且最后一轮仍要求调用工具 → Run 落 `FAILED`，`error` 固定文案「工具调用轮次超过上限」，不写助手消息。
  - 不做「时间预算」类的第二重限制：单工具已有超时，整体时长由排队参数与工具超时共同约束。
- **理由**：
  - 轮次是唯一稳定可复现的上限；时间上限在慢机器上会把「正常但慢」的 Run 误杀。
  - 落 `FAILED` 而不是「截断成最后一次文本」：静默截断会让用户以为任务完成，而实际没完成。
- **约束**：
  - 固定文案写进代码常量，用例按文案断言。
  - 配置值为 0 由装载校验拒绝（至少 1 轮）。
- **承接任务**：P3-005、P3-014、P3-030。

---

## D-P3-010 Run 事件扩展：三个事件 + 六个可选字段

- **状态**：`DECIDED`
- **问题**：工具调用怎么进事件流？
- **背景与证据**：`RunEvent` 是扁平结构 + 若干 `Option` 字段（P1 定稿，P2 未改）；`RunEventKind` 注释已预留 `tool_call_started`；客户端侧 `worker.rs` 的匹配是穷尽的。
- **可选方案**：
  1. 追加三个事件（`tool_call_started` / `tool_call_completed` / `tool_call_confirm_required`）+ 六个可选字段（`tool_call_id` / `tool_name` / `tool_arguments` / `tool_status` / `tool_duration_ms` / `tool_round`）；
  2. 复用 `message_delta` 承载工具信息（塞进 `delta`）；
  3. 嵌套一个 `tool` 子对象。
- **决策**：①。
- **理由**：
  - ②会让 `delta` 的语义变成「有时是文本、有时是 JSON」，客户端与用例都被迫解析。
  - ③与 P2 的扁平结构约定冲突（一次为工具改动结构，后续所有事件都要解释「为什么这里有个子对象」）。
  - ①的代价是 `RunEvent` 变宽，但它是「可选字段的扁平载荷」，与既有风格一致，且兼容性最好的做法就是只追加。
- **约束**：
  - 既有字段的名字、类型、语义一律不变；`error` 复用（工具失败也用它）。
  - `ToolCallStatus` 的取值与 `agent-tool` 的审计状态同源，两侧只允许一个定义点（`agent-tool` 定义，`agent-core` 用同一个枚举或显式映射）。
  - SSE 的 `event:` 行与 JSON 的 `event` 字段必须同值（`serde` 与 `strum` 两处同时改）。
- **承接任务**：P3-013、P3-020、P3-028、P3-029。

---

## D-P3-011 审计表结构、脱敏规则与 `service-core` 对 `agent-tool` 的依赖

- **状态**：`DECIDED (待验证)`
- **问题**：审计记录落在哪张表？谁负责脱敏？`service-core` 要不要依赖 `agent-tool`？
- **背景与证据**：
  - `0001_init.sql` 的约定：枚举列存 serde wire 值、不加 `CHECK`；`*_at_ms` 用 INTEGER。
  - 依赖方向：`service-core` 是通用业务层，理论上不该依赖 Agent 领域 crate；但 `RunDriver` 端口已经在 `service-core`，它已经依赖了 `agent-core`（`Run` / `RunEvent`）。
  - 工具参数里可能含敏感信息（`api_key` / `token` / `Cookie`），审计记录必须脱敏（README §4.7 的脱敏规则）。
- **决策**：
  - 表：`tool_calls`（列见 README §4.7），`UNIQUE(run_id, tool_call_id)` 保证重放幂等，`ON DELETE CASCADE` 挂在 `runs` 上。
  - 脱敏在**写出侧**（`agent-tool` 的 `redact_arguments`），落库的是已脱敏文本；查询接口不做二次处理。
  - `service-core` 新增对 `agent-tool` 的依赖（`agent-tool` 不依赖 `service-core`，无环）。若核对后发现这条边不可接受，备选是在 `service-core` 定义同构记录、由 `agent-runtime` 转换——两者都只允许存在一种，不允许两套并存。
- **理由**：
  - 脱敏放在写出侧是唯一正确的时机：一旦明文进了数据库，事后清洗不可能彻底（`AGENTS.md` 的「敏感检测在写出侧」同源）。
  - `UNIQUE + ON CONFLICT DO NOTHING` 让「同一 `tool_call_id` 写两次」成为无害操作，取消 / 重试路径不需要额外协调。
  - `service-core` 已依赖 `agent-core`，再加 `agent-tool` 不改变分层性质（都是 Agent 领域的基础类型），但这条边必须写进 `docs/workspace.md`。
- **约束**：
  - SQL 只在 `repository/`；写入点只有一个（`agent-runtime` 调 `SessionService`）。
  - 审计写入失败只记 `warn!`，不改变 Run 结论。
  - 审计写入不得在会话锁内（P2.1 的口径）。
  - `service-core → agent-tool` 的依赖要写进 `docs/workspace.md` 的分层说明。
- **承接任务**：P3-009、P3-017、P3-018。

---

## D-P3-012 审计查询接口的形态与路径

- **状态**：`DECIDED`
- **问题**：审计记录怎么查？要不要分页？要不要按会话查？
- **背景与证据**：验收方向 5 只要求「按 Run 查询」；`docs/develop.md` 的 path 规范是「资源段小写单数 + 动作段固定动词」。
- **可选方案**：
  1. `GET api/tool/audit/list?runId=…`（新增领域 `tool`）；
  2. 复用 `api/run/get` 把审计塞进 `Run` DTO；
  3. `GET api/run/tool/list?runId=…`（挂在 run 领域下）。
- **决策**：①。
- **理由**：
  - ②会让 `Run` 变成「会话业务 + 工具审计」的混合 DTO，且 `run/get` 的载荷会随工具数量膨胀。
  - ③在语义上也说得通，但「工具」本身是一个会成长的领域（P4 MCP、P5 Skill 都要查工具），独立领域文档 `docs/api/tool.md` 更合适。
  - 无分页：单 Run 的记录条数上限由 `max_tool_rounds × 并发上限` 约束（默认 8 × 8 = 64），远小于需要分页的量级。
- **约束**：
  - 未知 `runId` 返回 `[]`（不是 404）：审计查询按「没有记录」表达，与「Run 不存在」区分不了是 P3 的取舍。
  - `runId` 缺失返回 400（查询参数必填）。
  - 接口总数从 25 变 26（不含 debug + `metrics` 限定的 `api/probe/metrics`）。
- **承接任务**：P3-019、P3-020。

---

## D-P3-013 `file_read` / `file_search` 的安全边界

- **状态**：`DECIDED`
- **问题**：路径白名单怎么判？要不要跟随符号链接？搜索怎么剪枝？
- **背景与证据**：
  - `AGENTS.md` 的「搜索与 IO 约束」要求排除目录必须在**遍历阶段**剪枝，不允许事后过滤。
  - Windows 上路径大小写不敏感、盘符与短路径名（`PROGRA~1`）都会干扰字符串比较。
- **决策**：
  - 白名单是「根目录列表」，判定用 `canonicalize()` 之后的 `Path::starts_with`（按路径组件比较，不做字符串前缀）。
  - 白名单为空时只允许应用根目录（`AGENT_HOME`）。
  - 不跟随符号链接目录（`file_search`），`file_read` 由 `canonicalize` 承担解析（符号链接指向白名单外则被拒绝）。
  - `file_search` 遍历期剪枝固定排除目录列表（与 `AGENTS.md` 的默认排除集一致），深度上限 8，结果上限默认 50 / 硬上限 200。
  - `file_read` 先看 `metadata().len()` 再读；超限不读入内存。
  - 错误文案统一，不暴露真实绝对路径。
- **理由**：
  - `canonicalize` + 组件比较是「不做沙箱」前提下最不容易被绕过的做法；字符串前缀比较会把 `/data/agent-tool-evil` 判成在白名单 `/data/agent-tool` 内。
  - 遍历期剪枝是纪律要求，也顺带解决性能问题。
  - 不跟随符号链接目录是防止环与逃逸的最低成本手段。
- **约束**：
  - 这不是沙箱（README §七 第 8 条）：不做 ACL、不做挂载点检测。
  - 排除目录列表是代码常量，不做配置项（配置化会让「搜索纪律」变成可选项）。
- **承接任务**：P3-023、P3-024。

---

## D-P3-014 `http_request` 的安全边界

- **状态**：`DECIDED`
- **问题**：域名白名单怎么判？允不允许重定向？响应多大？
- **背景与证据**：README §4.8 要求 `http_request` 的域名与超时限制有明确口径；`framework-http` 的客户端支持 `no_redirect()` / 超时。
- **决策**：
  - scheme 只允许 `http` / `https`。
  - host 必须命中 `[tools.http] allow_hosts`（允许写 `host` 或 `host:port`），白名单为空即一律拒绝。
  - 禁止跟随重定向（`no_redirect()`），3xx 原样返回状态码。
  - 只允许 `GET` / `POST`。
  - 响应体上限 `[tools.http] max_bytes`（默认 256 KiB），超限截断并标注。
  - 只回灌状态码、内容类型与正文；不回灌响应头（避免把 `Set-Cookie` / `Authorization` 之类带进模型上下文）。
  - 默认权限 deny（D-P3-005）。
- **理由**：
  - 允许重定向等于把「白名单 host」变成「白名单 host 愿意跳到哪里」，SSRF 防护形同虚设。
  - 只读回正文是「最小回灌」原则：响应头对完成任务没有价值，泄漏风险却不小。
  - 默认 deny 让这条能力必须显式开启，符合「危险能力显式授权」。
- **约束**：
  - 这不是 SSRF 防护（README §七 第 9 条）：DNS 重绑定、白名单域名自身的内网跳转都在防护范围之外。
  - 不发请求头定制（P3 只需要 GET 取内容、POST 提交 JSON）。
- **承接任务**：P3-025。

---

## D-P3-015 `calculator` 的实现口径

- **状态**：`DECIDED`
- **问题**：表达式求值自己写还是引库？
- **背景与证据**：`AGENTS.md` 与 P2.2 的口径是「不新增第三方依赖」；表达式求值库（`evalexpr` / `meval`）会带来「函数集、变量、浮点行为」的不确定面。
- **决策**：自研递归下降，只支持 `+` `-` `*` `/` `%`、括号与一元正负号；输入长度上限 512 字符，嵌套深度上限 32；结果整数优先、否则最多 10 位小数去尾零。
- **理由**：
  - 求值范围越小，安全性越容易论证；P3 只需要一个「能算数」的工具来验证工具循环，不需要函数库。
  - 自研代码量约一百多行，比引入依赖 + 配置函数白名单更省。
- **约束**：
  - 不做幂、函数、变量、常量标识符；拒绝时返回固定文案。
  - 除零、语法错误、超长表达式都是 `ToolError::Failed` 或 `InvalidArguments`，不 panic。
- **承接任务**：P3-021。

---

## D-P3-016 `current_time` 的实现口径

- **状态**：`DECIDED`
- **问题**：要不要支持任意时区名？
- **背景与证据**：任意时区名需要 tzdata（`chrono-tz` 或系统库），会新增依赖与平台差异；`lib_core::time::now_millis` 已提供毫秒时间。
- **决策**：只支持 `utc` / `local` 两个取值，默认 `utc`；输出「ISO8601（Unix 毫秒 + 时区标记）」；非法取值返回 `InvalidArguments`。
- **理由**：P3 只需要一个「读取环境信息」的工具；任意时区名属于「模型自己能算」的范畴，不值得为它引 tzdata。
- **约束**：
  - 不引入新依赖（`chrono` 已在 workspace 依赖）。
  - 毫秒与 `created_at_ms` 同源（都用 `lib_core::time`）。
- **承接任务**：P3-022。

---

## D-P3-017 客户端展示口径

- **状态**：`DECIDED`
- **问题**：TUI 与 WebUI 怎么展示工具调用？要不要做「参数/结果」详情？
- **背景与证据**：TUI 的对话区是「增量文本流」，WebUI 的 `useChat.ts` 按事件累加消息；工具参数与结果可能是长 JSON。
- **决策**：
  - TUI：对话区一行一条（`→ 正在调用 calculator` / `✓ calculator（12ms）` / `✗ http_request：工具未授权`），不新增面板与快捷键。
  - WebUI：消息气泡之间的紧凑卡片，只显示工具名、状态、耗时；不展示参数与结果全文。
  - 两者都按 `toolCallId` 配对；Run 结束时未配对的 `started` 标为「未完成」。
- **理由**：
  - 展示的目的是「用户知道 Agent 在干什么、卡在哪」，不是「调试工具参数」；参数与结果全文属于审计查询的职责。
  - 不做详情面板可以让这一批任务保持小体量，也避免与既有布局规范冲突。
- **约束**：
  - 沿用既有主题与间距，不新增设计 token（gpui/ratatui 的样式常量一律复用）。
  - 工具调用本身已随 `messages` 落库（D-P3-003 方案②），因此刷新后历史里也能看到工具行 / 卡片；**确认卡片不落库**（它只表达「此刻等一次确认」），刷新后不再出现，需要时由服务端补发。
- **承接任务**：P3-028、P3-029、P3-030。

---

## D-P3-018 HTTP 工具走 `framework-http`（`reqwest` 特性）

- **状态**：`DECIDED`
- **问题**：`http_request` 用什么发请求？直接用 `reqwest` 还是在 `agent-tool` 里定义端口？
- **背景与证据**：
  - `framework-http` 是本地自有框架，`lib-bin-cli` 的客户端已经在用（`reqwest` 特性）；`reqwest` 已在 workspace 依赖里。
  - 在 `agent-tool` 定义「HTTP 端口」再注入实现，会让内置工具无法独立构造（需要装配层传实现），与「工具是领域能力」的定位冲突。
- **可选方案**：
  1. 直接用 `framework-http::HttpClient`；
  2. 直接用 `reqwest`；
  3. 定义 `ToolHttpClient` 端口，实现放装配层。
- **决策**：①。
- **理由**：
  - ②绕过项目自有的框架封装（超时、代理、重定向策略都在 `framework-http` 里配好了），会出现两套 HTTP 客户端配置。
  - ③的注入点收益（可替换、可 mock）在 P3 用不上——用例用本地假服务端验证真实请求，比 mock 更有价值。
  - ①的代价是 `agent-tool` 依赖框架（不是第三方），与 `agent-route` 依赖 rig-core 的结构同源。
- **约束**：
  - `framework-http` 只在 `http_request` 内使用，其它工具与抽象层不出现 HTTP。
  - 不新增第三方依赖（`reqwest` / `url` 都已在 workspace）。
- **承接任务**：P3-001、P3-025。

---

## D-P3-019 取消的传递方式：`select!` + drop future

- **状态**：`DECIDED`
- **问题**：取消 Run 时怎么中断正在执行的工具？
- **背景与证据**：P2 的取消已经是「`select!` 命中即 drop 模型流」，工具执行沿用同一套模型；`RunCancel` 提供 `CancellationToken`（`tokio-util`）。
- **可选方案**：
  1. `select!` 与 `cancel.cancelled()` 并列，命中即 drop 工具 future；
  2. 给 `Tool::call` 传取消 token，由工具实现自己检查；
  3. 用 `AbortHandle` 中止任务。
- **决策**：①。
- **理由**：
  - ②要求每个工具都正确实现取消检查，漏一个就会残留后台任务（验收方向 4 直接不成立）。
  - ③需要把工具放到独立任务里，引入额外的任务管理与「中途 panic」处理。
  - ①的语义是「future 被 drop」，所有 `await` 点都会立即返回，`reqwest` 的请求随之关闭；代价是工具实现不能做「必须先收尾」的事——P3 的工具都不需要。
- **约束**：
  - 工具实现不感知取消（`Tool::call` 不带 token）。
  - 取消时补一条 `tool_call_completed`（状态 `CANCELLED`），保证客户端不悬挂「进行中」。
  - 超时与取消共用 `select!` 结构，但状态不同（`TIMEOUT` vs `CANCELLED`），结局不同（Run 继续 vs Run 终止）。
- **承接任务**：P3-006、P3-015。

---

## D-P3-020 工具项在 Context 里的裁剪口径

- **状态**：`DECIDED`（**已被 D-P3-003 方案②与 D-P3-025 修订**，保留原记录）
- **问题**：本轮的 assistant `tool_calls` 与 tool 结果进入 Context 后，会不会被裁剪？超预算怎么办？
- **背景与证据**：P2 的裁剪是「先按条数、再按预算，整条丢弃最旧，只作用于历史」（D-P2-013）。定案 D-P3-003 之前，工具轨迹只在内存里、不进历史。
- **原决策**：工具项不参与裁剪；整轮估算超预算时 Run 落 `FAILED`。
- **修订后的口径**（D-P3-003 方案② + D-P3-025）：
  - 工具轨迹**进历史**，因此**参与裁剪**，但以「工具调用组（assistant `tool_calls` + 其全部 TOOL 结果）」为单位：整组保留或整组丢弃。
  - 裁剪仍然只丢最旧的一端；组内条数照常计入 `trimmed_messages`。
  - 组边界不可切：不允许只丢 TOOL 留下 assistant `tool_calls`（上游会 400），也不允许孤儿 TOOL。
  - 估算仍用 `MESSAGE_TOKEN_OVERHEAD` + 正文；组作为整体参与 `available_tokens` 比较。
- **保留原因**：原决策的失败模式判断仍然有效——「预算不够就 `FAILED`，不静默丢结果」这条没有变，变的是「谁可以被丢」。真正的实现口径见 D-P3-025。
- **承接任务**：P3-012（实现）、P3-030（`tool.context` 用例）。

---

## D-P3-021 工具类型声明本地文件操作行为

- **状态**：`DECIDED`
- **问题**：一个工具「会怎么动本地文件」要不要在工具类型上显式声明？声明用来做什么？
- **背景与证据**：
  - `ToolRisk`（`ReadOnly` / `Network` / `Write`）是**风险等级**，描述的是「这个工具危险不危险」，不区分它操作的是本地文件还是网络。`http_request`（网络）与 `file_read`（本地只读）无法靠它区分。
  - `ToolConcurrency`（`Parallel` / `Exclusive`）落到实现里需要一个默认值来源；按工具名特判违反「Runtime 不认识具体工具」，让每个工具自己写死又容易漏。
  - P4 的 MCP 工具会从外部带进来（其 annotations 有 `readOnlyHint` 一类的语义），需要一个项目内部的分类名来对接。
- **可选方案**：
  1. 在 `Tool` 上声明 `file_access()`（`None` / `Read` / `Write` / `ReadWrite`），并只用它推导 `ToolConcurrency` 的默认值 + 记审计；
  2. 只作为元数据（进定义与审计，不改变任何行为）；
  3. 除并发默认外，还作为权限默认判定的依据（`Write` / `ReadWrite` 未显式配置时默认拒绝）。
- **决策**：①。取值 `ToolFileAccess::{None, Read, Write, ReadWrite}` 对应「不操作 / 只读 / 只写 / 读写」。
  - `default_concurrency()`：`None` / `Read` → `Parallel`，`Write` / `ReadWrite` → `Exclusive`。
  - `Tool::concurrency()` 的默认实现即返回 `default_concurrency(self.file_access())`，工具可显式覆盖。
  - 声明进 `ToolDefinition` 与审计记录（`tool_calls.file_access`、`ToolCallVO.fileAccess`），**不发给模型**（`LlmToolDefinition` 仍然只有 name / description / parameters）。
- **理由**：
  - ②让声明成为死数据：写了没人用，一改就会与实现漂移。而并发默认值的推导是纯函数、零风险，是这份声明最自然的第一个消费者。
  - ③把两条正交的判据（「操作什么」与「是否允许」）揉在一起，会让权限配置的语义变得难解释：用户配 `file_read = "deny"` 与 `http_request = "allow"` 时，没人希望「因为它是只读工具，所以默认允许」这种隐式规则。权限分类继续由 `[tools.permissions]` + `ToolRisk` 决定（D-P3-005 不变）。
  - ①对 P4 有直接价值：MCP 工具接入时，`readOnlyHint` 之类的标注可以映射成这个声明，从而自动获得「写类工具串行」的默认语义。
- **约束**：
  - 声明是**自述**，编译器不校验（README §七 第 14 条）：新增工具必须人工核对声明与实现是否一致，判据 T-8 覆盖。
  - `Write` / `ReadWrite` 只影响并发默认值与审计，不改变权限判定、不改变执行路径。
  - 内置工具的声明固定为：`calculator` / `current_time` / `http_request` → `None`，`file_read` / `file_search` → `Read`；P3 没有内置写工具。
  - 该字段不进模型可见的工具定义（模型不需要知道，也不该让它据此推断权限）。
- **承接任务**：P3-002（契约）、P3-007（并发分段）、P3-009 / P3-017 / P3-019（审计与查询载荷）、P3-021 ~ P3-025（五个工具各自声明）、P3-030（`tool.decl` 用例）。

---

## D-P3-022 `api/client/update` → `api/client/post`

- **状态**：`DECIDED`（来自 D-P3-004 的方案调整）
- **问题**：工具确认的上行消息放在哪里？
- **背景与证据**：
  - 现有上行只有 `POST api/client/update`（`ClientUpdatePO { clientId, kind }`，`kind` 取 `PING` / `INQUIRY_ACK`），响应体是 `ClientUpdateVO { pongAtMs }`。
  - 涉及调用点：`crates/lib-web/src/runtime/api.rs`（handler）、`crates/types-core/src/runtime/client.rs`（DTO）、`manager-agent` 的 `registry.rs` / `client.rs`（`heartbeat`）、`lib-bin-cli/src/run.rs`、`lib-bin-tui/src/worker.rs`、`lingting-agent-ui/src/api/link.ts`，以及 `docs/api/runtime.md` / `docs/api/readme.md` 与 `client.*` 用例。
  - path 规范只允许固定动词：`create` / `update` / `delete` / `get` / `list` / `cancel` / `stream`（`docs/develop.md`）。工具确认不属于其中任何一类，因此「换个名字」比「硬塞进 update」更符合规范。
- **可选方案**：
  1. 保留 `api/client/update`，在其 `kind` 里追加 `TOOL_CONFIRM`；
  2. 新增 `api/client/confirm`，只承载确认；
  3. **`api/client/update` 重命名为 `api/client/post`**，统一承载上行消息（心跳、问询应答、工具确认）。
- **决策**：③。
  - 路径：`POST api/client/post`；DTO 改名 `ClientUpdatePO` → `ClientPostPO`、`ClientUpdateVO` → `ClientPostVO`、`ClientUpdateKind` → `ClientPostKind`（取值 `PING` / `INQUIRY_ACK` / `TOOL_CONFIRM`）。
  - `ClientPostPO` 追加两个可空字段：`toolCallId`、`decision`（`APPROVE` / `REJECT`）；`PING` / `INQUIRY_ACK` 时两者必须为空，`TOOL_CONFIRM` 时两者必须非空（缺一 400）。
  - `manager-agent` 的 `heartbeat` 拆成「刷新心跳」与「提交确认」两个入口，按 `kind` 分派；不再让一个函数吃三种语义。
- **理由**：
  - ①在 path 语义上说不通（`update` 是「更新客户端」，而确认更新的是 Run 的挂起状态），且会让 `POST api/client/update` 的请求体永远带两个大多数时候为空的字段。
  - ②多一条接口，而心跳与确认的鉴权前提（已注册的客户端）完全一致，没有拆开的必要。
  - ③一次改名把「上行的所有无状态动作」收进一条通道，接口总数不增（`tool/audit/list` 是本次唯一新增）。
- **约束**：
  - 破坏性变更：不保留旧路径，不同时注册两代（README §二 硬约束 9）。
  - 响应体仍是 `{ pongAtMs }`：确认请求也会刷新 `lastSeen` 并返回 pong（一次确认同时被当作心跳，避免确认期间心跳被误判超时）。
  - `docs/api/runtime.md`、`docs/api/readme.md` 的错误码表、`docs/develop.md` 的「运行时接口」段落必须同步。
- **承接任务**：P3-026（服务端与 DTO 改名）、P3-028（TUI 调用点与确认上行）、P3-029（WebUI 调用点）、P3-030（用例改名）、P3-031（文档同步）。

---

## D-P3-023 挂起的落地口径

- **状态**：`DECIDED`（来自 D-P3-004 的方案调整，细节由需求方逐项确认）
- **问题**：确认期间 Run 挂在什么状态、占不占并发位、同会话后续 Run 能不能跑、等多久、断连怎么办、怎么恢复？
- **决策**：
  - **状态**：Run 回 `QUEUED`，`runs.pending_tool_call_id` / `pending_tool_round` 记录等的是什么；**不新增 `WAITING` 状态**。
  - **不占位**：挂起时释放全局并发许可，结束本次 `drive`；`metrics` 快照的 `running_now` 不包含它。
  - **会话不阻塞**：挂起期间该会话允许执行后续 Run（这是 P2「同会话串行」的唯一例外）。代价是同一会话的历史可能交错，客户端按 `runId` 分组展示。
  - **确认落库**：挂起时 `tool_calls` 插入 `status = PENDING`（参数已脱敏）；批准 → 执行后覆盖为 `OK` / `ERROR`；驳回 → `DENIED` + 结果文本「用户拒绝调用该工具」。待确认列与 `PENDING` 记录一起清除。
  - **不设超时**：没有 `confirm_timeout_ms`；挂起的 Run 只能由「客户端确认 / 取消 Run / 释放占用或断连 / 服务关闭」四条路径收尾，后三条走既有取消链路落 `CANCELLED` 并清 `pending_tool_call_id`。
  - **重连补发**：客户端重新注册并建立 `api/client/stream` 后，服务端对「该客户端占用会话下、`pending_tool_call_id` 非空」的 Run 各补发一次 `tool_call_confirm_required`。
  - **恢复方式**：唤醒后从 `messages` 的历史里重建 Context；识别「最后一个 assistant `tool_calls` 里的调用没有配对 TOOL 结果」，按决定执行该工具（批准）或写入拒绝结果（驳回），然后继续推理。
- **理由**：
  - 不新增状态：`pending_tool_call_id` 足以区分「正常排队」与「等确认」，而新增状态会波及状态机、迁移、客户端展示与全部既有用例。
  - 释放并发许可 + 不阻塞会话，是「挂起不占用 Run 位」的直接实现；否则一个人没点确认就会拖慢整个进程。
  - 不设超时是需求方的选择：确认是人的动作，机器不宜替人决定「不确认 = 拒绝」。
  - 重连补发解决「确认入口随断连丢失」的可用性问题；否则挂起的 Run 会永久卡住（与「不设超时」叠加）。
- **约束**：
  - 恢复只处理「最后一个未配对的调用」；若历史里出现多处未配对，按数据损坏处理 → `FAILED`「工具轨迹不完整」。
  - 挂起期间不发 `tool_call_completed`；取消时对 `PENDING` 记录补一条 `CANCELLED` 的收尾（不留悬挂的审计行）。
  - 唤醒与取消竞争时以数据库条件更新为准：`WHERE pending_tool_call_id = ? AND status = 'QUEUED'` 命中者胜，另一方看到 0 行即放弃。
- **承接任务**：P3-014（挂起点与恢复）、P3-017（列与 PENDING 状态）、P3-018（挂起 / 唤醒的 Service 入口）、P3-027（服务端链路与重连补发）。

---

## D-P3-024 每轮边界落库（修订 D-P2-008）

- **状态**：`DECIDED`（D-P3-003 方案②的必然结果）
- **问题**：工具轮次进行中要不要写库？P2 的「流式期间不写库」还成立吗？
- **背景与证据**：P2 的 D-P2-008 规定「流式期间不写库，助手消息与 `COMPLETED` 同事务提交」。多轮工具循环下，中间轮次的 assistant `tool_calls` 与工具结果必须落库，否则挂起恢复与跨 Run 历史都没有事实源。
- **决策**：
  - 流式期间仍然**不写库**（一条增量都不落）。
  - **每轮边界**成为新的落库点：进入下一轮之前，本轮的 assistant `tool_calls` 消息与全部 TOOL 结果消息必须在同一事务里提交。
  - 最终助手消息与 `COMPLETED` 仍在同一事务提交（P2 规则对最后一轮不变）。
  - 落库失败 → 本轮失败 → Run 落 `FAILED`「工具轨迹落库失败」（不允许「事件已发但库里没有」）。
- **理由**：
  - 挂起 / 唤醒的恢复依赖历史（D-P3-023）；不落库就只能重跑工具或另存轨迹。
  - 保留「流式期间不写库」避免了写放大：一次多轮 Run 最多 N 个事务（N = 轮次），而不是每个增量一个事务。
- **约束**：
  - 事务粒度是「一轮」，不是「一条消息」：assistant `tool_calls` 与其全部 TOOL 结果同事务提交，避免半轮历史。
  - `seq` 仍由 Repository 按会话内递增分配，工具消息与普通消息共用同一序列。
  - 用例 `persist.*` 的既有断言口径需要复核（成功 Run 的助手消息条数不再恒为 1）。
- **承接任务**：P3-014、P3-018、P3-030。

---

## D-P3-025 工具轨迹在 `messages` 里的配对与裁剪口径

- **状态**：`DECIDED`（D-P3-003 方案②的必然结果）
- **问题**：`TOOL` 消息进入历史后，怎么配对、怎么排序、怎么裁剪？
- **决策**：
  - **配对**：`TOOL` 消息的 `tool_call_id` 必须与同会话内某条 assistant 消息的 `tool_calls[].id` 相同。
  - **顺序**：assistant（含 `tool_calls`）→ 该轮的 TOOL 结果（按模型给出的调用顺序）→ 下一轮的 assistant……历史整体仍按 `seq` 升序。
  - **裁剪**：以「工具调用组」为单位（一条 assistant + 其全部 TOOL 结果）。按条数与预算裁剪时，整组一起保留或整组一起丢弃；不允许只丢 TOOL 留下 assistant `tool_calls`（上游会 400），也不允许只丢 assistant 留下孤儿 TOOL。
  - **预算计算**：组内每条消息照 `MESSAGE_TOKEN_OVERHEAD` + 正文估算；组作为整体参与 `available_tokens` 的比较。
  - **系统提示词与本次输入**仍不参与裁剪（P2 不变）。
- **理由**：
  - 按条丢弃是 P2 的规则，但它假设「每条消息独立」。工具轨迹是有依赖的成组结构，必须整组处理。
  - 整组丢弃保持了「最旧的先走」的语义，同时不会产生非法请求。
- **约束**：
  - 裁剪实现集中在 `agent-context/src/trim.rs` 与 `builder.rs`；`ContextItem` 需要携带 `tool_call_id` 与「组标识」，不允许在 Repository 或 Runtime 里二次切片。
  - `metadata.trimmed_messages` 语义调整为「被裁掉的消息条数」（组内条数照常计入），并在注释里写清。
  - 用例 `chat.context` 的口径需要扩展：新增 `tool.context` 覆盖「组整体丢弃」。
- **承接任务**：P3-012、P3-030。

---

## D-P3-026 确认交互的客户端形态

- **状态**：`DECIDED`
- **问题**：用户在 TUI / WebUI 上怎么看到确认请求、怎么应答？
- **决策**：
  - **TUI**：对话区追加一行提示（`⚠ 需要确认：http_request(…) [y] 允许 / [n] 拒绝`），按 `y` / `n` 即发送 `api/client/post { kind = TOOL_CONFIRM, decision }`；不新增面板、不新增第二个快捷键。没有待确认项时 `y` / `n` 不生效（不误触）。
  - **WebUI**：在对话流里渲染一张确认卡片（工具名 + 脱敏参数摘要 + 「允许」/「拒绝」两个按钮）；点击后按钮变为不可重入的等待态，直到收到下一次 `tool_call_started` / `tool_call_completed` 或超时前重连的补发。
  - 两者都只展示「一次确认」，不提供「总是允许」与批量确认。
- **理由**：
  - 沿用既有交互模型（TUI 是键盘驱动的行式界面，WebUI 是消息流里的卡片），改动最小。
  - 不做「总是允许」：它是权限系统的长期语义，需要在配置层面表达，而不是一次点击。
- **约束**：
  - 客户端不做本地「已确认」去重：重复确认由服务端按条件更新幂等拒绝（400 固定文案）。
  - 确认卡片的样式沿用既有主题与间距，不新增设计 token。
- **承接任务**：P3-028、P3-029、P3-030。

---

## D-P3-027 挂起态改用 `WAITING` 状态

- **状态**：`DECIDED`（需求方在推进 Batch D 时定案，修订 D-P3-023 的「回 `QUEUED`」）
- **问题**：挂起的 Run 若退回 `QUEUED`，执行队列的 `queued_run` 只按 `status = 'QUEUED'` 取最早的一条，会在 `drive` 返回后立刻把同一个 Run 再取出来：既不能真正挂起（忙循环），也说不清「它在等什么」。
- **决策**：
  - `RunStatus` 追加 `Waiting`（wire 值 `WAITING`）：`QUEUED → RUNNING → WAITING → QUEUED → RUNNING …` 或终态。
  - 挂起 = `RUNNING → WAITING` + 写 `pending_tool_call_id` / `pending_tool_round`；唤醒 = `WAITING → QUEUED`（清待确认列），由 `resume_run` 的条件更新完成。
  - `WAITING` 是「未结束」：可被取消、可被启动自愈清理（落 `CANCELLED` 并清待确认列），但不会被队列取走，也不计入 `running_now`。
  - 取消 / 释放占用 / 断连 / 自愈的 SQL 在落终态时一并清空待确认列，不留悬挂挂起态。
- **理由**：
  - 状态本身就是「它在等什么」的第一层表达，`pending_*` 列只补「等哪一次调用」；后续要加「等用户输入」「等外部事件」时可以平行追加，不必再动队列的挑选条件。
  - 队列条件保持「只取 `QUEUED`」不变：挂起不需要在队列里再写一遍排除逻辑，也不会出现「取出来又放回去」的空转。
  - 用状态区分「排队」与「等确认」，`run/get` 与客户端展示也不再需要靠 `pendingToolCallId` 反推。
- **约束**：
  - `WAITING` 不是终态：`is_open` 必须包含它（可取消），`fail_run` / `cancel_run` / `cancel_session_runs` 的条件集合必须包含它。
  - 迁移不需要改表结构（`status` 列存 wire 值，`pending_*` 列已由 `0002` 落地）。
  - `RunOutcome` 追加 `Suspended { tool_call_id }`：挂起既不是完成也不是失败，执行队列只记日志、不写终态。
- **承接任务**：P3-014（挂起点与恢复）、P3-015（取消收尾）、P3-027（服务端链路）、P3-030（用例口径改为断言 `WAITING`）。


---

## D-P3-028 回归判据 R-1 的口径：请求体默认携带工具定义

- **状态**：`DECIDED`（推进 Batch H 的 P3-032 时定案，修订 `acceptance.md` R-1 的一处表述）
- **问题**：R-1 原来写「`chat.*` 全绿 + Mock `last-request` 中无 `tools` 键（与 P2 请求体一致）」。P3 的 G-1 要求「模型可见清单就是权限边界」，默认配置下 `calculator` / `current_time` / `file_read` / `file_search` 四个工具始终可见，因此任何一次 Run 的请求都带 `tools`——R-1 的后半句在 P3 不可能成立。
- **决策**：
  - R-1 只约束「无工具语义时行为不变」：消息序列（system 在前、末条为本次输入、历史按 `seq` 升序居中）、裁剪算法、token 语义、终态事务与 P2 一致，由 `chat.context` / `chat.multiturn` / `chat.truncate` / `run.cancel` 取证。
  - 请求体带 `tools` 不是回归，而是 P3 的目标行为；「清单里该有谁」由 `tool.schema` / `tool.denied` 取证。
  - `tools` 为空时不序列化该字段（`agent-route` 的 `LlmRequest` 契约）：一个工具都不可见的配置下请求体仍与 P2 同形。
- **理由**：判据要能区分「回归」与「本阶段新增的语义」；把 `tools` 缺席当成基线，会让 P3 的目标与验收判据互相矛盾。
- **约束**：不得为了让 R-1 字面成立而在请求里省略工具定义。
- **承接任务**：P3-032（判据改写与报告取证）、P3-030（`tool.schema` / `tool.denied` 的清单断言）。

---

## 定案状态

21 项决策（D-P3-001 ~ D-P3-021）与 7 项追加决策（D-P3-022 ~ D-P3-028）**全部 `DECIDED`**，无 `OPEN` 项。

追加决策的来源：D-P3-003 / D-P3-004 / D-P3-005 由需求方定案（其中 D-P3-004 追加了两条方案调整），D-P3-022 ~ D-P3-026 是把这两条调整与方案②的后果逐项写死；D-P3-027 是推进 Batch D 时对 D-P3-023「回 `QUEUED`」的修订（挂起改用 `WAITING` 状态）；D-P3-028 是 P3-032 阶段验收时对 R-1 口径的修订（请求体默认携带工具定义）。
