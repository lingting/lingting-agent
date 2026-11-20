# occupancy 接口（会话占用）

`sessionId ↔ clientId` 的**一对一**独占关系：占用成功即启动该会话的运行时会话并打开事件文件；释放即停止它。通用约定（地址、响应包装、错误体、字段类型）见 [readme.md](readme.md)。

- 定义位置：`crates/lib-web/src/runtime/occupancy_api.rs`（handler）、`crates/types-core/src/runtime/occupancy.rs`（契约类型）
- 业务实现：`crates/service-core/src/runtime/{occupancy,registry,journal,session}.rs`
- 契约决策：`.steps/p1.1/decisions.md` D-P1.1-002（建立顺序、冲突表达与 Run 联动）、D-P1.1-003（事件文件与轮转）、D-P1.1-008（一对一约束）

## 接口清单

| 方法 | path | 入参 | 返回 | 说明 |
|---|---|---|---|---|
| POST | `api/occupancy/create` | `OccupancyCreatePO`（请求体） | `OccupancyVO` | 建立占用；成功即启动运行时会话、打开事件文件 |
| POST | `api/occupancy/delete` | `OccupancyDeletePO`（请求体） | `Void`（204） | 释放占用；幂等，没有占用时同样成功 |
| GET | `api/occupancy/get` | `OccupancyGetParams`（查询参数） | `OccupancyVO` | 查询当前占用者；无人占用 404 |

## 入参

| 类型 | 字段 | 位置 | 必填 | 说明 |
|---|---|---|---|---|
| `OccupancyCreatePO` | `clientId` | body | 是 | 占用方客户端 ID，必须已注册 |
| `OccupancyCreatePO` | `sessionId` | body | 是 | 被占用的会话 ID，必须存在 |
| `OccupancyDeletePO` | `clientId` | body | 是 | 释放方客户端 ID（一对一约束下不需要 `sessionId`） |
| `OccupancyGetParams` | `sessionId` | query | 是 | 目标会话 ID |

## 载荷类型

### OccupancyVO

| 字段 | 类型 | 说明 |
|---|---|---|
| `sessionId` | `string` | 被占用的会话 ID |
| `clientId` | `string` | 占用方客户端 ID |
| `runtimeSessionId` | `string` | 本次占用对应的运行时会话 ID；每次占用重新生成，也是事件文件名 |
| `acquiredAtMs` | `string` | 占用建立时间（Unix 毫秒） |

## 语义

### 校验顺序

固定为「注册 → 会话存在 → 客户端一对一 → 会话独占」，因此同一个请求只可能命中一条错误，客户端不需要按优先级猜测：

1. `clientId` 不在注册表（含已反注册）→ 400 `客户端未注册，请重新连接`
2. `sessionId` 不存在 → 404 `会话不存在：<sessionId>`
3. 该 `clientId` 已占用**其他**会话 → 400 `客户端已占用其他会话`
4. `sessionId` 已被**其他** `clientId` 占用 → 400 `会话已被其他客户端占用`

### 一对一约束

- 一个客户端同时只能占用一个会话：占用 B 之前必须先释放 A，服务端**不做**隐式切换、抢占或自动让出。
- 一个会话同时只能被一个客户端占用，因此这条下行流内不会混杂两个会话的事件。
- 不做隐式切换的理由：一次请求里既要停旧运行时会话（落终态、flush、关文件）又要建新会话，中途失败无法回滚；拆成两个幂等请求后，任一步失败都能原样重试。

### 幂等口径

| 动作 | 重复调用 |
|---|---|
| `create`（同一对 `clientId` + `sessionId`） | 返回当前 `OccupancyVO`，不重建运行时会话、不重开事件文件、不重复落盘 |
| `delete`（该客户端没有占用） | 204，与首次释放一致 |

### 建立顺序

「先登记占用，再建运行时会话」：登记是纯内存动作、可回滚；文件与任务不可回滚。文件打开失败时立即撤回占用登记并返回 500 `服务器内部错误`，因此除补偿瞬间外不存在「有登记无会话」的状态。

### 释放与 Run 的联动

释放占用、断连处理链、服务关闭三条来源共用同一段逻辑：

1. 停止 Run 执行队列，该会话 `QUEUED` / `RUNNING` 的 Run 一律落 `CANCELLED`（带 `finishedAtMs`）。
2. 追加终态事件（`run_cancelled` 或 `server_shutdown`）并 flush，随后关闭文件；文件保留、不删除。
3. 撤回占用表的双向登记。

释放**不关下行通道**：客户端仍在连接，只是不再收到该会话的 `session_event`，也不会收到 `client_closed`（那一帧只在反注册时发）。

### 事件文件

| 项 | 约定 |
|---|---|
| 路径 | `<应用数据目录>/events/<sessionId>/<runtimeSessionId>.jsonl`（与 P2 的 `agent.db` 同级） |
| 格式 | 行分隔 JSON，一行一条 `RunEvent`（字段见 [session.md](session.md)），`\n` 结尾，UTF-8 |
| `seq` | 文件内从 `0` 单调递增，由写者覆写，不信任事件自带的值 |
| 写序 | 先落盘并 flush，再转发到 `api/client/stream`；转发失败不回滚 |
| 终态 | `run_completed` / `run_failed` / `run_cancelled` / `server_shutdown` 之一收尾，之后不再追加 |
| 轮转 | 按会话目录数量（`max_sessions`，默认 200）与 `data/events` 总字节数（`max_bytes`，默认 512 MiB）清理，占用中的目录只计不删；`0` 表示该维度不限 |

## 请求与响应样例

```text
POST api/occupancy/create
{"clientId":"1842000000000000001","sessionId":"1842000000000000005"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"sessionId":"1842000000000000005","clientId":"1842000000000000001","runtimeSessionId":"1842000000000000009","acquiredAtMs":"1767225620000"}}
```

```text
GET api/occupancy/get?sessionId=1842000000000000005

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"sessionId":"1842000000000000005","clientId":"1842000000000000001","runtimeSessionId":"1842000000000000009","acquiredAtMs":"1767225620000"}}
```

```text
POST api/occupancy/delete
{"clientId":"1842000000000000001"}

HTTP/1.1 204 No Content
```

## 错误响应

| 场景 | HTTP | `message` |
|---|---|---|
| `clientId` 未注册或已被反注册 | 400 | `客户端未注册，请重新连接` |
| 会话已被其他客户端占用 | 400 | `会话已被其他客户端占用` |
| 客户端已占用其他会话 | 400 | `客户端已占用其他会话` |
| 会话不存在 | 404 | `会话不存在：<sessionId>` |
| 查询占用但无人占用 | 404 | `会话未被占用` |
| 建立运行时会话失败（建目录 / 开文件 / 起任务） | 500 | `服务器内部错误`，细节只进日志 |
| 请求体或查询参数缺失、类型不符 | 400 | 框架固定文案（`请求参数转换失败` / `查询参数转换失败` / `请求体不是有效的 JSON`） |

三条冲突 message 是**过渡期**的固定文案（D-P1.1-005），映射集中在 `crates/lib-web/src/runtime/mod.rs` 的一个函数里；框架支持自定义 code 后只改这一处，客户端的判断位置不变。
