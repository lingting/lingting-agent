# P1-004 服务端 API 定义（framework-proc-web）验证报告

本报告是 P1-004 的交付物。结论分为两类，逐条标注：

- **实测**：启动探针服务后发真实 HTTP 请求得到的响应。
- **源码核实**：只阅读 `lingting-rust-framework`（26.11.0）源码得出结论，未运行。

验证时间点：`.steps/p1/tasks.md` 中 P1-004 执行时。

---

## 1. 结论摘要

| 问题 | 结论 | 依据 |
|---|---|---|
| REST 风格路径（`api/sessions/{id}`） | **不可用**。`WebRouter` 按 `method + path` 精确字符串匹配，无占位符解析 | 源码核实 + 实测 |
| `GET` 接口参数个数 | 最多一个参数对象；两个及以上是编译错误 | 源码核实 |
| `WebResponse` 的 API 元数据 | 被标记为 `Blob`；需要类型化载荷时改用 `WebResponseOf<T>`（元数据为对应 `Type`） | 源码核实 |
| 参数默认值 `AuthRule::login()` | **对本地客户端无影响**。`framework-web-axum` 不按 `auth` 鉴权，只有自定义 `WebRouteWrapper` 才会校验 | 实测 + 源码核实 |
| 关闭 `collect` 后手动挂载 | **不可行**。`WebRouter` 无插入 API，`build_<fn>_route` 是 `pub(crate)`，且 `framework-web-axum` 在未开 `collect` 时无法编译 | 源码核实 |
| 链接锚点 | 消费方**必须**引用本 crate 的符号（`lib_web::web_routes()`），否则 `inventory` 注册项被链接器丢弃，表现为“接口定义了但全部 404” | 实测（未引用时收集到 0 条路由） |
| 参数提取失败 | HTTP 400 + `{"code":400,"message":"<对外消息>"}`，serde 细节只进日志，不回传调用方 | 实测 |

---

## 2. 探针实现（已保留）

| 位置 | 作用 |
|---|---|
| `crates/lib-web/src/probe/dto.rs` | 探针 DTO，全部用 `auto_type` 标注（camelCase + `i64` 按字符串 + `Option` 可省略），可被 SDK 生成器导出 |
| `crates/lib-web/src/probe/api.rs` | 探针接口（P1-004 时 7 条，P1-009 起 11 条） |
| `crates/lib-web/src/router.rs` | `web_routes()`：路由收集 + 链接锚点 |
| `crates/lib-web/examples/api_probe_server.rs` | 探针服务，P1-005 / P1-006 生成 SDK 后从这里联调 |

启动方式：

```text
cargo run -p lib-web --example api_probe_server              # 127.0.0.1:20103
AGENT_PROBE_PORT=9000 cargo run -p lib-web --example api_probe_server
```

启动日志会打印收集到的路由数：`probe server listening on http://127.0.0.1:20103 (11 routes)`。

### 探针接口清单

| 方法与路径 | 参数 | 返回类型 | 覆盖点 |
|---|---|---|---|
| `GET api/probe/void` | 无 | 省略（`Void`） | 204 空响应 |
| `POST api/probe/echo` | `Json<ProbeEchoPO>` | `Result<ProbeEchoVO>` | 请求体提取、错误响应 |
| `GET api/probe/query` | `Query<ProbeQueryPO>` | `Result<ProbeQueryVO>` | 查询参数提取 |
| `GET api/probe/typed` | `Query<ProbeQueryPO>` | `ProbeQueryVO`（`Type`） | `R<T>` 包装 + 显式 `auth = AuthRule::anonymous()` |
| `GET api/probe/fail` | `Query<ProbeQueryPO>` | `Result<ProbeQueryVO>` | 业务错误的状态码与响应体 |
| `GET api/probe/response` | 无 | `WebResponse` | 自定义响应头 + 元数据 `Blob` |
| `GET api/probe/response_of` | 无 | `WebResponseOf<ProbeQueryVO>` | 自定义响应头 + 类型化载荷 |

> P1-009 追加 `GET api/probe/internal`、`GET api/probe/panic`（错误分类样例）与 `GET api/probe/health`、`GET api/probe/version`（探活），现共 11 条；接口清单、错误码与样例见 `decisions.md` D-024。

---

## 3. 实测响应样例（可复制）

### 3.1 成功响应

```text
GET /api/probe/void
HTTP/1.1 204 No Content
x-trace-id: 244399117927845888
（无 content-type、无 content-length、无响应体）
```

```text
POST /api/probe/echo
content-type: application/json
{"id":"1","text":"hello","tags":["a","b"]}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"id":"1","text":"hello","tagCount":"2","flag":false}}
```

```text
GET /api/probe/query?keyword=abc&size=5

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"keyword":"abc","size":"5","desc":false}}
```

```text
GET /api/probe/typed

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"keyword":null,"size":"10","desc":false}}
```

### 3.2 错误响应

```text
GET /api/probe/fail?size=200
HTTP/1.1 400 Bad Request
{"code":400,"message":"size 超出探针允许范围"}
```

```text
POST /api/probe/echo   {"id":1,"text":"hello","tags":[]}     # i64 传了数字
POST /api/probe/echo   {"id":"1","tags":[]}                  # 缺必填 text
POST /api/probe/echo   {"id":"2","text":"t","tags":"not-array"}   # Vec 类型不符
HTTP/1.1 400 Bad Request
{"code":400,"message":"请求参数转换失败"}          # 三者响应完全相同
```

```text
POST /api/probe/echo   空请求体
HTTP/1.1 400 Bad Request
{"code":400,"message":"请求体不是有效的 JSON"}
```

```text
GET /api/probe/query?size=abc
GET /api/probe/query?keyword=abc&size=5&desc=true     # bool 查询参数
HTTP/1.1 400 Bad Request
{"code":400,"message":"查询参数转换失败"}
```

```text
POST /api/probe/void          # 方法与路由不匹配
GET  /api/probe/nothing       # 路径不存在
HTTP/1.1 404 Not Found
{"code":404,"message":"请求接口不存在"}              # 两者响应相同
```

### 3.3 自定义响应

```text
GET /api/probe/response
HTTP/1.1 200 OK
content-type: application/json; charset=utf-8
x-probe-kind: web-response
{"code":200,"message":"Success","data":"probe-blob"}
```

```text
GET /api/probe/response_of
HTTP/1.1 200 OK
x-probe-kind: web-response-of
{"code":200,"message":"Success","data":{"keyword":null,"size":"10","desc":false}}
```

---

## 4. 参数与路径的行为细节

### 4.1 路径

- `path` 两端 `/` 被裁掉，请求路径同样 `trim_matches('/')` 后比较，因此 `/api/probe/void/` 与 `/api/probe/void` 等价（实测均为 204）。
- 无占位符解析，`api/session/{id}` 会被当作字面量路径。资源 id 只能走 body 或 query（D-008 方案 1）。
- 路由不存在与方法不匹配都返回 404，**没有 405**，调用方无法区分“路径错”与“方法错”。

### 4.2 请求体（`Json<T>`）

- `i64` 字段必须传字符串（`auto_type` 注入 `DisplayFromStr`），传 JSON 数字直接 400。这是 D-018 在运行时的实际约束。
- 未知字段被忽略（未启用 `deny_unknown_fields`），拼错字段名不会报错，只会走默认值或缺失错误。
- 不校验 `content-type`：不带头也能正常解析 JSON 请求体。
- 空 body 与非法 JSON 的错误消息不同（`请求体不是有效的 JSON` vs `请求参数转换失败`）。

### 4.3 查询参数（`Query<T>`）

`query_json()` 把查询串的每个值都变成 JSON 字符串，因此：

| 字段类型 | 查询参数可用性 |
|---|---|
| `String` / `Option<String>` | 可用 |
| `i64`（经 `auto_type`，带 `DisplayFromStr`） | 可用（`?size=5` 解析成功） |
| `bool` / `Option<bool>` | **不可用**，400 `查询参数转换失败` |
| `Vec<T>` | 多值同名参数会变成数组，需自定义适配，未验证 |

- 查询参数名大小写敏感，且必须与 serde 字段名一致（`auto_type` 下为 camelCase）：`?Keyword=abc&Size=9` 被整体忽略，返回默认值。
- 结论：**GET 的查询 DTO 只放字符串类字段**；布尔与数值入参优先改成字符串枚举/字符串字段，或在 DTO 上自定义 `deserialize_with`。

---

## 5. 对后续任务的硬约束

1. **必须开启 `collect`**：`lib-web` 对 `framework-web` / `framework-web-axum` / `framework-proc-ts` / `framework-proc-core` 全部开 `collect`，这是唯一可行路径（见摘要第 5 行）。
2. **必须保留链接锚点**：任何消费 `lib-web` 路由或元数据的二进制（服务启动、TS/Rust SDK 生成器）都要先引用 `lib_web::web_routes()`，否则得到空集合。P1-005 / P1-006 的生成器与 P1-008 的服务启动都适用。
3. **`auth` 与鉴权**：P1 不实现鉴权，`auth` 值对本地客户端无影响；是否显式写 `AuthRule::anonymous()` 由 D-014 决定，不影响功能。
4. **ID 与数值字段**：DTO 的 `i64` 必须沿用 `auto_type`（`i64` ↔ 字符串），服务端与客户端都不得传数字，否则 400。
5. **错误契约**：错误响应固定为 `{code, message}`，`code` 与 HTTP 状态码一致；参数类错误的 `message` 是固定文案，不含字段名，客户端不要依赖其内容做分支。

---

## 6. 未覆盖 / 留待后续

- 关闭 `collect` 的构建未能实测（改为源码核实）：框架在该模式下与 `framework-web-axum` 不兼容，实际无法组成可运行的服务。
- `Vec<T>` 查询参数、`PaginationParams` 提取、SSE（`WebBody::Stream`）未验证，分别归 P1-011 / P1-009 阶段。
- 尚未验证 `WebResponseOf<T>` 在 TS 元数据里导出的具体类型名，属 P1-005。
