# session 接口（Mock 会话业务）

会话 / 消息 / Run 的业务接口。P1 是 Mock 实现（内存存储、无真实 Agent 执行），P2 只替换实现、不改契约。通用约定（地址、响应包装、错误体、字段类型）见 [readme.md](readme.md)。

- 定义位置：`crates/lib-web/src/session/api.rs`（handler）、`crates/types-core/src/session/`（契约类型）
- 业务实现：`crates/service-core/src/session/`（P2 换成 Repository + Agent Runtime）
- 契约决策与 P2 替换点：`.steps/p1/decisions.md` D-025

## 接口清单

| 方法 | path | 入参 | 返回 | 说明 |
|---|---|---|---|---|
| POST | `api/session/create` | `SessionCreatePO`（请求体） | `SessionSummary` | 标题缺省为「新会话」，模型缺省为 `mock-model` |
| GET | `api/session/list` | 无 | `SessionSummary[]` | 按 `updatedAtMs` 倒序 |
| GET | `api/message/list` | `MessageListParams`（查询参数） | `Message[]` | 按写入顺序；会话不存在 404 |
| POST | `api/message/create` | `MessageSendPO`（请求体） | `MessageSendVO` | 写入用户消息并创建 `QUEUED` 的 Run |
| GET | `api/run/get` | `RunGetParams`（查询参数） | `Run` | Run 不存在 404 |
| POST | `api/run/cancel` | `RunCancelPO`（请求体） | `Run` | 只允许取消 `QUEUED` / `RUNNING`，否则 400 |

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

## Mock 行为边界

- 存储是进程内内存（`Mutex<HashMap>`），**重启服务后会话、消息、Run 全部丢失**。
- 发送消息只写入用户消息与 `QUEUED` 的 Run，不产出助手回复、不推进 Run 状态；未取消的 Run 会一直停在 `QUEUED`。
- `messageCount` 与 `updatedAtMs` 在每次发送消息时更新；取消 Run 不影响会话统计。
- 会话隔离由「消息按 `sessionId` 分桶」保证，不存在跨会话读取入口。
- P2 起：存储换 Repository，状态机换 Agent Runtime，`types-core` 的契约类型迁入 `agent-core`（字段与类型名不变），handler 与路径不变。
