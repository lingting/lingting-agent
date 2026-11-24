# tool 接口（工具审计）

工具调用的审计流水：一次调用一条记录，可按 Run 查询。P3 起由 `agent-runtime` 在执行期写入，本领域只提供只读查询；工具的注册表与执行器不经过 HTTP 接口（`crates/agent-tool`）。

- 定义位置：`crates/lib-web/src/tool/api.rs`（handler）、`crates/agent-tool/src/audit.rs`（`ToolCallVO` / `ToolAuditListParams`）
- 业务入口：`manager-agent::list_tool_calls` → `service-core` 的 `SessionService::tool_calls_of_run`；SQL 在 `crates/service-core/src/repository/tool.rs`
- 决策：`.steps/p3/decisions.md` D-P3-011（表结构与写出侧脱敏）、D-P3-012（按 Run 查询、无分页）

## 接口清单

| 方法 | path | 入参 | 返回 | 说明 |
|---|---|---|---|---|
| GET | `api/tool/audit/list` | `ToolAuditListParams`（查询参数） | `ToolCallVO[]` | 按 Run 查询，按产生时间升序；未知 `runId` 返回 `[]` |

## 入参

| 类型 | 字段 | 位置 | 必填 | 说明 |
|---|---|---|---|---|
| `ToolAuditListParams` | `runId` | query | 是 | 目标 Run ID；缺失或类型不符返回 400 |

## 载荷类型

### ToolCallVO

审计记录的契约快照：字段与落库的 `tool_calls` 行同构，`arguments` / `result` 都是**写出侧已脱敏、已截断**的文本。

| 字段 | 类型 | 说明 |
|---|---|---|
| `toolCallId` | `string` | 模型给出的调用标识；与 `tool_call_*` 事件、工具结果消息同值 |
| `toolName` | `string` | 工具名 |
| `fileAccess` | `ToolFileAccess` | 本地文件操作声明：`NONE` / `READ` / `WRITE` / `READ_WRITE` |
| `status` | `ToolCallStatus` | `OK` / `ERROR` / `DENIED` / `TIMEOUT` / `CANCELLED` / `INVALID_ARGUMENTS` / `PENDING` |
| `arguments` | `string` | 已脱敏并截断的参数 JSON 文本（上限 2048 字符） |
| `result` | `string \| null` | 结果或错误文本（按 `[tools] max_result_chars` 截断）；挂起时为空 |
| `durationMs` | `string` | 耗时（毫秒，含重试），`i64` → 字符串 |
| `toolRound` | `number` | 第几轮工具调用（从 1 开始） |
| `createdAtMs` | `string` | 记录产生时间（Unix 毫秒） |

说明：

1. `PENDING` 是挂起等待确认期间的中间态（D-P3-023）；客户端确认或驳回后由同一条记录收尾，因此一次调用始终只有一条记录。
2. 敏感键（`api_key` / `token` / `secret` / `password` / `authorization` / `cookie` 等，大小写不敏感）的值在写出侧替换为 `***`，查询接口不做二次处理。

## 请求与响应样例

```text
GET api/tool/audit/list?runId=1842000000000000003

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":[{"toolCallId":"call_1","toolName":"calculator","fileAccess":"NONE","status":"OK","arguments":"{\"expression\":\"2+3*4\"}","result":"14","durationMs":"1","toolRound":1,"createdAtMs":"1767225602000"}]}
```

```text
GET api/tool/audit/list?runId=1842000000000009999

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":[]}
```

## 错误响应

| 场景 | HTTP | `message` |
|---|---|---|
| `runId` 缺失或类型不符 | 400 | 框架固定文案（`查询参数转换失败`） |
| 存储异常 | 500 | `服务器内部错误`，细节只进日志 |

审计记录不是会话对象：未知 `runId` 不返回 404，而是空列表（D-P3-012）。
