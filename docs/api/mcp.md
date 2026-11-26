# mcp 接口（MCP 管理）

MCP server 的运行期管理面：看连接状态、看已发现的工具、触发启停 / 重连 / 探测。P4 起由 `agent-mcp` 的连接门面提供服务端状态，本领域只暴露只读快照与一个动作入口；工具的注册表与执行器仍走 `agent-tool`，MCP 工具在运行时眼里与内置工具同构。

- 定义位置：`crates/lib-web/src/mcp/api.rs`（handler）、`crates/lib-web/src/mcp/mod.rs`（错误映射）、`crates/agent-mcp/src/vo.rs`（`McpServerVO` / `McpToolVO` / `McpServerUpdatePO` / `McpToolListParams` 与两个枚举），契约类型经 `manager-agent` 转发
- 业务入口：`manager-agent::mcp_servers` / `mcp_tools` / `mcp_update` → `agent-mcp` 的 `McpManager`（快照、四个幂等动作、退避重连）
- 决策：`.steps/p4/decisions.md` D-P4-016（三条接口与动作段）、D-P4-008（启停只改进程内状态）、D-P4-019（状态只经接口与 `metrics` 暴露，不新增 Run 事件）

## 接口清单

| 方法 | path | 入参 | 返回 | 说明 |
|---|---|---|---|---|
| GET | `api/mcp/server/list` | 无 | `McpServerVO[]` | 配置里的全部 server（含 `DISABLED`），按 `id` 升序 |
| GET | `api/mcp/tool/list` | `McpToolListParams`（查询参数） | `McpToolVO[]` | 当前已发现的工具快照，按模型可见名升序；未知 `serverId` 返回 `[]` |
| POST | `api/mcp/server/update` | `McpServerUpdatePO`（请求体） | `McpServerVO` | 四个动作都返回**动作完成后**的状态 |

## 入参

| 类型 | 字段 | 位置 | 必填 | 说明 |
|---|---|---|---|---|
| `McpToolListParams` | `serverId` | query | 否 | 只返回该 server 的工具；缺省返回全部，未知 `id` 返回 `[]`（不是 404） |
| `McpServerUpdatePO` | `serverId` | body | 是 | 目标 server 标识；未知 `id` 返回 400 + `MCP 服务器不存在` |
| `McpServerUpdatePO` | `action` | body | 是 | `ENABLE` / `DISABLE` / `RECONNECT` / `PROBE` |

## 载荷类型

### McpStatusKind

连接状态，五个取值；与 `agent-mcp` 的状态机、`metrics` 的 `mcp` 段计数口径一致。

| 取值 | 含义 |
|---|---|
| `DISABLED` | 配置存在但未启用（`enabled = false`，或被 `DISABLE` 动作关掉） |
| `CONNECTING` | 正在建 transport / 握手 |
| `READY` | 已握手成功：可以列工具、可以调用 |
| `FAILED` | 连接不可用，原因在 `lastError`；按退避自动重连，也可手动 `RECONNECT` |
| `STOPPED` | 服务已关闭，拒绝新动作与新调用；只在关闭流程中出现，正常请求不会看到 |

### McpAction

动作取值，四个；全部幂等（同一动作重复调用不产生第二条连接、不重复注销工具）。

| 取值 | 语义 |
|---|---|
| `ENABLE` | 连接并发现工具；已 `READY` 时直接返回当前状态，不建第二条连接 |
| `DISABLE` | 断开并注销该 server 的全部工具；已 `DISABLED` 时直接返回 |
| `RECONNECT` | 先关闭旧连接，再按 `ENABLE` 走一遍 |
| `PROBE` | 只读探测：`READY` 时真发一次 `tools/list` 校验连通性，其余状态直接返回状态本身，不建连接 |

### McpServerVO

一个 server 的状态快照。

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `string` | 配置里的 server 标识 |
| `transport` | `string` | 传输方式标签，`stdio` / `http`（小写字面量，不是枚举） |
| `enabled` | `boolean` | 进程内的启用标记；`DISABLE` 后为 `false` |
| `status` | `McpStatusKind` | 当前状态 |
| `toolCount` | `number` | 该 server 当前注册在册的 MCP 工具数 |
| `lastError` | `string \| null` | 最近一次失败的可读原因（不含 URL、请求头、命令行） |
| `lastConnectedAtMs` | `string \| null` | 最近一次连接成功的时间（Unix 毫秒） |
| `nextRetryAtMs` | `string \| null` | 下一次自动重连的时间（Unix 毫秒）；未排重连时为空 |

### McpToolVO

一条已发现的 MCP 工具。

| 字段 | 类型 | 说明 |
|---|---|---|
| `serverId` | `string` | 所属 server 的标识 |
| `toolName` | `string` | 模型可见名 `mcp__<server>__<tool>`；事件与审计里的 `toolName` 就是它 |
| `originalName` | `string` | server 里的工具原名（调 `tools/call` 时用它） |
| `description` | `string` | 工具说明 |
| `parameters` | `string` | 原样透传的参数 JSON Schema 文本；前端不解析，只展示 |

说明：

1. `parameters` 是文本而不是对象：各 server 的 Schema 方言不一致，服务端不做归一，也不校验参数（校验口径同内置工具，D-P3-002）。
2. 清单只含**当前注册在册**的工具：连接成功后发现的工具在 `DISABLE` / `RECONNECT` / 服务关闭时注销，`deny` 的工具在注册环节就不进入；连接异常断开时快照可能仍保留上一次发现的条目，此时以 `status` 为准（`FAILED` 表示这一路已不可用）。
3. 状态与工具数都是进程内的易失事实，不做持久化：重启后回到 `mcp.toml` 的配置（`enabled` 与 server 清单），`DISABLE` / `ENABLE` 的结果不保留。

## 请求与响应样例

```text
GET api/mcp/server/list

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":[{"id":"filesystem","transport":"stdio","enabled":true,"status":"READY","toolCount":2,"lastError":null,"lastConnectedAtMs":"1767225600000","nextRetryAtMs":null},{"id":"search","transport":"http","enabled":false,"status":"DISABLED","toolCount":0,"lastError":null,"lastConnectedAtMs":null,"nextRetryAtMs":null}]}
```

```text
GET api/mcp/tool/list?serverId=filesystem

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":[{"serverId":"filesystem","toolName":"mcp__filesystem__read_file","originalName":"read_file","description":"读取一个文件的全文","parameters":"{\"type\":\"object\",\"properties\":{\"path\":{\"type\":\"string\"}},\"required\":[\"path\"]}"}]}
```

```text
POST api/mcp/server/update
{"serverId":"filesystem","action":"RECONNECT"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"id":"filesystem","transport":"stdio","enabled":true,"status":"READY","toolCount":2,"lastError":null,"lastConnectedAtMs":"1767225612000","nextRetryAtMs":null}}
```

`PROBE` 对已 `DISABLED` 的 server 不建立连接，返回 200 与状态本身：

```text
POST api/mcp/server/update
{"serverId":"search","action":"PROBE"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"id":"search","transport":"http","enabled":false,"status":"DISABLED","toolCount":0,"lastError":null,"lastConnectedAtMs":null,"nextRetryAtMs":null}}
```

## 错误响应

| 场景 | HTTP | `message` |
|---|---|---|
| `serverId` 缺失或类型不符 | 400 | 框架固定文案（`请求体不是有效的 JSON` / `请求参数转换失败`） |
| `action` 缺失或不在四个取值内 | 400 | 框架固定文案（`请求体不是有效的 JSON`） |
| `serverId` 不在配置里 | 400 | 固定 `MCP 服务器不存在` |
| 门面未装配、内部失败 | 500 | 固定 `服务器内部错误`，细节只进日志 |

未知 `serverId` 是 400 而不是 404：它是请求参数里的标识，与运行时的「客户端未注册」同一套判定口径（客户端按 `message` 识别）；反过来，`tool/list` 的未知 `serverId` 返回空列表，因为工具快照是「此刻发现了什么」，不是会话对象。

连接失败本身不是接口错误：它体现在 `status = FAILED` 与 `lastError` 上，接口照常返回 200。

## 与配置的关系

`mcp.toml` 是唯一持久化来源（D-P4-008）：

- `server/list` 里的 `enabled` 初值来自配置；`ENABLE` / `DISABLE` 只改进程内状态。
- `RECONNECT` / 自动重连不改配置。
- 重启后回到配置值：想让某个 server 默认关闭，改 `mcp.toml` 的 `enabled`，不要靠接口。
