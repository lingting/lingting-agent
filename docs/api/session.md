# session 接口（Mock 会话业务）

会话 / 消息 / Run 的业务接口。P1 是 Mock 实现（内存存储、无真实 Agent 执行），P2 只替换实现、不改契约。通用约定（地址、响应包装、错误体、字段类型）见 [readme.md](readme.md)。

- 定义位置：`crates/lib-web/src/session/api.rs`（handler）、`crates/types-core/src/session/`（契约类型）
- 业务实现：`crates/service-core/src/session/`（P2 换成 Repository + Agent Runtime）
- 契约决策与 P2 替换点：`.steps/p1/decisions.md` D-025；事件流契约见 D-007、D-026

## 接口清单

| 方法 | path | 入参 | 返回 | 说明 |
|---|---|---|---|---|
| POST | `api/session/create` | `SessionCreatePO`（请求体） | `SessionSummary` | 标题缺省为「新会话」，模型缺省为 `mock-model` |
| GET | `api/session/list` | 无 | `SessionSummary[]` | 按 `updatedAtMs` 倒序 |
| GET | `api/message/list` | `MessageListParams`（查询参数） | `Message[]` | 按写入顺序；会话不存在 404 |
| POST | `api/message/create` | `MessageSendPO`（请求体） | `MessageSendVO` | 写入用户消息并创建 `QUEUED` 的 Run |
| GET | `api/run/get` | `RunGetParams`（查询参数） | `Run` | Run 不存在 404 |
| POST | `api/run/cancel` | `RunCancelPO`（请求体） | `Run` | 只允许取消 `QUEUED` / `RUNNING`，否则 400 |
| GET | `api/run/stream` | `RunStreamParams`（查询参数） | SSE 事件流（`RunEvent`） | 订阅 Run 的事件流，只有 `QUEUED` 的 Run 可订阅 |

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
| `RunStreamParams` | `runId` | query | 是 | 目标 Run ID |

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
| `model` | `string \| null` | 生成该消息时使用的模型；用户消息为空 |
| `inputTokens` | `string \| null` | 输入 token 用量；未统计时为空 |
| `outputTokens` | `string \| null` | 输出 token 用量；未统计时为空 |
| `createdAtMs` | `string` | 创建时间（Unix 毫秒） |

### Run

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `string` | Run ID |
| `sessionId` | `string` | 所属会话 ID |
| `status` | `RunStatus` | `QUEUED` / `RUNNING` / `COMPLETED` / `CANCELLED` / `FAILED` |
| `model` | `string` | 本次执行使用的模型标识 |
| `error` | `string \| null` | 失败原因；仅 `FAILED` 时有值，且不含内部细节 |
| `inputTokens` | `string \| null` | 输入 token 用量；未统计时为空 |
| `outputTokens` | `string \| null` | 输出 token 用量；未统计时为空 |
| `createdAtMs` | `string` | 创建时间（Unix 毫秒） |
| `updatedAtMs` | `string` | 最后更新时间（Unix 毫秒） |
| `finishedAtMs` | `string \| null` | 结束时间（Unix 毫秒）；未结束时为空 |

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
| `error` | `string \| null` | 失败或终止原因；仅 `run_failed` 与 `server_shutdown` 有值，不含内部细节 |
| `inputTokens` | `string \| null` | 输入 token 用量；`run_started` 与 `run_completed` 有值（Mock 按字符数估算） |
| `outputTokens` | `string \| null` | 输出 token 用量；`run_completed` 有值（Mock 按字符数估算） |

### RunEventKind

| 取值 | 触发时机 | 额外有值的字段 |
|---|---|---|
| `run_started` | 订阅成功、Run 转为 `RUNNING` | `status`、`messageId`、`inputTokens` |
| `message_delta` | 助手回复的增量片段 | `delta` |
| `run_completed` | Run 正常结束，助手消息已落库 | `status`、`messageId`、`inputTokens`、`outputTokens` |
| `run_failed` | 执行失败；P1 的 Mock 不产出，留给 P2 的真实执行 | `error` |
| `server_shutdown` | 服务关闭，事件流被主动终止（Run 已落成 `CANCELLED`） | `status`、`error` |
| `heartbeat` | 空闲满 15 秒 | 仅公共字段 |

P2 追加事件（如 `tool_call_*`）只允许新增枚举值，不改已有事件的字段语义。

## SSE 事件流

`GET api/run/stream` 不走 `R<T>` 包装，直接返回 `text/event-stream`；元数据里它是 `Blob`（D-007），SDK 只导出 path 常量，事件消费由客户端自己实现。

| 项 | 约定 |
|---|---|
| content-type | `text/event-stream; charset=utf-8` |
| 响应头 | `cache-control: no-cache`、`x-accel-buffering: no`（避免中间层缓冲） |
| 帧格式 | `id: <seq>` + `event: <RunEventKind>` + `data: <RunEvent 的 JSON>`，空行结束一帧 |
| 事件名 | 与 `data` 里的 `event` 字段同值（snake_case），客户端任取其一判别 |
| 结束 | 服务端发出 `run_completed` 后关闭连接，没有额外的结束哨兵事件；服务关闭时改为发出 `server_shutdown` 后关闭连接（D-028） |
| 状态推进 | 订阅即把 Run 从 `QUEUED` 推进到 `RUNNING`；`run_completed` 发出前助手消息与 Run 终态已落库 |
| 断开 | 客户端断开（流被 drop）→ 生产方停止并释放任务，Run 落成 `CANCELLED`，不写半成品助手消息 |
| 取消 | 流式期间调用 `api/run/cancel` → 生产方在下一段增量前发现状态变化并停止推送，Run 保持 `CANCELLED` |
| 重复订阅 | 只有 `QUEUED` 可订阅；同一 Run 再次订阅返回 400，避免两个生产方写同一条助手消息 |
| 服务关闭 | 生产方在下一段增量前发现关闭标记 → Run 落成 `CANCELLED` → 发出 `server_shutdown`（含 `status` 与可读 `error`）→ 连接关闭；关闭期间的新请求返回 `503` `{"code":503,"message":"服务正在关闭，请稍后重试"}` |
| 客户端 | 浏览器用 **fetch 流式响应 + 自行切帧**（`lingting-agent-ui/src/api/runStream.ts`）：原生 `EventSource` 拿不到订阅失败的 4xx 错误体、断线会自动重连（重订阅被 400 拒绝）、也无法中断，见 D-007 与 A-6 的 P1-016 结论；path 取自生成的 `ApiDefinitions.runStream.path`。Rust 侧用生成的 path 常量 + 自己的 SSE 解析 |

## 请求与响应样例

```text
POST api/session/create
{"title":"第一个会话"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"id":"1842000000000000001","title":"第一个会话","model":"mock-model","messageCount":"0","createdAtMs":"1767225600000","updatedAtMs":"1767225600000"}}
```

```text
GET api/session/list

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":[{"id":"1842000000000000001","title":"第一个会话","model":"mock-model","messageCount":"1","createdAtMs":"1767225600000","updatedAtMs":"1767225601000"}]}
```

```text
POST api/message/create
{"sessionId":"1842000000000000001","content":"你好"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{
  "message":{"id":"1842000000000000002","sessionId":"1842000000000000001","runId":"1842000000000000003","role":"USER","content":"你好","model":null,"inputTokens":null,"outputTokens":null,"createdAtMs":"1767225601000"},
  "run":{"id":"1842000000000000003","sessionId":"1842000000000000001","status":"QUEUED","model":"mock-model","error":null,"inputTokens":null,"outputTokens":null,"createdAtMs":"1767225601000","updatedAtMs":"1767225601000","finishedAtMs":null}
}}
```

```text
GET api/message/list?sessionId=1842000000000000001

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":[{"id":"1842000000000000002","sessionId":"1842000000000000001","runId":"1842000000000000003","role":"USER","content":"你好","model":null,"inputTokens":null,"outputTokens":null,"createdAtMs":"1767225601000"}]}
```

```text
GET api/run/get?runId=1842000000000000003

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"id":"1842000000000000003","sessionId":"1842000000000000001","status":"QUEUED","model":"mock-model","error":null,"inputTokens":null,"outputTokens":null,"createdAtMs":"1767225601000","updatedAtMs":"1767225601000","finishedAtMs":null}}
```

```text
POST api/run/cancel
{"runId":"1842000000000000003"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"id":"1842000000000000003","sessionId":"1842000000000000001","status":"CANCELLED","model":"mock-model","error":null,"inputTokens":null,"outputTokens":null,"createdAtMs":"1767225601000","updatedAtMs":"1767225602000","finishedAtMs":"1767225602000"}}
```

```text
GET api/run/stream?runId=1842000000000000003

HTTP/1.1 200 OK
content-type: text/event-stream; charset=utf-8
cache-control: no-cache
x-accel-buffering: no

id: 0
event: run_started
data: {"event":"run_started","seq":"0","runId":"1842000000000000003","sessionId":"1842000000000000001","createdAtMs":"1767225602000","delta":null,"messageId":"1842000000000000004","status":"RUNNING","error":null,"inputTokens":"2","outputTokens":null}

id: 1
event: message_delta
data: {"event":"message_delta","seq":"1","runId":"1842000000000000003","sessionId":"1842000000000000001","createdAtMs":"1767225602120","delta":"收到你的","messageId":null,"status":null,"error":null,"inputTokens":null,"outputTokens":null}

id: 2
event: run_completed
data: {"event":"run_completed","seq":"2","runId":"1842000000000000003","sessionId":"1842000000000000001","createdAtMs":"1767225603000","delta":null,"messageId":"1842000000000000004","status":"COMPLETED","error":null,"inputTokens":"2","outputTokens":"52"}
```

## 错误响应

业务失败由 `service-core` 的 `SessionError` 表达，`lib-web` 统一映射为 404 / 400 / 500，响应体仍是 `{code, message}`（D-024）：

| 场景 | HTTP | `message` |
|---|---|---|
| `message/list`、`message/create` 的 `sessionId` 不存在 | 404 | `会话不存在：<sessionId>` |
| `run/get`、`run/cancel` 的 `runId` 不存在 | 404 | `Run 不存在：<runId>` |
| `message/create` 的 `content` 全为空白 | 400 | `消息内容不能为空` |
| 取消已结束的 Run | 400 | `Run 当前状态为 <STATUS>，不可取消` |
| `run/stream` 的 `runId` 不存在 | 404 | `Run 不存在：<runId>` |
| 订阅的 Run 不处于 `QUEUED`（正在执行或已结束） | 400 | `Run 当前状态为 <STATUS>，不可订阅事件流` |
| 请求体或查询参数缺失、类型不符 | 400 | 框架固定文案（`请求参数转换失败` / `查询参数转换失败` / `请求体不是有效的 JSON`） |
| 服务内部错误（如 ID 生成失败） | 500 | `服务器内部错误`，细节只进日志 |

## Mock 行为边界

- 存储是进程内内存（`Mutex<HashMap>`），**重启服务后会话、消息、Run 全部丢失**。
- 发送消息只写入用户消息与 `QUEUED` 的 Run，不产出助手回复：助手消息与 `QUEUED → RUNNING → COMPLETED` 由 `api/run/stream` 的 Mock 生产方完成，从未订阅的 Run 会一直停在 `QUEUED`。
- 助手回复的内容是回显用户输入（附一句说明），按 4 个字符一段、每段间隔 120ms 推送；`inputTokens` / `outputTokens` 按字符数估算，不是真实分词结果。
- `messageCount` 与 `updatedAtMs` 在每次发送消息时更新；取消 Run 不影响会话统计。
- 会话隔离由「消息按 `sessionId` 分桶」保证，不存在跨会话读取入口。
- P2 起：存储换 Repository，状态机换 Agent Runtime，`types-core` 的契约类型迁入 `agent-core`（字段与类型名不变），handler 与路径不变。
