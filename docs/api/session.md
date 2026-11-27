# session 接口（会话 / 消息 / Run）

会话 / 消息 / Run 的业务接口。P1 为 Mock 实现，P2 起由真实 Agent Runtime 执行；接口、DTO 与事件枚举在 P2 全程未变。通用约定（地址、响应包装、错误体、字段类型）见 [readme.md](readme.md)。

- 定义位置：`crates/lib-web/src/session/api.rs`（handler）、`crates/agent-core/src/session/`（契约类型，P2 起）
- 业务实现：`crates/service-core/src/session/`（Service）与 `crates/service-core/src/repository/`（SQLite 访问）；执行由 `crates/agent-runtime/` 实现
- 契约与执行决策：`.steps/p1/decisions.md` D-025、`.steps/p1.1/decisions.md` D-P1.1-004（Run 触发与 `api/run/stream` 移除）、`.steps/p2/decisions.md`（真实执行、终态归属与 token 语义）

## 接口清单

| 方法 | path | 入参 | 返回 | 说明 |
|---|---|---|---|---|
| POST | `api/session/create` | `SessionCreatePO`（请求体） | `SessionSummary` | 标题缺省为「新会话」，模型缺省取 `[provider]` 的 `default_model`；指定未配置的模型返回 400「模型不可用：<id>」 |
| GET | `api/session/list` | 无 | `SessionSummary[]` | 按 `updatedAtMs` 倒序 |
| GET | `api/message/list` | `MessageListParams`（查询参数） | `Message[]` | 按写入顺序；会话不存在 404 |
| POST | `api/message/create` | `MessageSendPO`（请求体） | `MessageSendVO` | 写入用户消息并创建 `QUEUED` 的 Run；会话已被占用时，该会话的 `PAUSED` Run 会先被恢复（D-P6-009） |
| GET | `api/run/get` | `RunGetParams`（查询参数） | `Run` | Run 不存在 404 |
| POST | `api/run/cancel` | `RunCancelPO`（请求体） | `Run` | 只允许取消未结束的 Run（`QUEUED` / `RUNNING` / `WAITING` / `PAUSED`），否则 400 |
| GET | `api/run/history` | `RunHistoryParams`（查询参数） | `RunHistory` | Run 的步骤历史（检查点序列 + 恢复次数 + 中断原因）；只读，Run 不存在 404 |

会话与 Run 的 ID 只能走查询参数或请求体：`web_api` 的 path 不支持占位符（D-008）。

## 入参

| 类型 | 字段 | 位置 | 必填 | 说明 |
|---|---|---|---|---|
| `SessionCreatePO` | `title` | body | 否 | 全为空白时按缺省处理 |
| `SessionCreatePO` | `model` | body | 否 | 全为空白时按缺省处理 |
| `MessageListParams` | `sessionId` | query | 是 | 目标会话 ID |
| `MessageSendPO` | `sessionId` | body | 是 | 目标会话 ID |
| `MessageSendPO` | `content` | body | 是 | 用户消息正文；全为空白时 400 |
| `RunGetParams` | `runId` | query | 是 | 目标 Run ID |
| `RunCancelPO` | `runId` | body | 是 | 目标 Run ID |
| `RunHistoryParams` | `runId` | query | 是 | 目标 Run ID |

## 载荷类型

### SessionSummary

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `string` | 会话 ID |
| `title` | `string` | 会话标题 |
| `model` | `string` | 当前使用的模型标识 |
| `messageCount` | `string` | 已保存的消息数量（`i64` → 字符串） |
| `createdAtMs` | `string` | 创建时间（Unix 毫秒） |
| `updatedAtMs` | `string` | 最后更新时间（Unix 毫秒） |

### Message

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `string` | 消息 ID |
| `sessionId` | `string` | 所属会话 ID |
| `runId` | `string \| null` | 产生该消息的 Run；系统侧预置消息没有 Run |
| `role` | `MessageRole` | `SYSTEM` / `USER` / `ASSISTANT` / `TOOL` |
| `content` | `string` | 消息正文 |
| `toolCallId` | `string \| null` | 工具结果应答的调用标识；只有 `TOOL` 消息有值 |
| `toolName` | `string \| null` | 工具结果对应的工具名；只有 `TOOL` 消息有值 |
| `toolCalls` | `LlmToolCallVO[] \| null` | 助手本轮请求的调用清单；只有 `ASSISTANT` 消息可能非空 |
| `model` | `string \| null` | 生成该消息时使用的模型；用户消息为空 |
| `inputTokens` | `string \| null` | 输入 token 用量；未统计时为空 |
| `outputTokens` | `string \| null` | 输出 token 用量；未统计时为空 |
| `createdAtMs` | `string` | 创建时间（Unix 毫秒） |

### LlmToolCallVO

助手消息里的工具调用清单：与 `agent-route` 的 `LlmToolCall` 字段同构，Provider 协议类型不出服务端适配层（D-P3-010）。

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `string` | 调用标识；与工具结果消息的 `toolCallId` 配对 |
| `name` | `string` | 工具名 |
| `arguments` | `string` | 参数：模型给出的完整 JSON 文本，读取方自行解析 |

### Run

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `string` | Run ID |
| `sessionId` | `string` | 所属会话 ID |
| `status` | `RunStatus` | `QUEUED` / `RUNNING` / `WAITING` / `PAUSED` / `COMPLETED` / `CANCELLED` / `FAILED`；`WAITING` 表示在等客户端确认某次工具调用（`runs.pending_tool_call_id` 非空），不占全局并发许可（D-P3-027）；`PAUSED` 是非预期中断留下的可恢复中间态，见「Run 状态机与检查点」 |
| `model` | `string` | 本次执行使用的模型标识 |
| `error` | `string \| null` | 失败原因；仅 `FAILED` 时有值，且不含内部细节 |
| `interruptedReason` | `string \| null` | 中断原因（固定文案）；仅 `PAUSED` 时有值，不含内部细节与环境路径 |
| `resumeCount` | `string` | 已成功推进的恢复次数；每次 `PAUSED → RUNNING` 加一（`i64` → 字符串） |
| `lastCheckpointSeq` | `string \| null` | 最近一条检查点的序号；没有检查点或升级前的旧 Run 为空（`i64` → 字符串） |
| `inputTokens` | `string \| null` | 输入 token 用量；未统计时为空 |
| `outputTokens` | `string \| null` | 输出 token 用量；未统计时为空 |
| `createdAtMs` | `string` | 创建时间（Unix 毫秒） |
| `updatedAtMs` | `string` | 最后更新时间（Unix 毫秒） |
| `finishedAtMs` | `string \| null` | 结束时间（Unix 毫秒）；未结束时为空 |

### RunHistory

`api/run/history` 的响应：状态在 `run` 上，过程在 `steps` 上（`get` 是状态，`history` 是过程）。

| 字段 | 类型 | 说明 |
|---|---|---|
| `run` | `Run` | 该 Run 的当前状态快照 |
| `resumeCount` | `string` | 已成功推进的恢复次数（`i64` → 字符串） |
| `interruptedReason` | `string \| null` | 中断原因；只有中断态与恢复过的 Run 有值 |
| `steps` | `RunStep[]` | 步骤序列：按 `seqNo` 升序；升级前的旧 Run 为空数组（合法响应，不是错误） |

### RunStep

一个已提交边界上的步骤：只暴露派生字段，`payload` 原文不外泄。

| 字段 | 类型 | 说明 |
|---|---|---|
| `seqNo` | `string` | 同一 Run 内从 1 连续递增（`i64` → 字符串） |
| `phase` | `CheckpointPhase` | `MODEL_DONE` / `TOOLS_DONE` / `AWAITING_CONFIRM` |
| `toolRound` | `string` | 轮次序号：起点为 0，其后每轮加一 |
| `createdAtMs` | `string` | 记录产生时间（Unix 毫秒） |
### MessageSendVO

| 字段 | 类型 | 说明 |
|---|---|---|
| `message` | `Message` | 已保存的用户消息 |
| `run` | `Run` | 由本次发送创建的 Run |

### RunEvent

事件流的载荷：一条事件一帧 JSON，`event` 字段与 SSE 的 `event:` 行同值。

| 字段 | 类型 | 说明 |
|---|---|---|
| `event` | `RunEventKind` | 事件类型，取值见下 |
| `seq` | `string` | 事件序号，从 `0` 起递增，与 SSE 的 `id` 相同 |
| `runId` | `string` | 所属 Run ID |
| `sessionId` | `string` | 所属会话 ID |
| `createdAtMs` | `string` | 事件产生时间（Unix 毫秒） |
| `delta` | `string \| null` | 增量文本；仅 `message_delta` 有值 |
| `messageId` | `string \| null` | 助手消息 ID；`run_started` 与 `run_completed` 有值 |
| `status` | `RunStatus \| null` | Run 最新状态；`run_started` 与 `run_completed` 有值 |
| `error` | `string \| null` | 失败或终止原因；`run_failed`、`run_cancelled` 与 `server_shutdown` 有值，`tool_call_completed` 失败时复用为工具错误文本；均不含内部细节 |
| `inputTokens` | `string \| null` | 输入 token 用量；`run_started` 与 `run_completed` 有值（Mock 按字符数估算） |
| `outputTokens` | `string \| null` | 输出 token 用量；`run_completed` 有值（Mock 按字符数估算） |
| `toolCallId` | `string \| null` | 工具调用标识；三个工具事件都有值，客户端据此配对开始与结束 |
| `toolName` | `string \| null` | 工具名；三个工具事件都有值 |
| `toolArguments` | `string \| null` | 已脱敏、已截断的参数 JSON 文本；`tool_call_started` 与 `tool_call_confirm_required` 有值 |
| `toolStatus` | `ToolCallStatus \| null` | 工具调用状态；仅 `tool_call_completed` 有值 |
| `toolDurationMs` | `string \| null` | 工具调用耗时（毫秒，含重试），`i64` → 字符串；仅 `tool_call_completed` 有值 |
| `toolRound` | `number \| null` | 第几轮工具调用（从 1 开始）；`tool_call_started` 与 `tool_call_confirm_required` 有值 |

### RunEventKind

| 取值 | 触发时机 | 额外有值的字段 |
|---|---|---|
| `run_started` | Run 被推进到 `RUNNING`（由会话执行队列触发） | `status`、`messageId`、`inputTokens` |
| `message_delta` | 助手回复的增量片段 | `delta` |
| `run_completed` | Run 正常结束，助手消息已落库 | `status`、`messageId`、`inputTokens`、`outputTokens` |
| `run_failed` | 执行失败（P2 起真实产出）：认证 / 上游 / 空闲超时 / 上下文超预算 / 排队超时 | `error`（固定脱敏文案） |
| `run_cancelled` | 会话停止（占用释放 / 客户端断连）导致 Run 被取消 | `status`、`error`（固定原因文本） |
| `server_shutdown` | 服务关闭导致 Run 终止（Run 已落成 `CANCELLED`） | `status`、`error` |
| `tool_call_started` | 工具调用开始执行之前（每轮里每次调用一条） | `toolCallId`、`toolName`、`toolArguments`、`toolRound` |
| `tool_call_completed` | 工具调用结束：成功 / 失败 / 超时 / 拒绝 / 取消都在这一条上 | `toolCallId`、`toolName`、`toolStatus`、`toolDurationMs`；失败时 `error` |
| `tool_call_confirm_required` | `confirm` 级工具被请求调用，Run 转 `WAITING` 等客户端确认 | `toolCallId`、`toolName`、`toolArguments`、`toolRound` |
| `heartbeat` | 保留值：Run 事件流的空闲保活；P1.1 起客户端心跳（`api/client/post`）承担保活，运行时会话暂不产出 | 仅公共字段 |

P3 追加上述三个工具事件（D-P3-013）：只新增枚举值与 `RunEvent` 的可选字段，既有事件的字段语义一律不变。`ToolCallStatus` 取 `OK` / `ERROR` / `DENIED` / `TIMEOUT` / `CANCELLED` / `INVALID_ARGUMENTS` / `PENDING`；`PENDING` 只出现在审计记录里（挂起期间），不会出现在 `tool_call_completed` 上。

## Run 触发与事件

`api/run/stream` 已移除（D-P1.1-004）：服务端推送只剩 `api/client/stream` 一条通道，Run 的执行由「会话被占用 + 消息创建」触发。

| 项 | 约定 |
|---|---|
| 触发点 | 一条规则：Run 只在所属会话**当前被占用**时执行，执行者是该会话的运行时会话 |
| 占用时 | `api/occupancy/create` 把该会话存量 `QUEUED` 按 `createdAtMs` 升序串行执行；`api/message/create` 追加到队列尾部 |
| 未占用时 | 新建的 Run 停在 `QUEUED`，不排队、不执行（日志记一条原因），客户端必须先占用 |
| 串行 | 同一会话同一时刻最多一个 `RUNNING` Run；`QUEUED → RUNNING` 仍走状态 CAS，一个 Run 只有一个生产方。**唯一例外是挂起**：`WAITING` 的 Run 不占全局并发许可，同一会话可以继续执行后续 Run（D-P3-023 / D-P3-027），因此历史顺序可能不等于发起顺序 |
| 全局并发 | `QUEUED → RUNNING` 在拿到全局许可（`[agent] max_concurrent_runs`）之后才发生，排队期间 `run/get` 显示 `QUEUED`；等待超过 `run_queue_timeout_ms` 落 `FAILED`「执行排队超时」，且不产出 `run_started` |
| token 语义 | `run_started.inputTokens` 是预算估算器的估算值；`run_completed` 的 `inputTokens` / `outputTokens` 用 Provider 的真实 usage，Provider 未给出时留空 |
| 落盘 | 事件先写进该会话的事件文件并 flush，再转发到 `api/client/stream`；文件是唯一事实源（见 [occupancy.md](occupancy.md) 的「事件文件」） |
| 转发帧 | 一条 `RunEvent` 包成 `session_event` 帧，`data` 里是整条事件；`RunEvent.seq`（文件内序号）与外层 `ClientEvent.seq` 不同源 |
| 终态 | `run_completed` / `run_failed` / `run_cancelled` / `server_shutdown` 之一收尾，之后不再向该文件追加 |
| 终态归属 | 每条 Run 的终态事件由执行层发出（P2-024）；写者的兜底补写只在最后一条事件不是终态时才触发，作为安全网 |
| 断开 | 客户端断开（下行流被 drop）→ 断连处理链停止运行时会话 → 未结束的 Run（`QUEUED` / `RUNNING` / `WAITING` / `PAUSED`）落 `CANCELLED` 并补终态事件 |
| 取消 | `api/run/cancel` 只允许取消未结束的 Run（`QUEUED` / `RUNNING` / `WAITING` / `PAUSED`）；取消把 Run 置 `CANCELLED` 后触发取消信号：执行方 drop 模型流或工具执行并补 `run_cancelled`（`error` = 「用户取消执行」），挂起列随之清空、`PENDING` 审计收尾为 `CANCELLED` |
| 服务关闭 | 运行时会话以 `server_shutdown` 收尾（含 `status` 与可读 `error`）；关闭期间的新请求返回 `503` `{"code":503,"message":"服务正在关闭，请稍后重试"}` |
| 客户端 | 编排顺序为 `client/create` → `client/stream` → `occupancy/create` → `message/create` → 消费 `session_event` → `run/get` 对账；下行帧格式、心跳与问询见 [runtime.md](runtime.md)，TUI 与 WebUI 都用 fetch 流式响应自行切帧 |

## Run 状态机与检查点（P6 6A 起）

`RunStatus` 增加一个非终态 `PAUSED`，其余取值与 wire 值逐字不变。

| 迁移 | 允许的前置状态 | 触发者 |
|---|---|---|
| `QUEUED → RUNNING` | `QUEUED` | 执行队列拿到全局许可后 |
| `RUNNING → WAITING` | `RUNNING` | 命中工具确认点（D-P3-027） |
| `WAITING → QUEUED` | `WAITING` | 客户端回传确认决定 |
| `RUNNING → PAUSED` | `RUNNING` | 只由启动自愈产生（进程消失后的第一次装配） |
| `PAUSED → RUNNING` | `PAUSED` | 只由「会话被占用 + 该会话收到一条新消息」触发 |
| 四个非终态 → `CANCELLED` | `QUEUED` / `RUNNING` / `WAITING` / `PAUSED` | 取消 / 会话停止 / 服务关闭 |
| `RUNNING → COMPLETED` / `FAILED` | `RUNNING` | 正常收尾 / 结构性失败 |

三条约定：

- **终态唯一**：`COMPLETED` / `CANCELLED` / `FAILED` 各自只能写入一次，任何路径都不得改写已是终态的行；`PAUSED` 不是终态，也不能由终态回到 `PAUSED`。
- **`PAUSED` 的生产者是且只有启动自愈**（写入条件为 `status = 'RUNNING'`）；它不占全局并发许可、不阻塞同会话后续 Run、不计入 `api/probe/health` 的 `runningRunCount`。
- **状态集合只有一处定义**：`RunStatus` 的 `TERMINAL` / `ACTIVE` 两个集合与 `is_terminal()` / `is_active()` / `can_cancel()`；数据层的条件更新用 `ACTIVE` 生成取值列表，调用点不另写列表。

### 检查点

检查点是「一次 Run 已提交到哪」的唯一进度指针：只进 SQLite 的 `run_checkpoints` 表，**不进事件文件**（事件文件会被轮转删除、`seq` 每次运行时会话从 0 重计、写入不 `sync_all`，三者都破坏「已提交边界」的定义）。

| 列 | 说明 |
|---|---|
| `seqNo` | 同一 Run 内从 1 连续递增，由存储层分配；`UNIQUE (run_id, seq_no)` 让重复写入直接报错 |
| `phase` | `MODEL_DONE` / `TOOLS_DONE` / `AWAITING_CONFIRM`，依次对应「模型轮次已落库」「工具批次已落库」「Run 停在工具确认点」三种已提交边界 |
| `toolRound` | 轮次序号：起点为 0，其后每轮加一 |
| `lastMessageSeq` | 已提交消息的 `seq` 上界，恢复时「从这条之后继续」 |
| `payload` | 只放恢复所需的最小事实：格式版本 + 本批工具组的调用标识与落库状态 + 起点检查点的助手消息 ID；不含上下文全文、系统提示词、`AGENTS.md` 内容与 Provider 原始响应 |
| `formatVersion` | 格式版本，当前为 `1`；与载荷里的同名字段一并写入，两处必须一致 |

- `Run.lastCheckpointSeq` 与插行在同一事务里更新，插入失败时保持原值。
- 版本高于当前代码（`Unsupported`）或版本为 0 / 缺字段 / 与列不一致（`Corrupt`）一律判「不可恢复」：不猜测、也不降级成「从头重跑」。
- **落终态即裁剪**：`COMPLETED` / `FAILED` / `CANCELLED` 三条收尾路径都在推进状态的事务里把该 Run 的检查点裁到最近 20 条（`CHECKPOINT_KEEP_LAST`），因此不存在「终态已提交、检查点还留着全部过程」的窗口。未结束的 Run（含 `PAUSED`）一条都不删。
- 步骤历史由 `api/run/history` 提供（只读，`steps` 按 `seqNo` 升序，不含 `payload` 原文）；`Run.lastCheckpointSeq` 仍是「已提交到哪」的指针。
- **恢复时重发的 `run_started` 复用起点检查点记下的助手消息 ID**（`payload.assistantMessageId`）：客户端的助手气泡因此不会因为恢复多出一条；该字段只在起点检查点（`toolRound = 0` 的 `MODEL_DONE`）上有值。
- **`PAUSED` 的生产者是启动自愈，消费入口是 `api/message/create`**：会话被占用后发一条消息，该会话的 `PAUSED` Run 会先被恢复并跑完，本次消息产生的 `QUEUED` Run 在其后执行；只占用不发消息不触发恢复。

## 请求与响应样例

```text
POST api/session/create
{"title":"第一个会话"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"id":"1842000000000000001","title":"第一个会话","model":"gpt-4o-mini","messageCount":"0","createdAtMs":"1767225600000","updatedAtMs":"1767225600000"}}
```

```text
GET api/session/list

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":[{"id":"1842000000000000001","title":"第一个会话","model":"gpt-4o-mini","messageCount":"1","createdAtMs":"1767225600000","updatedAtMs":"1767225601000"}]}
```

```text
POST api/message/create
{"sessionId":"1842000000000000001","content":"你好"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{
  "message":{"id":"1842000000000000002","sessionId":"1842000000000000001","runId":"1842000000000000003","role":"USER","content":"你好","toolCallId":null,"toolName":null,"toolCalls":null,"model":null,"inputTokens":null,"outputTokens":null,"createdAtMs":"1767225601000"},
  "run":{"id":"1842000000000000003","sessionId":"1842000000000000001","status":"QUEUED","model":"gpt-4o-mini","error":null,"interruptedReason":null,"resumeCount":"0","lastCheckpointSeq":null,"inputTokens":null,"outputTokens":null,"createdAtMs":"1767225601000","updatedAtMs":"1767225601000","finishedAtMs":null}
}}
```

```text
GET api/message/list?sessionId=1842000000000000001

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":[{"id":"1842000000000000002","sessionId":"1842000000000000001","runId":"1842000000000000003","role":"USER","content":"你好","toolCallId":null,"toolName":null,"toolCalls":null,"model":null,"inputTokens":null,"outputTokens":null,"createdAtMs":"1767225601000"}]}
```

```text
GET api/run/get?runId=1842000000000000003

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"id":"1842000000000000003","sessionId":"1842000000000000001","status":"QUEUED","model":"gpt-4o-mini","error":null,"interruptedReason":null,"resumeCount":"0","lastCheckpointSeq":null,"inputTokens":null,"outputTokens":null,"createdAtMs":"1767225601000","updatedAtMs":"1767225601000","finishedAtMs":null}}
```

```text
POST api/run/cancel
{"runId":"1842000000000000003"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"id":"1842000000000000003","sessionId":"1842000000000000001","status":"CANCELLED","model":"gpt-4o-mini","error":null,"interruptedReason":null,"resumeCount":"0","lastCheckpointSeq":null,"inputTokens":null,"outputTokens":null,"createdAtMs":"1767225601000","updatedAtMs":"1767225602000","finishedAtMs":"1767225602000"}}
```


## 错误响应

业务失败由 `service-core` 的 `SessionError` 表达，`lib-web` 统一映射为 404 / 400 / 500，响应体仍是 `{code, message}`（D-024）：

| 场景 | HTTP | `message` |
|---|---|---|
| `message/list`、`message/create` 的 `sessionId` 不存在 | 404 | `会话不存在：<sessionId>` |
| `run/get`、`run/cancel` 的 `runId` 不存在 | 404 | `Run 不存在：<runId>` |
| `message/create` 的 `content` 全为空白 | 400 | `消息内容不能为空` |
| 取消已结束的 Run | 400 | `Run 当前状态为 <STATUS>，不可取消` |
| 请求体或查询参数缺失、类型不符 | 400 | 框架固定文案（`请求参数转换失败` / `查询参数转换失败` / `请求体不是有效的 JSON`） |
| 服务内部错误（如 ID 生成失败） | 500 | `服务器内部错误`，细节只进日志 |

## 执行行为边界（P2 起）

- 存储是 SQLite（`<应用根目录>/data/agent.db`），重启后会话、消息与 Run 都在；`RUNNING` 的 Run 启动自愈为 `FAILED`，`QUEUED` 的 Run 自愈为 `CANCELLED`，占用不恢复（重启后 `occupancy/get` 返回 404）。
- 发送消息只写入用户消息与 `QUEUED` 的 Run：助手消息与 `QUEUED → RUNNING → COMPLETED` 由该会话的运行时会话经执行端口（`agent-runtime`）真实调用模型完成，因此**必须先 `api/occupancy/create` 占用会话**；未被占用的会话里 Run 会一直停在 `QUEUED`（D-P1.1-004）。
- 助手回复的正文是 Provider 的流式增量拼接结果；`run_completed` 的 token 用 Provider 真实 usage，缺失时留空。
- 执行失败（认证 / 上游 / 空闲超时 / 上下文超预算 / 排队超时）落 `FAILED` + 固定脱敏 `error`，不写助手消息。
- `messageCount` 与 `updatedAtMs` 在每次发送消息时更新；取消 Run 不影响会话统计。
- 会话隔离由 `sessionId` 保证，不存在跨会话读取入口。

## 工具轮次与挂起（P3 起）

模型请求调用工具时，一次 Run 内会出现多个「推理轮次」，中间轮次的事实同样落在 `messages` 里：

- **工具轨迹进历史**：assistant 消息带 `toolCalls`（此时 `content` 可以为空串），每个工具结果是一条 `role=TOOL` 的消息并带 `toolCallId` / `toolName`；`message/list` 按写入顺序返回这些中间轮次的消息，客户端按 `toolCallId` 配对。
- **每轮边界落库**（D-P3-024）：进入下一轮之前，本轮的 assistant `toolCalls` 与全部 `TOOL` 结果已提交；流式期间仍不写库。最终助手消息与 `COMPLETED` 仍在同一事务提交（D-P2-008 保留）；失败 / 取消的 Run 不写最终助手消息，已落库的中间轮次消息保留。
- **挂起**（D-P3-023 / D-P3-027）：`confirm` 级工具在确认前不执行，Run 由 `RUNNING` 转 `WAITING` 并下发 `tool_call_confirm_required`；客户端经 `api/client/post { kind: TOOL_CONFIRM }` 回传决定后 Run 退回 `QUEUED` 重新排队，链路细节见 [runtime.md](runtime.md) 的「工具确认链路」。确认没有超时。
- **轮次上限**：`[agent] max_tool_rounds` 用尽时 Run 落 `FAILED`「工具调用轮次超过上限」，同样不写最终助手消息。
