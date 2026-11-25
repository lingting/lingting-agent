# runtime 接口（客户端连接）

客户端连接实体的注册、上行消息（心跳 / 问询应答 / 工具确认）、主动断开与**唯一的下行事件流**。通用约定（地址、响应包装、错误体、字段类型）见 [readme.md](readme.md)。

- 定义位置：`crates/lib-web/src/runtime/{api,stream}.rs`（handler 与帧合成）、`crates/types-core/src/runtime/`（契约类型）
- 业务实现：`crates/manager-agent/src/runtime/`（注册表、心跳记账、问询判定、工具确认链路、断连处理链）
- 契约决策：`.steps/p1.1/decisions.md` D-P1.1-001（传输与断开时序）、D-P1.1-005（固定 message）、D-P1.1-006（心跳与问询参数）；`.steps/p3/decisions.md` D-P3-022（上行通道改名与承载确认）、D-P3-023（挂起与唤醒）

## 接口清单

| 方法 | path | 入参 | 返回 | 说明 |
|---|---|---|---|---|
| POST | `api/client/create` | `ClientCreatePO`（请求体） | `ClientCreateVO` | 注册；`clientId` 由服务端生成，响应里的首个 pong 即注册成功信号 |
| GET | `api/client/stream` | `ClientStreamParams`（查询参数） | SSE 事件流（`ClientEvent`） | `text/event-stream`，全系统唯一的服务端推送通道；建立后补发挂起中的确认请求 |
| POST | `api/client/post` | `ClientPostPO`（请求体） | `ClientPostVO` | 上行消息：心跳（`PING`）、问询应答（`INQUIRY_ACK`）或工具确认（`TOOL_CONFIRM`）；pong 只在这条响应里 |
| POST | `api/client/delete` | `ClientDeletePO`（请求体） | `Void`（204） | 主动断开；只打标记，反注册与资产清理由断连处理链完成 |

`clientId` 不出现在路径里（D-008），只走请求体或查询参数。下行流必须是 `GET`（浏览器 `EventSource` 的限制），上行一律 `POST` + JSON 请求体。

上行通道在 P3 由 `update` 动作改为 `post`（D-P3-022）：上行只有一条通道，按 `kind` 分派语义，不保留旧路径、不同时注册两代。

## 入参

| 类型 | 字段 | 位置 | 必填 | 说明 |
|---|---|---|---|---|
| `ClientCreatePO` | — | body | — | 注册动作不携带字段，传 `{}` |
| `ClientStreamParams` | `clientId` | query | 是 | 目标客户端 ID |
| `ClientPostPO` | `clientId` | body | 是 | 目标客户端 ID |
| `ClientPostPO` | `kind` | body | 是 | `PING` / `INQUIRY_ACK` / `TOOL_CONFIRM` |
| `ClientPostPO` | `toolCallId` | body | 否 | 待确认的调用标识；只有 `TOOL_CONFIRM` 允许非空 |
| `ClientPostPO` | `decision` | body | 否 | `APPROVE` / `REJECT`；只有 `TOOL_CONFIRM` 允许非空 |
| `ClientDeletePO` | `clientId` | body | 是 | 目标客户端 ID |

字段组合是硬约束：`PING` / `INQUIRY_ACK` 不得携带确认字段，`TOOL_CONFIRM` 必须两个字段齐全，否则 400 + 固定 message（见「错误响应」）。

## 载荷类型

### ClientCreateVO

| 字段 | 类型 | 说明 |
|---|---|---|
| `clientId` | `string` | 客户端 ID，每次注册都不同 |
| `pongAtMs` | `string` | 首个 pong 的产生时间（Unix 毫秒） |

### ClientPostVO

| 字段 | 类型 | 说明 |
|---|---|---|
| `pongAtMs` | `string` | 本次 pong 的产生时间（Unix 毫秒） |

### ClientPostKind

| 取值 | 含义 |
|---|---|
| `PING` | 心跳：刷新 `lastSeen`，响应体回 pong |
| `INQUIRY_ACK` | 问询应答：只清掉待答状态，**不**刷新 `lastSeen`（否则超时判定失效） |
| `TOOL_CONFIRM` | 工具确认：对一次挂起的工具调用给出决定；同时刷新 `lastSeen` 并回 pong（一次确认被当作一次心跳） |

### ClientDecision

| 取值 | 含义 |
|---|---|
| `APPROVE` | 批准：唤醒后执行这次调用 |
| `REJECT` | 驳回：不再执行，把「用户拒绝调用该工具」回灌给模型 |

没有「总是允许」与批量确认：一次决定只对一次调用有效（D-P3-026）。

### ClientEvent

下行流的一帧载荷。判别字段 `event` 与 SSE 的 `event:` 行同值。

| 字段 | 类型 | 说明 |
|---|---|---|
| `event` | `ClientEventKind` | 事件类型，取值见下 |
| `seq` | `string` | 客户端内的事件序号，从 `0` 起递增，同帧的 `id:`；与 `RunEvent.seq` **不同源** |
| `createdAtMs` | `string` | 事件产生时间（Unix 毫秒） |
| `sessionEvent` | `RunEvent \| null` | `session_event` 的载荷，字段见 [session.md](session.md) |
| `inquiryAtMs` | `string \| null` | `inquiry` 的投递时刻 |
| `inquiryDeadlineMs` | `string \| null` | `inquiry` 的应答截止时刻 |
| `closeReason` | `ClientCloseReason \| null` | `client_closed` 的关闭原因 |

### ClientEventKind

| 取值 | 触发时机 | 有值的字段 |
|---|---|---|
| `inquiry` | `lastSeen` 超过 30 秒未前进，服务端要求客户端应答 | `inquiryAtMs`、`inquiryDeadlineMs` |
| `session_event` | 客户端占用的运行时会话产出一条 `RunEvent` | `sessionEvent` |
| `client_closed` | 服务端即将关闭这条下行流（该帧是最后一帧） | `closeReason` |

工具确认请求不新增下行事件类型：它以 `session_event` 承载 `RunEvent.event = tool_call_confirm_required`（见 [session.md](session.md) 的事件表）。

### ClientCloseReason

| 取值 | 来源 |
|---|---|
| `HEARTBEAT_TIMEOUT` | `lastSeen` 超过 TTL（45 秒）未刷新 |
| `INQUIRY_TIMEOUT` | 问询投递后 10 秒内没有 `INQUIRY_ACK` |
| `CLIENT_CLOSED` | 客户端主动调 `api/client/delete` |
| `STREAM_CLOSED` | 下行流被断开，或向下行通道投递失败（含慢客户端） |
| `SERVER_SHUTDOWN` | 服务进入关闭流程 |
| `JOURNAL_ERROR` | 事件文件写入失败 |

同一次断连只保留**第一次**的原因；六条来源共用一条处理链，重复触发不会重复处理。

## 下行事件流

`GET api/client/stream` 不走 `R<T>` 包装，直接返回 `text/event-stream`；元数据里它是 `Blob`（D-007），SDK 只导出 path 常量，帧由客户端自行解析。

| 项 | 约定 |
|---|---|
| content-type | `text/event-stream; charset=utf-8` |
| 响应头 | `cache-control: no-cache`、`x-accel-buffering: no` |
| 帧格式 | `id: <ClientEvent.seq>` + `event: <ClientEventKind>` + `data: <ClientEvent 的 JSON>`，空行结束一帧 |
| 事件名 | 与 `data` 里的 `event` 字段同值（snake_case），客户端任取其一判别 |
| 保活 | 下行流空闲时每 15 秒一条注释帧 `: keepalive`；它不进入 `ClientEvent`、不占 `seq`、不落事件文件，只充当写失败探测 |
| 补发 | 流建立成功后，对该客户端**当前占用会话**下每个 `pending_tool_call_id` 非空的 Run 各补发一次 `tool_call_confirm_required`（同一 `toolCallId`，`seq` 由写者继续分配） |
| 结束 | 服务端主动关流时先发一帧 `client_closed`（带关闭原因），随后结束响应体；流已被对端断开时这一帧发不出去，属预期路径 |
| 续传 | **不支持**：服务端忽略 `Last-Event-ID`，不重放事件；`id` 只用于丢帧检测与排障 |
| 断开感知 | 响应体被 drop 即判定该流已断，服务端标记 `STREAM_CLOSED` 并交给断连处理链 |
| 静默窗口 | 已打标记但处理链尚未消化的窗口期内，该客户端的 ping 仍然成功，但不再收到事件、标记也不会被 ping 清除 |

补发只覆盖「同一个 `clientId` 重新建立下行流」这条路径：完整断连（含反注册）会按 D-P3-023 把该会话的未结束 Run 收尾为 `CANCELLED` 并清空挂起列，重连后没有可补发的确认请求。

## 工具确认链路

| 步 | 行为 |
|---|---|
| 1 | 模型请求调用一个 `confirm` 级工具：Run 由 `RUNNING` 转 `WAITING`，写入 `runs.pending_tool_call_id` / `pending_tool_round`，插入 `status=PENDING` 的审计记录，随后下发 `tool_call_confirm_required`，本次执行结束并让出并发许可 |
| 2 | 客户端调 `api/client/post { kind: TOOL_CONFIRM, toolCallId, decision }` |
| 3 | 服务端鉴权：`clientId` 已注册（未注册 → 400 未注册文案）、该客户端正占用该会话（否则 400 `客户端未占用会话`）、`toolCallId` 等于该会话某个 Run 的 `pending_tool_call_id`（否则 400 `待确认的工具调用不存在或已处理`） |
| 4 | 条件更新把 Run 从 `WAITING` 退回 `QUEUED` 并清空挂起列；驳回时同事务把审计置 `DENIED`；随后唤醒会话队列，由队列重新取这个 Run |
| 5 | 唤醒后按 `APPROVE` 执行该次调用（审计转 `OK` / `ERROR` 等）或按 `REJECT` 回灌固定文案，然后继续推理 |
| 6 | 取消 Run / 释放占用 / 断连 / 服务关闭都会清空挂起列，并把仍为 `PENDING` 的审计收尾为 `CANCELLED` |

挂起不设超时：确认是人的动作，机器不代替人做「不确认 = 拒绝」的决定。

## 心跳与问询

参数全部是常量，不开放配置（D-P1.1-006）：

| 项 | 值 | 说明 |
|---|---|---|
| 客户端心跳间隔 | 15 秒 | 客户端自行定时 `POST api/client/post {PING}` |
| 心跳超时 `CLIENT_TTL` | 45 秒 | `now - lastSeen > 45s` 即标记 `HEARTBEAT_TIMEOUT` |
| 扫描周期 | 5 秒 | 定时任务只做「判定 + 标记 + 投递」 |
| 问询时机 `INQUIRY_AFTER` | 30 秒 | `lastSeen` 超过 30 秒未前进就投递一次 `inquiry` |
| 问询窗口 `INQUIRY_WINDOW` | 10 秒 | 窗口内没有 `INQUIRY_ACK` 即标记 `INQUIRY_TIMEOUT` |

- 同一客户端同一时刻最多一条未答问询；问询未送达（没有下行流、通道满）时不记待答状态。
- `pong` 只出现在 `client/create` 与 `client/post` 的同步响应体里，**不**进下行流，一次心跳只有一帧。

## 请求与响应样例

```text
POST api/client/create
{}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"clientId":"1842000000000000001","pongAtMs":"1767225600000"}}
```

```text
POST api/client/post
{"clientId":"1842000000000000001","kind":"PING"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"pongAtMs":"1767225615000"}}
```

```text
POST api/client/post
{"clientId":"1842000000000000001","kind":"INQUIRY_ACK"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"pongAtMs":"1767225630000"}}
```

```text
POST api/client/post
{"clientId":"1842000000000000001","kind":"TOOL_CONFIRM","toolCallId":"call-7","decision":"APPROVE"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"pongAtMs":"1767225635000"}}
```

```text
GET api/client/stream?clientId=1842000000000000001

HTTP/1.1 200 OK
content-type: text/event-stream; charset=utf-8
cache-control: no-cache
x-accel-buffering: no

id: 0
event: inquiry
data: {"event":"inquiry","seq":"0","createdAtMs":"1767225630000","sessionEvent":null,"inquiryAtMs":"1767225630000","inquiryDeadlineMs":"1767225640000","closeReason":null}

: keepalive

id: 1
event: session_event
data: {"event":"session_event","seq":"1","createdAtMs":"1767225630120","sessionEvent":{"event":"message_delta","seq":"1","runId":"1842000000000000003","sessionId":"1842000000000000005","createdAtMs":"1767225630120","delta":"收到你的","messageId":null,"status":null,"error":null,"inputTokens":null,"outputTokens":null},"inquiryAtMs":null,"inquiryDeadlineMs":null,"closeReason":null}

id: 2
event: client_closed
data: {"event":"client_closed","seq":"2","createdAtMs":"1767225645000","sessionEvent":null,"inquiryAtMs":null,"inquiryDeadlineMs":null,"closeReason":"HEARTBEAT_TIMEOUT"}
```

```text
POST api/client/delete
{"clientId":"1842000000000000001"}

HTTP/1.1 204 No Content
```

## 错误响应

| 场景 | HTTP | `message` |
|---|---|---|
| `clientId` 未注册或已被反注册（`stream` / `post` / `delete`） | 400 | `客户端未注册，请重新连接` |
| `TOOL_CONFIRM` 缺 `toolCallId` | 400 | `工具确认缺少 toolCallId` |
| `TOOL_CONFIRM` 缺 `decision` | 400 | `工具确认缺少 decision` |
| `PING` / `INQUIRY_ACK` 携带 `toolCallId` 或 `decision` | 400 | `心跳与问询应答不得携带工具确认字段` |
| `TOOL_CONFIRM` 来自未占用会话的客户端 | 400 | `客户端未占用会话` |
| `toolCallId` 不存在、已处理或状态不匹配（重复确认、过期标识） | 400 | `待确认的工具调用不存在或已处理` |
| `kind` 不在三个取值内 | 400 | 框架固定文案 `请求体不是有效的 JSON` |
| 请求体或查询参数缺失、类型不符 | 400 | 框架固定文案（`请求参数转换失败` / `查询参数转换失败` / `请求体不是有效的 JSON`） |
| 服务内部错误 | 500 | `服务器内部错误`，细节只进日志 |

`客户端未注册，请重新连接` 是**过渡期**的固定 message（D-P1.1-005）：框架暂不支持自定义错误码，客户端按 message 识别并重新走注册流程，服务端与客户端不必按 code 猜测。等框架支持自定义 code 后只改映射函数，判断位置不变。

## 行为边界

- 客户端记录只存在进程内：服务重启后全部丢失，客户端按「400 客户端未注册」重新注册（不做持久化，也不按事件文件恢复会话）。
- 反注册只按 `clientId` 精确移除，重连生成新的 `clientId`，因此新旧连接不会互相误清理。
- 一个客户端同时只占用一个会话（见 [occupancy.md](occupancy.md)），因此这条下行流内不会混杂两个会话的事件；工具确认也因此只能由占用该会话的客户端提交。
- 下行流断开即视为该客户端断连：其占用的运行时会话被停止，挂起的 Run 收尾为 `CANCELLED`，客户端需要重连并重新占用才能继续。
