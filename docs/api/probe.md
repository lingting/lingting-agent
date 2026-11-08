# probe 接口（技术验证探针与探活）

`probe` 不是业务领域，它承担三件事：验证 `web_api` 宏的能力边界、提供错误响应与错误码的回归基线、提供客户端探活入口。通用约定（地址、响应包装、错误体、字段类型）见 [readme.md](readme.md)。

- 定义位置：`crates/lib-web/src/probe/{api.rs,dto.rs}`
- 默认鉴权：`typed` / `health` / `version` / `shutdown` 显式声明 `AuthRule::anonymous()`，其余沿用 `auth` 默认值（P1 未启用鉴权，对本地客户端无影响）
- 探针服务启动：`cargo run -p lib-web --example api_probe_server`（默认 `127.0.0.1:20103`，可用 `AGENT_PROBE_PORT` 改端口），启动日志打印收集到的路由数
- 结论与实测记录：`docs/reports/p1-004-api-probe.md`、`.steps/p1/decisions.md` D-024

## 接口清单

| 方法 | path | 入参 | 返回 | 覆盖点 |
|---|---|---|---|---|
| GET | `api/probe/void` | 无 | 无（204） | 空响应 |
| POST | `api/probe/echo` | `ProbeEchoPO` | `ProbeEchoVO` | `Json<T>` 请求体、`Vec` 与 `Option` 字段 |
| GET | `api/probe/query` | `ProbeQueryPO` | `ProbeQueryVO` | `Query<T>` 查询参数 |
| GET | `api/probe/typed` | `ProbeQueryPO` | `ProbeQueryVO` | `R<T>` 包装 + 显式 `AuthRule::anonymous()` |
| GET | `api/probe/fail` | `ProbeQueryPO` | `ProbeQueryVO` | 业务参数错误（`size > 100` 时 400） |
| GET | `api/probe/response` | 无 | `WebResponse` | 自定义响应头，元数据为 `Blob` |
| GET | `api/probe/response_of` | 无 | `WebResponseOf<ProbeQueryVO>` | 自定义响应头 + 类型化载荷 |
| GET | `api/probe/internal` | 无 | `ProbeQueryVO` | 内部错误对外固定文案 |
| GET | `api/probe/panic` | 无 | `ProbeQueryVO` | panic 兜底后响应体不含堆栈 |
| GET | `api/probe/health` | 无 | `ProbeHealthVO` | 客户端探活 |
| GET | `api/probe/version` | 无 | `ProbeVersionVO` | 版本与协议版本核对 |
| POST | `api/probe/shutdown` | 无 | 无（204） | 优雅关闭联调；**仅 debug 构建注册**（release 返回 404） |

> `api/probe/shutdown` 只记录关闭请求（`lib_core::lifecycle::request_shutdown()`），真正的关闭动作由 `lib-bin-web` 注册的钩子完成：先进入 200ms 拒绝窗口（此期间新请求返回 `503 {"code":503,"message":"服务正在关闭，请稍后重试"}`），再停止监听并排空进行中的请求。它用于关闭流程联调，不是对外契约，P2 前可删除；`make gen` 在 debug 下生成，因此两份 SDK 会包含该 path。语义见 `.steps/p1/decisions.md` D-028。

## 参数与载荷

### ProbeEchoPO（请求体）

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `id` | `string` | 是 | `i64` 按约定以字符串传输 |
| `text` | `string` | 是 | 必填文本 |
| `tags` | `string[]` | 是 | 标签列表 |
| `flag` | `bool` | 否 | 省略时为 `false` |

### ProbeEchoVO（响应）

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `string` | 回显的业务 ID |
| `text` | `string` | 回显的文本 |
| `tagCount` | `string` | 标签数量（`i64` → 字符串） |
| `flag` | `bool` | 请求中的开关 |

### ProbeQueryPO（查询参数）

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `keyword` | `string` | 否 | 关键字 |
| `size` | `string` | 否 | 数量，缺省 `10`；依赖 `DisplayFromStr` 从查询串解析 |
| `desc` | `bool` | 否 | **不可用**：`bool` 从查询串解析必然失败（D-021），该字段只用于演示此限制 |

### ProbeQueryVO（响应）

| 字段 | 类型 | 说明 |
|---|---|---|
| `keyword` | `string` | 回显的关键字 |
| `size` | `string` | 生效的数量（`i64` → 字符串） |
| `desc` | `bool` | 生效的排序方向 |

### ProbeHealthVO / ProbeVersionVO（探活）

| 字段 | 类型 | 出现于 | 说明 |
|---|---|---|---|
| `version` | `string` | 两者 | workspace 版本 |
| `protocolVersion` | `string` | 两者 | API 协议版本，当前 `"1"`，不兼容变更时递增 |
| `startedAtMs` | `string` | `health` | 服务启动时间（Unix 毫秒） |
| `uptimeMs` | `string` | `health` | 已运行时长（毫秒） |

`startedAtMs` 由服务端启动时的 `mark_startup()` 记录；未显式记录时退化为首次访问时刻。

## 响应样例

```text
GET api/probe/void

HTTP/1.1 204 No Content
x-trace-id: 244399117927845888
（无 content-type、无 content-length、无响应体）
```

```text
POST api/probe/echo
{"id":"1","text":"hello","tags":["a","b"]}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"id":"1","text":"hello","tagCount":"2","flag":false}}
```

```text
GET api/probe/query?keyword=abc&size=5

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"keyword":"abc","size":"5","desc":false}}
```

```text
GET api/probe/typed

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"keyword":null,"size":"10","desc":false}}
```

```text
GET api/probe/health

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"version":"26.11.0","protocolVersion":"1","startedAtMs":"1767225600000","uptimeMs":"1234"}}
```

```text
GET api/probe/response

HTTP/1.1 200 OK
content-type: application/json; charset=utf-8
x-probe-kind: web-response
{"code":200,"message":"Success","data":"probe-blob"}
```

## 错误与限制样例

| 请求 | HTTP | `code` | 响应体 |
|---|---|---|---|
| `GET api/probe/fail?size=200` | 400 | 400 | `{"code":400,"message":"size 超出探针允许范围"}` |
| `POST api/probe/echo`，`id` 传 JSON 数字 / 缺 `text` / `tags` 类型不符 | 400 | 400 | `{"code":400,"message":"请求参数转换失败"}` |
| `POST api/probe/echo`，空请求体 | 400 | 400 | `{"code":400,"message":"请求体不是有效的 JSON"}` |
| `GET api/probe/query?size=abc`，或带 `?desc=true` | 400 | 400 | `{"code":400,"message":"查询参数转换失败"}` |
| `GET api/probe/internal` | 500 | 500 | `{"code":500,"message":"服务器内部错误"}` |
| `GET api/probe/panic` | 500 | 500 | `{"code":500,"message":"服务器内部错误"}` |
| `POST api/probe/void`、`GET api/probe/nothing` | 404 | 404 | `{"code":404,"message":"请求接口不存在"}` |

限制说明：

- 参数类错误的 `message` 是固定文案，不含字段名，客户端不要依赖其内容做分支。
- 未知字段被忽略，字段名拼错不会报错；请求体不校验 `content-type`。
- 查询参数名大小写敏感（`?Keyword=abc&Size=9` 会被整体忽略，走默认值）。
