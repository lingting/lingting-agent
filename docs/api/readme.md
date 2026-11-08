# 接口文档索引

服务端 HTTP 接口的**人类可读说明**：领域划分、接口清单、参数表与调用样例。

**契约的唯一来源仍然是代码**：`crates/lib-web/src/` 下 `#[web_api_*]` 标注的 handler 与 `#[auto_type]` / `#[auto_enum]` 标注的 DTO，客户端 Rust / TS SDK 由 `make gen` 生成。本目录只做说明与样例，不引入第二份契约定义；与代码不一致时以代码为准，并按下文「维护规则」修正文档。

## 领域文档

| 文档 | 领域 | 内容 |
|---|---|---|
| [probe.md](probe.md) | probe（探针与探活） | 技术验证探针、错误响应样例、`health` / `version` 探活，共 11 条 |
| [session.md](session.md) | session（会话） | Mock 会话 / 消息 / Run 业务接口与 SSE 事件流，共 7 条 |

文件命名：领域名小写、单数，与 path 的资源段一致（`api/session/...` → `session.md`；`api/probe/...` → `probe.md`）。新增领域时新建对应文档并登记到上表。

## 通用约定

### 地址与路径

| 项 | 约定 |
|---|---|
| 基础地址 | 未显式配置时的默认端口按构建形态切换（D-022）：release 构建 `http://127.0.0.1:20100`，debug 构建 `http://127.0.0.1:20102`（开发环境服务端）；探针服务 `20103`（见 `docs/develop.md` 端口表） |
| path | 统一 `api/` 开头，**不带版本段**；资源段小写单数，动作段只用 `create` / `update` / `delete` / `get` / `list` / `cancel` / `stream` |
| 路径参数 | 无路径占位符（D-008）：会话、Run 等 ID 只能走查询参数或请求体 |
| 方法错配 | 与路径不存在同为 404，**没有 405**，调用方无法从状态码区分「路径错」与「方法错」 |
| 大小写 | path 与查询参数名大小写敏感，查询参数名为 camelCase 字段名 |
| 查询参数 | 每个 `GET` 最多一个参数对象；查询 DTO 只放字符串字段，不出现 `bool`（D-021） |

### 响应形态

| 返回形态 | 响应体 | 示例接口 |
|---|---|---|
| `Void` | 204，无响应体 | `GET api/probe/void` |
| 载荷类型 / `Result<T>` | `{"code":200,"message":"Success","data":<载荷>}`，由 `R<T>` 包装 | 绝大多数接口 |
| `WebResponse` / `WebResponseOf<T>` | 自定义状态码与响应头；元数据中被标记为 `Blob` | `GET api/probe/response` |
| `WebResponse` + `WebBody::Stream` | `text/event-stream`，一帧一条事件（不经 `R<T>` 包装） | `GET api/run/stream` |

### 错误响应

所有失败响应由框架统一生成（`WebResponse::from_error`），结构固定为：

```json
{"code":400,"message":"请求参数转换失败"}
```

`code` 与 HTTP 状态码同值，不引入独立业务码；已分配取值只追加、不复用。完整分类与触发方式见 `.steps/p1/decisions.md` D-024，错误契约类型 `ApiError { code, message }` 定义在 `crates/lib-web/src/error.rs` 并随 SDK 生成导出。

| code | 含义 | 对外 `message` |
|---|---|---|
| 400 | 参数或状态不满足规则 | 调用方 message，或框架固定文案（`请求参数转换失败` / `查询参数转换失败` / `请求体不是有效的 JSON`） |
| 401 / 403 | 未认证 / 无权限 | 调用方 message（P1 未启用鉴权，暂无样例） |
| 404 | 对象不存在、路由不存在、方法不匹配 | 调用方 message，或 `请求接口不存在` |
| 500 | 服务端错误（含返回值转换失败与 panic） | 固定 `服务器内部错误`，堆栈、SQL、路径只进日志 |

### 字段类型

| 项 | 约定 |
|---|---|
| id | 一律 `String`（雪花 ID 转字符串，D-018） |
| `i64`（数量、token 用量、`*Ms` 时间戳） | wire 上是**字符串**，传 JSON 数字直接 400；TS 侧类型为 `string` |
| `Option<T>` | 可省略，缺省时按 `null` 或字段缺失处理 |
| 枚举 | wire 值为 `SCREAMING_SNAKE_CASE`，如 `QUEUED` / `CANCELLED` / `USER` |
| 请求体 | 未知字段被忽略，字段名拼错不会报错，只会走默认值或缺失错误；不校验 `content-type` |
| DTO 命名 | 请求体 `*PO`、查询参数 `*Params`、响应 `*VO` |

## 维护规则

1. 改接口先改 `crates/lib-web/src/` 的 handler 与 DTO。
2. 同步本目录中对应领域文档（接口清单、参数表、样例）。
3. `make gen` 重新生成两份 SDK，不手改生成物。

命名、错误响应与版本管理的规范原文见 `docs/develop.md` 的「API 契约规范」。
