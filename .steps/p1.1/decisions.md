# P1.1 决策记录

**状态：已完成（RT-001 ~ RT-022 全部完成，21 条判据全部通过）。** 本文件记录 P1.1 的阶段级决策。编号 `D-P1.1-xxx`；被否决的方案保留记录，不删除。

任务分解见 [tasks.md](tasks.md)，验收见 [acceptance.md](acceptance.md)，协议与流程见 [README.md](README.md)。

---

## D-P1.1-001 双向传输形态

- **问题**：设计与客户端保持双向通信（注册与心跳），用什么传输？
- **背景**：`framework-web` 只有 `WebBody::Stream`（SSE），没有 upgrade / WebSocket 抽象；`framework-web-axum` 也未启用 axum 的 `ws` 特性。走 WebSocket 需先改造框架。
- **可选方案**：
  1. SSE 下行 + HTTP POST 上行。
  2. 改造 `framework-web` / `framework-web-axum` 支持 WebSocket。
  3. 两者并存。
- **决策**：方案 1。`GET api/client/stream` 承载全部服务端推送；注册、心跳、问询应答、主动断开全部走 POST。
- **决策理由**：零框架改动；WebUI（浏览器）、TUI、CLI 共用同一套 HTTP 形态，与 P1 已验收的 SSE 通道保持一致；注册/心跳/断连语义都定义在 `service-core` 的值类型与通道上，将来插 WebSocket 只换 `lib-web` 适配层。
- **配套约束**：`service-core` 不得感知传输（只产出事件值类型 + `mpsc`）；下行流断开只产生 `StreamClosed` 标记，清理统一交给处理链。
- **状态**：`DECIDED`（RT-001 已定稿，细节见下）。

### RT-001 定稿细节（上行协议与断开时序）

#### 1. 端点形态（入参位置 + 返回形态）

| 语义 | 方法 + path | 入参位置 | 返回形态 | SDK 侧形态 |
|---|---|---|---|---|
| 注册 | `POST api/client/create` | JSON 请求体 `ClientCreatePO`（当前无字段，`{}`） | `Result<ClientCreateVO>` → `R<ClientCreateVO { clientId, pongAtMs }>` | 有类型方法 |
| 心跳 / 问询应答 | `POST api/client/update` | JSON 请求体 `ClientUpdatePO { clientId, kind }`，`kind ∈ {PING, INQUIRY_ACK}` | `Result<ClientUpdateVO>` → `R<ClientUpdateVO { pongAtMs }>` | 有类型方法 |
| 主动断开 | `POST api/client/delete` | JSON 请求体 `ClientDeletePO { clientId }` | `()` → 204，无响应体 | 有类型方法 |
| 下行流 | `GET api/client/stream` | 查询参数 `ClientStreamParams { clientId }` | `Result<WebResponse>`（`WebBody::Stream`），**不**经 `R<T>` 包装 | **`Blob`**：只导出 path 常量 |
| 建立占用 | `POST api/occupancy/create` | JSON 请求体 `OccupancyCreatePO { clientId, sessionId }` | `Result<OccupancyVO>` → `R<OccupancyVO>` | 有类型方法 |
| 释放占用 | `POST api/occupancy/delete` | JSON 请求体 `OccupancyDeletePO { clientId }` | `()` → 204，无响应体 | 有类型方法 |
| 查询占用 | `GET api/occupancy/get` | 查询参数 `OccupancyGetParams { sessionId }` | `Result<OccupancyVO>` → `R<OccupancyVO>`；无人占用 404 | 有类型方法 |

- **没有路径占位符**（D-008）：所有 ID 只出现在查询参数或请求体；`clientId` / `sessionId` 的格式校验在 service 层，不合法按未注册 / 未找到处理。
- 查询参数对象只放字符串字段（D-021）；`*Ms` 时间戳（`i64` → wire 字符串）只出现在响应体，不出现在查询参数。
- 下行流必须是 `GET`（浏览器 `EventSource` 限制，A-6），上行一律 `POST` + JSON 请求体（动作段沿用 `create` / `update` / `delete` / `get` 白名单）。
- 下行流的响应头固定沿用 `lib_web_core::sse` 常量：`content-type: text/event-stream; charset=utf-8`、`cache-control: no-cache`、`x-accel-buffering: no`。
- 两个 `delete` 走 204 空响应：幂等删除没有可表达的结果，重复删除与首次删除返回一致，调用方不依赖返回值。
- 下行流建立以前的失败（未注册的 `clientId`）走普通错误响应：400 + `客户端未注册，请重新连接`（D-P1.1-005）；流一旦建立就不能再改状态码，之后的失败只能用 `client_closed` 帧或直接断流表达。

#### 2. 下行帧与事件

- 一帧 = `id:` + `event:` + `data:`（单行 JSON）+ 空行，复用 `sse::json_frame`（与 P1 的 `api/run/stream` 帧编码同一实现）。
- `event:` 取值 = 下行事件枚举的 wire 值，**snake_case**：`inquiry`、`session_event`、`client_closed`。与 `RunEventKind` 同一约定（`serde(rename_all)` 与 `strum(serialize_all)` 必须同时设成 snake_case，否则 `event:` 行与 JSON 的 `event` 字段会不一致）。
- `data:` 载荷：`session_event` → 整条 `RunEvent`（**复用 `types-core::session::RunEvent`**，不另造事件模型，字段与事件文件一致）；`inquiry` → 问询请求时刻与应答截止时刻；`client_closed` → 关闭原因（取值同断连判定的六种来源）。
- `id:` = `ClientEvent.seq`，由 `RuntimeState` 按客户端从 0 单调递增分配。`session_event` 内部还有 `RunEvent.seq`（事件文件内的序号），**两者不同源**，不得混用。
- **不支持续传**：服务端忽略 `Last-Event-ID`，不重放事件；`id` 只用于客户端丢帧检测与排障。
- **`pong` 不进下行流**：pong 只出现在 `client/create` 与 `client/update` 的同步响应体里，一次心跳一帧，不存在“同一问询两处应答”。
- 保活：下行流空闲时由 `lib-web` 的帧合成层每 15s 注入一条 SSE 注释帧 `: keepalive`。它不进入 `ClientEvent`、不占 `seq`、不落事件文件、不计入 `lastSeen`，只是写探测与空闲防掐断。

#### 3. 断开感知与「先标记、后清理」时序

- **检测只依赖通道生命周期**：`lib-web` 用 `unfold` 持有 `mpsc::Receiver<ClientEvent>`，响应体被 drop（对端断开、写失败、代理掐断）即 receiver drop。
- `service-core::runtime` 在建立通道时额外保留一份 `Sender` 的 watcher：`sender.closed().await` 完成即标记 `StreamClosed` 并发出总线通知。`lib-web` **不写 Drop 钩子、也不知道“标记”**，反过来 `service-core` 不感知 `unfold`、帧与响应体。
- 转发用 `try_send`：`Closed`（对端已断）或 `Full`（消费过慢，视为流已废）都只造成 `StreamClosed` 标记；`try_send` 不阻塞事件写入，转发失败不回滚已落盘的事件。
- 固定时序（顺序不可交换）：
  1. 传输失联：receiver drop / `try_send` 失败 / 协商期以外的心跳超时与问询超时 / `client/delete` / 服务关闭。
  2. `RuntimeState::mark(clientId, reason)`：把原因写进客户端记录，并把 `clientId` 放进标记集合（同一客户端重复标记只保留第一次的原因）。
  3. `bus.notify::<ClientDisconnected>()`：纯通知、不带数据；总线会合并触发，处理链靠 take 语义保证只处理一次。
  4. 处理链 `take` 标记集合：为空直接结束；非空则依次 反注册 → 释放占用 → 停止运行时会话（追加终态事件 + flush）→ 关闭下行通道（先发一帧 `client_closed`，再 drop sender）→ 记日志。
  5. 反注册之后该 `clientId` 的任何上行请求一律 400 `客户端未注册，请重新连接`（客户端据此重新注册）。
- **窗口期行为**：标记已置位但处理链尚未消化的这段时间里，该 `clientId` 的 ping **仍然成功**（记录还在注册表里，`lastSeen` 照常更新），但**不解除标记、不清原因**；只有处理链能清除标记。此后不再向该客户端转发事件，事件照常落盘。
- 流已断时 `client_closed` 帧发不出去（`try_send` 返回 `Closed`），这是预期路径，不算错误；只有服务端主动关闭（主动断开 / 关停 / 超时）才可能真正送达。

#### 4. `service-core` 的传输边界

- 只认**值类型 + 通道**：`ClientEvent`（`seq` + 载荷，`Clone` 且可序列化）、`mpsc::Sender<ClientEvent>`、`mpsc::Receiver<ClientEvent>`。`service-core` 里不出现 `Bytes`、SSE 帧字符串、`content-type`、响应头、`WebError` / `WebResponse`、HTTP 状态码、路由或 path 常量。
- serde 属于契约而非传输（`types-core` 已有先例）：`service-core` 可以用 serde 标注的事件类型，但 `event:` 行名、`id:`、`: keepalive`、响应头一律由 `lib-web` 从值类型投影。
- 错误映射集中在 `lib-web/src/runtime/` 的一个函数里：`RuntimeError::{ClientNotRegistered, SessionOccupied, ClientBusy, NotFound, Internal}` → `WebError`，其中三条固定 message 见 D-P1.1-005；`service-core` 只返回语义化错误，不携带 HTTP 细节。
- 将来若改 WebSocket，只替换 `crates/lib-web/src/runtime/{api,stream}.rs` 与帧合成层；`RuntimeState`、标记集合、处理链、事件文件不动。

---

## D-P1.1-002 会话占用的建立方式

- **问题**：`sessionId ↔ clientId` 的占用由谁建立？
- **可选方案**：
  1. 由会话事件流订阅隐式建立。
  2. 显式占用接口（`api/occupancy/create|delete|get`）。
  3. 两者都提供。
- **决策**：方案 2。显式接口建立与释放占用；占用成功即启动该会话的运行时会话并打开事件文件。
- **决策理由**：设计原文把「会话占用」当成独立动作（“会话占用时写独占原始事件文件”“取消这些客户端的会话占用”），隐式建立无法表达“客户端主动让出会话”与“查询当前占用者”；显式接口也让冲突错误可以精确落到具体请求上。
- **配套约束**：
  1. **一对一**：一个客户端同时只能占用一个会话；再占第二个返回 400 `客户端已占用其他会话`。
  2. 同一会话第二方占用返回 400 `会话已被其他客户端占用`。
  3. 占用释放会停止运行时会话，该会话 `QUEUED` / `RUNNING` 的 Run 一律落 `CANCELLED`。
  4. 不新增路径动词，沿用动作白名单（`create` / `delete` / `get`）。
- **状态**：`DECIDED`（RT-002 已定稿，细节见下）。

### RT-002 定稿细节（占用的语义边界）

#### 1. 建立占用的顺序与补偿

`POST api/occupancy/create { clientId, sessionId }` 的处理顺序固定：

1. 校验 `clientId` 已注册：不在注册表（含已反注册）→ 400 `客户端未注册，请重新连接`。
2. 校验会话存在：不存在 → 404 `会话不存在：<sessionId>`（沿用 session 领域现有文案风格）。
3. 取 `sessionId` 粒度锁（`README.md` 第五节规则 4），锁内完成第 4~6 步，不允许持锁跨 `await`；需要 `await` 的部分（打开文件、起任务）在第 5 步一次性完成并立即释放锁。
4. 占用表判定：
   - 同一对 (clientId, sessionId) 已存在 → **幂等成功**：直接返回当前 `OccupancyVO`，不重建运行时会话、不重开事件文件、不重复落盘。
   - `sessionId` 已被其他 `clientId` 占用 → 400 `会话已被其他客户端占用`。
   - 该 `clientId` 已占用其他会话 → 400 `客户端已占用其他会话`。
5. **先登记占用，再建运行时会话**：写占用表（双向索引 `clientId → Occupancy`、`sessionId → Occupancy`），成功后打开事件文件（`create_new`）并启动该会话的运行时会话（写者 + 转发）。
6. 第 5 步后半失败（建目录 / 开文件 / 起任务失败）：撤回占用登记与半建文件（尽力删除，失败只记日志），返回 500 固定 `服务器内部错误`。不留“登记存在但运行时会话不在”的状态。

顺序理由：登记是纯内存动作、可回滚；文件与任务不可回滚。先登记可保证任何时刻“有占用记录 ⇔ 有运行时会话”（失败补偿除外，且补偿立即执行）。

`OccupancyVO`：`{ sessionId, clientId, runtimeSessionId, acquiredAtMs }`；`*Ms` 为 `i64`，wire 上是字符串。

#### 2. 冲突表达与校验顺序

| 场景 | code | message |
|---|---|---|
| 客户端未注册 / 已反注册 | 400 | `客户端未注册，请重新连接` |
| 会话已被其他客户端占用 | 400 | `会话已被其他客户端占用` |
| 客户端已占用其他会话 | 400 | `客户端已占用其他会话` |
| 会话不存在 | 404 | `会话不存在：<sessionId>` |
| 查询占用但无人占用 | 404 | `会话未被占用` |
| 内部失败（建文件 / 起任务） | 500 | 固定 `服务器内部错误` |

- 校验顺序固定为「注册 → 会话存在 → 客户端一对一 → 会话独占」，因此同一请求只可能命中一条 message，客户端无需按优先级猜测。
- 不新增状态码：没有 409 / 410（D-P1.1-005），`code` 与 HTTP 状态码同值，客户端按 message 识别；与 `docs/api/readme.md` 的 400 / 404 / 500 分类一致。
- 三条冲突 message 的映射集中在 `lib-web/src/runtime/` 的同一个函数里（D-P1.1-005 的收敛点），`service-core` 只返回 `RuntimeError` 变体。

#### 3. 「客户端已占用其他会话」不做隐式切换

- 客户端占用 A 后要占用 B，**必须**先 `POST api/occupancy/delete { clientId }` 释放 A，再 `create` B；服务端不提供隐式切换、抢占或“自动让出”。
- 理由：隐式切换要在一次请求里既停止旧运行时会话（落终态、flush、关文件）又建新会话，中途失败无法回滚到原状；拆成两个幂等请求后，任一步失败都能原样重试。
- 幂等口径：同一对重复 `create` 返回当前占用；`delete` 在该客户端没有占用时同样返回 204。

#### 4. 释放占用与 Run 的联动

- 释放的三条触发来源共用一段逻辑：`POST api/occupancy/delete`、断连处理链（`README.md` 7.2 步骤 D~E）、服务关闭流程。
- 释放时该会话下 **`QUEUED` 与 `RUNNING` 的 Run 一律落 `CANCELLED`**，并写 `finished_at_ms`：会话一旦不被占用就没有执行者，留着 `QUEUED` 也没有机会被推进。
- 与当前代码的差异点（RT-004 之后的规则）：**无占用的会话里新建的 Run 仍停在 `QUEUED`**（等待下一次占用），这是“等待”，不是“残留”；只有一次显式释放才会取消当时已存在的 `QUEUED` / `RUNNING`。
- 释放顺序（不可交换）：取消 Run → 向事件文件追加终态事件并 flush → 关闭文件 → 撤回占用表的双向登记。运行时会话与占用记录同生命周期（一一对应），释放即销毁。
- 释放**不关下行通道**：客户端仍在连接，只是不再收到该会话的 `session_event`；不发 `client_closed`。`client_closed` 只在反注册时发（D-P1.1-001）。
- 取消后若客户端再次占用同一会话，新建的 Run 是新的 `QUEUED`，不复用被取消的记录。

#### 5. 交出去的两处（不属本任务）

- **取消类事件类型**：现有 `RunEventKind` 只有 `run_started` / `message_delta` / `run_completed` / `run_failed` / `server_shutdown` / `heartbeat`，**没有取消事件**。释放导致的 `CANCELLED` 在事件文件里如何表达（新增 `run_cancelled` 还是复用 `run_failed` + error 文本）已定稿：新增 `RunEventKind::RunCancelled`（RT-003），类型落地归 RT-005。
- **Run 触发点的具体时机**（占用建立时是否立即执行存量 `QUEUED`）归 RT-004 定稿；本项只约束“释放时一律取消”。

---

## D-P1.1-003 原始事件文件与轮转

- **问题**：会话运行时的原始事件如何落盘？文件如何清理？
- **可选方案**：
  1. 不落盘，只做内存转发。
  2. 落盘但不轮转。
  3. 落盘 + 按会话数与总字节数轮转清理。
- **决策**：方案 3。事件文件是唯一事实源：一行一条 `RunEvent` 的 JSONL，先落盘 flush 再转发；轮转按 `max_sessions`（会话目录数）与 `max_bytes`（总字节数）配置，**满足任一即清理**，按会话目录最旧先删，正在占用/写入的目录永不删除。
- **决策理由**：设计明确要求“会话占用时写独占原始事件文件、每个事件按顺序写入”；不落盘则失去排障与后续 Checkpoint/Resume 的基础。轮转是必需的，否则单进程长期运行会把用户目录吃满。
- **配套约束**：
  1. 独占靠 `create_new(true)` + 进程内 `sessionId` 级锁，不依赖平台文件锁。
  2. 复用 `types-core::session::RunEvent`，不另造事件模型。
  3. `0` 表示对应限制关闭，默认值 200 会话 / 512 MiB（RT-003 定稿）。
  4. 删除失败（Windows 上文件被占用）跳过并继续，只记日志。
- **状态**：`DECIDED`（RT-003 已定稿，细节见下）。

### RT-003 定稿细节（事件文件、终态与轮转）

#### 1. 路径、命名与打开方式

| 项 | 定稿 |
|---|---|
| 路径 | `<应用根目录>/data/events/<sessionId>/<runtimeSessionId>.jsonl`；应用根目录由 `lib_core::directorys` 推导，与 P2 的 `agent.db` 同级 |
| `runtimeSessionId` | `lib_core::id` 生成的雪花 ID（字符串），一次占用一个文件 |
| 目录 | `create_dir_all`；文件用 `OpenOptions::new().create(true).append(true)`（**追加，不截断**）；进程内再加 `sessionId` 级锁，不使用平台文件锁 |
| 写者 | `BufWriter<File>` 包在运行时会话对象里，随运行时会话创建/销毁 |
| 文件名排序 | 不参与排序：轮转按**会话目录**的最后更新时间判定，不看文件名 |

- 说明：`create_new(true)` 用于“同一 `runtimeSessionId` 不会被打开两次”的断言，但由于 `runtimeSessionId` 每次占用都新生成，实践中冲突只可能来自 id 生成失败，此处以 `append` 打开、由 `sessionId` 级锁保证独占语义（对比 D-P1.1-003 配套约束 1，落地以本节为准）。

#### 2. 行格式与 seq

- 一行一条 `RunEvent` 的 JSON（`serde_json::to_vec` + `\n`），UTF-8；不做 pretty、不写注释行。
- `seq` 从 0 开始、文件内单调递增，由写者持有计数器；**不信任事件自带的值**，写入前由写者覆写。
- 复用 `types-core::session::RunEvent`，不另造事件模型；字段不需要扩展。

#### 3. 终态事件（含新增取消事件）

- **新增 `RunEventKind::RunCancelled`**，wire 值 `run_cancelled`（`serde(rename_all)` 与 `strum(serialize_all)` 同时为 snake_case，否则 SSE 的 `event:` 行与 JSON 的 `event` 字段不一致）。载荷用现有字段：`status = CANCELLED`、`error` = 取消原因文本。
- 取消原因只取三个固定文本（对外可见、不含内部细节）：`会话占用已释放`、`客户端连接已断开`、`服务正在关闭`。
- 终态事件清单：`run_completed`（正常结束）、`run_failed`（执行失败）、`run_cancelled`（取消）、`server_shutdown`（服务关闭导致的终止）。
- 追加时机：Run 抵终态时写一条；释放占用时**先为每个被取消的 Run 各写一条 `run_cancelled`**，再写 `server_shutdown`（仅服务关闭时），最后 flush + 关闭文件。
- 终态之后不再向该文件追加；文件保留，不删除（不恢复，见 D-P1.1-007）。

#### 4. 写序与 flush 粒度

- **先落盘 flush，再转发**：文件是事实源，转发失败（`try_send` 返回 `Closed` / `Full`）不影响已写入的内容，也不回滚。
- 每条事件一次 `write_all` + `flush`；**不做 `sync_all`（fsync）**。理由：每条 fsync 会把事件吞吐按磁盘 IOPS 限死；进程 panic 最多丢最后若干条未 fsync 的行，正常关闭由关闭流程的 flush 兜底。
- 写失败（磁盘满、目录被删、权限变化）：停止该运行时会话 → 把占用它的客户端标记为 `JournalError` → 走断连处理链补偿，只进日志，不向客户端暴露内部细节。

#### 5. 轮转配置、默认值与数据量估算

```toml
[runtime.events]
max_sessions = 200
max_bytes = 536870912
```

| 项 | 定稿 |
|---|---|
| `max_sessions` | 保留的会话目录数量上限，默认 `200`；`0` = 不限 |
| `max_bytes` | `data/events` 下全部文件字节数上限，默认 `536870912`（512 MiB）；`0` = 不限 |
| 字段类型 | 两者都是 `u64`（配置项，不参与 API 契约，无 wire 形态） |
| 触发 | 启动一次 + 每个运行时会话建立后 + 60s 定时任务 |
| 判定 | `max_sessions` 与 `max_bytes` **满足任一即清理** |
| 顺序 | 按会话目录最后更新时间**最旧先删**，删到两项都在限内为止 |
| 保护 | 占用表快照里的 `sessionId` 目录永不删除，但计入统计（只计不删） |
| 失败 | 删除失败（Windows 上文件被占用）跳过并继续下一个，只记日志 |

**数据量估算**（用于说明默认值合理性）：单条 `RunEvent` JSON 约 250–350 B（含 `null` 字段）；一次对话约 50–80 条事件（`run_started` + 若干 `message_delta` + 终态 + 心跳）约 15–30 KB；200 个会话常态约 4–6 MB。也就是说 `max_sessions` 会先触发，`max_bytes` 是异常增长（长回复、大量心跳）的兜底，512 MiB 对应约 20 万次对话量级。

#### 6. 保护实现方式（不留竞态）

1. 轮转在**单个任务内串行**执行，同一时刻只有一份统计。
2. 先取占用表快照（`HashSet<String>`），再遍历 `data/events` 的各会话目录：读 `mtime`、累加字节数。
3. 以“最后一次写入时间升序”排序，逐个判断：受保护的跳过（但计入统计），其余进入删除候选。
4. 删除时**不二次探测文件状态**：判据只有占用表快照，避免“探测通过 → 文件被打开 → 删除失败/误判”的竞态；删除失败按上表跳过。
5. 配置 `0` 表示该维度关闭：两个都为 `0` 时整个轮转任务不运行（启动时判定一次）。

#### 7. 连接配置与透传路径

- `crates/lib-bin-cli/src/config.rs` 的 `ServerConfig` 新增 `runtime_events: RuntimeEventsConfig { max_sessions: u64, max_bytes: u64 }`，缺省值即上表。
- 透传：`config.toml` → `lib-bin-cli` → `lib_bin_web::serve` 入参 → `service_core::runtime::journal::JournalConfig` → 轮转任务持有。
- 不做热更新：配置在启动时读一次，运行期变更需重启。

---

## D-P1.1-004 与 `api/run/stream` 的关系

- **问题**：现有 `GET api/run/stream`（per-run 单消费者 SSE，订阅即推进 `RUNNING`）与新运行时如何共存？
- **可选方案**：
  1. 保留两套并存。
  2. 移除 `api/run/stream`，事件推送统一走 `api/client/stream`；客户端一次 Run 由内部编排多个接口完成。
  3. 一次性替换但保留兼容层。
- **决策**：方案 2。移除 `api/run/stream` 与 `RunStreamParams`；Run 触发点改为“会话被占用 + 消息创建”；客户端自行串联 `client/create` → `client/stream` → `occupancy/create` → `message/create` → `run/get`。
- **决策理由**：设计是客户端中心（连接 → 占用 → 事件），per-run 事件流与“会话占用”语义冲突（同一 Run 可能被多处订阅）；保留两套会让事件落盘与转发出现两条路径。
- **必须显式接受的后果**：
  1. 无客户端占用的会话里，新建的 Run 永远停在 `QUEUED`（不做后台自执行）。
  2. 属破坏性契约变更：`RunStreamParams` 删除、SDK 生成物里 `runStream` 消失、`lib-web/src/session/stream.rs` 删除、TUI/WebUI 必须同批改造。
  3. P1 / P2 的既有决策需要回填，清单见下节。
- **状态**：`DECIDED`（RT-004 已定稿，细节见下）。

### RT-004 定稿细节（Run 触发点与 `run/stream` 移除）

#### 1. 触发点规则（单一判据）

**一条规则**：Run 只有在**所属会话当前被占用**时才会被执行；执行者始终是该会话的运行时会话。

| 时机 | 行为 |
|---|---|
| `occupancy/create` 成功 | 把该会话现有 `QUEUED` Run 按 `created_at_ms` 升序**串行**执行：一次只推进一个到 `RUNNING`，抵终态后再取下一个 |
| `message/create` 成功且会话已占用 | 新 Run 追加到该会话执行队列尾部（队列空则立即开始） |
| `message/create` 成功但会话未占用 | Run 停在 `QUEUED`，不排队、不执行 |
| 释放占用（含断连链） | 队列清空，执行任务退出（`QUEUED` / `RUNNING` 一律 `CANCELLED`，见 D-P1.1-002） |

- `QUEUED → RUNNING` 的推进仍走 `store.start_run` 的 CAS 语义（只有 `QUEUED` 能推进），保证同一 Run 只有一个生产方。
- **串行**的含义：同一会话同一时刻最多一个 `RUNNING` Run，因此事件文件与下行流里不会交错两个 Run 的 `message_delta`；队列实现用一条 `mpsc` 串行任务，不并发推进多个 Run。
- 不做后台自执行、不做“无占用时先跑起来再落库”的妥协。

#### 2. 客户端编排顺序与失败回滚

1. `POST api/client/create` → 拿 `clientId`（响应里的 pong 即注册成功）。
2. `GET api/client/stream?clientId=...` 建立下行流。
3. `POST api/occupancy/create` → 成功即建运行时会话、打开事件文件、开始执行存量 `QUEUED`。
4. `POST api/message/create` → 写入用户消息与 `QUEUED` Run，排队执行。
5. 消费 `session_event` 渲染；以 `GET api/run/get` 对账终态。

| 失败点 | 处理 |
|---|---|
| `occupancy/create` 失败 | 不建文件、不排队；按 message 决定重试，或先 `occupancy/delete` 释放再重试 |
| 占用已建立但 `message/create` 失败（内容为空、会话已删） | **占用保留**，客户端直接重试 `message/create`；不自动释放 |
| 下行流断开 | 客户端不上行报错，重新走第 1 步（新 `clientId`）；旧连接由服务端断连链回收 |
| 客户端主动结束 | `POST api/occupancy/delete` → `POST api/client/delete` |

#### 3. 迁移清单

**服务端**

- 删除 `crates/lib-web/src/session/api.rs` 的 `run_stream` handler 与 `WebResponse` / `sse` 相关 import。
- 删除 `crates/lib-web/src/session/stream.rs`。
- `crates/service-core/src/session/service.rs`：删除 `subscribe_run`；生产方迁到 `runtime/session.rs`，`crates/service-core/src/session/stream.rs` 的 Mock 生产者改为运行时会话调用。
- `crates/lib-web/src/router.rs` 不需要改（`inventory` 自动收集）。

**客户端**

- `crates/lib-bin-cli/src/client/stream.rs`：`subscribe_run` 删除，改为 `client/create` + `client/stream` 消费 + 心跳保活。
- `crates/lib-bin-tui`：连接管理拆成「注册/心跳」与「下行事件消费」两件事，重连判据改为收到 400 `客户端未注册，请重新连接`。
- `lingting-agent-ui`（WebUI）：`runStream` 调用改为 `clientStream` + 按 `event` 分流。

**类型**

- 删除 `RunStreamParams`；保留 `RunEvent` / `RunEventKind`，新增 `RunCancelled`（RT-003）。
- `make gen` 重新生成两份 SDK：`runStream` 消失，出现 `clientCreate` / `clientUpdate` / `clientDelete` / `occupancyCreate` / `occupancyDelete` / `occupancyGet` 与 `clientStream` 的 path 常量。

**文档与验收记录**

- `docs/api/session.md` 删 `run/stream` 条目与样例；新增 `docs/api/runtime.md`、`docs/api/occupancy.md`；`docs/api/readme.md` 的响应形态示例行改为 `GET api/client/stream`。
- `.steps/p1/decisions.md` D-007 / D-011 / D-026、`.steps/p2/decisions.md` D-P2-006、`docs/reports/p1-020-acceptance.md` E-1 回填（RT-021）。
- `crates/bin-tests` 中断言“订阅即 `RUNNING`”的用例全部改写为“占用 + 消息创建”。

#### 4. 必须显式接受的后果

1. **无占用的会话里 `QUEUED` Run 永不执行**，没有后台自执行；客户端必须先占用。
2. `api/run/stream` 与 `RunStreamParams` 彻底消失，属破坏性契约变更，**不提供兼容层**；服务端与 TUI/WebUI 在同一次交付里完成（Batch F）。
3. 一个客户端的下行流断开即意味着其占用的会话停止执行（断连链），客户端需重连 + 重新占用才能继续。

---

## D-P1.1-005 「客户端未注册」的错误表达

- **问题**：客户端被反注册后，其上行请求怎么让客户端明确知道“要重新连接”？
- **背景**：`framework-web` 的 `WebError` 只有 Message / Parameter / NotFound / Unauthorized / Forbidden / Internal / Panic 几类，`code` 与 HTTP 状态码同值，没有 409 / 410，也没有独立业务码。
- **可选方案**：
  1. 400 + 固定 message，客户端按 message 识别。
  2. 404 + 固定 message。
  3. 先给框架加自定义 code 再实现。
- **决策**：方案 1，**过渡方案**。三条固定 message：`客户端未注册，请重新连接` / `会话已被其他客户端占用` / `客户端已占用其他会话`。错误映射集中在一个函数里。
- **决策理由**：400 表达“请求状态不合法”最贴近语义；改造框架需要跨仓库改动，不应阻塞本阶段。等 `framework-web` 确认支持自定义 code 后，只改映射函数，调用方的判断位置不变。
- **待办**：关注 `framework-web` 的自定义 code 方案；有结论后回填本项并替换映射。
- **状态**：`DECIDED`（自定义 code 部分 `暂缓`）。

---

## D-P1.1-006 心跳与问询参数

- **问题**：心跳间隔、超时阈值、问询时机与窗口取什么值？是否开放配置？
- **可选方案**：
  1. 全部写成常量（15s ping / 45s TTL / 30s 问询 / 10s 窗口 / 5s 扫描）。
  2. 全部开放到 `config.toml`。
  3. 只开放 TTL。
- **决策**：方案 1，收敛为常量，不开放配置。
- **决策理由**：这些值只有“客户端与服务端一致”与“容忍一次网络抖动”两个约束，开放配置会让客户端与服务端失配，属负收益；P1 已有 15s 心跳先例（`RunEventKind::Heartbeat`）。
- **配套约束**：问询应答不重置 `lastSeen`（否则超时判定失效）；同一客户端同刻最多一个未答问询。
- **状态**：`DECIDED`。

---

## D-P1.1-007 重启后是否按事件文件恢复

- **问题**：服务重启后，是否读取事件文件恢复运行时会话？
- **可选方案**：
  1. 不恢复，事件文件只作事实源与排障依据。
  2. 启动时扫描并恢复。
- **决策**：方案 1。
- **决策理由**：恢复涉及 Run 状态重建、占用重建与客户端重连时序，属 P6 的 Checkpoint/Resume 范围；本阶段先把“事件按序落盘”这条基础做实。
- **状态**：`DECIDED`。

---

## D-P1.1-008 一个客户端能否占用多个会话

- **问题**：设计原文用复数量词（“停止被**这些**客户端占用的运行时会话”“取消**这些**客户端**的**会话占用”），一个客户端是否能同时占用多个会话？
- **可选方案**：
  1. 只允许一个会话。
  2. 允许多个会话。
- **决策**：方案 1，一对一。
- **决策理由**：下行通道只有一条（`api/client/stream`），多占用会让事件在单条流内混杂多个会话；一对一也让“断连即停会话”的补偿逻辑无需按会话列表遍历。
- **状态**：`DECIDED`。

---

## D-P1.1-009 `RunStreamParams` 的删除时机

- **问题**：RT-005 要求删除 `RunStreamParams`，RT-018 又把同一文件的删除列为自己的范围，两处冲突；而 `run_stream` handler、`service_core::session::service::subscribe_run`、`bin-cli` / `bin-tui` 都引用该类型，RT-005 删除会让工作区无法编译，且此时还没有替代的事件生产方。
- **可选方案**：
  1. RT-005 只新增 `types-core::runtime` 契约，`RunStreamParams` 与 `run_stream` 一起留到 RT-018 删除。
  2. RT-005 提前删除该类型，同时把 handler、`subscribe_run`、客户端消费路径一并改造（把 RT-018 前移至 Batch B）。
- **决策**：方案 1。RT-005 只新增契约类型（含 `RunEventKind::RunCancelled`），`RunStreamParams` 的删除与端点移除统一归 RT-018。
- **决策理由**：RT-018 依赖 Batch C + Batch E 的运行时会话，只有那时才有承接 `RunEvent` 的生产方；提前删除会让 P1 的对话闭环中断，且违反 Batch 之间的强序。
- **配套约束**：RT-005 的验收判据中“`RunStreamParams` 已消失”与“path 常量”两项随 RT-007 / RT-014 / RT-018 收口，`generate` 侧只需确认新类型已进入两份 SDK。
- **状态**：`DECIDED`。

---

## D-P1.1-010 关闭流程的「停定时任务」与轮转触发方式

- **问题**：RT-017 写的是「`is_shutdown` → 停定时任务 → 按断连链处理全部客户端」，RT-016 要求「启动一次 + 每个运行时会话建立后 + 60s 定时」触发轮转；`framework-timer` 没有「停止某个任务」的 API，任务只有周期与按名唤醒两种驱动方式。
- **可选方案**：
  1. 给 `framework-timer` 增加停止 / 注销能力，关闭流程显式停任务。
  2. 任务自己读 `lifecycle::is_shutdown()` 决定是否执行（关闭即空转），轮转的额外触发用 `wake` 按名唤醒。
  3. 关闭流程置位后不再关心定时任务，靠进程退出回收。
- **决策**：方案 2。心跳扫描与事件文件轮转都在入口判一次 `lifecycle::is_shutdown()`，置位后直接返回；轮转由 `#[timer(interval = 60000)]` 的首次立即执行覆盖「启动一次」，由 `framework_timer::wake("event_rotation")` 覆盖「每个运行时会话建立后」。
- **决策理由**：关闭期间的定时任务只剩「多跑一次」的代价，改为空转即可，不需要跨仓库改造框架；`wake` 在运行时未启动时是空操作，用例与单进程场景都不用额外分支。轮转动作本身在阻塞线程池里执行，`wake` 不阻塞事件写入。
- **配套约束**：
  1. 关闭流程的执行顺序固定为「打 `ServerShutdown` 标记（覆盖已有原因）→ 复用断连链 → 摘下残留运行时会话逐个 flush → 拒绝窗口 → 框架优雅关闭」。停客户端与运行时会话必须早于框架的排空窗口，否则长连接下行流会让排空一直等到超时。
  2. 关闭导致的会话终止一律落 `server_shutdown`（`stop_reason`），与 `session/stream` 既有的 `server_shutdown` 事件语义一致；已标记但原因不同的客户端在关闭时被 `mark_shutdown` 覆盖，避免出现「服务关闭却落 `run_cancelled`」。
  3. 轮转保护判据只有占用表快照，不探测文件状态；删除失败只记日志。
- **状态**：`DECIDED`。

---
## D-P1.1-011 服务端主动关流与取消收尾

- **问题**：`client_closed` 帧发出后怎么真正结束 SSE 响应体？`api/run/cancel` 取消后客户端在等什么？
- **背景**：Batch F 落地时发现两处与 D-P1.1-001 / RT-003 的文字描述不一致的实现约束。
- **可选方案**：
  1. 靠丢弃下行通道的发送端来结束响应体。
  2. 由 `lib-web` 的帧合成层在发出 `client_closed` 帧后结束响应体。
- **决策**：方案 2，并明确「取消不产出终态事件」的收尾口径。
- **决策理由**：
  1. 下行流的 watcher（`Registry::open_stream` 里的 `sender.closed()` 等待）持有一份发送端，反注册只丢弃注册表里那一份，发送端永远不会全部 drop，响应体会一直挂住到框架关闭超时。因此把 `client_closed` 作为服务端主动关流的最后一帧，`lib-web` 发完即结束（`crates/lib-web/src/runtime/stream.rs`）。客户端先收到 `client_closed`（带关闭原因）再读到 EOF。
  2. `api/run/cancel` 属客户端主动取消，不属于 RT-003 的三个固定取消原因（占用释放 / 连接断开 / 服务关闭），因此不新增终态文本：取消后不再为该 Run 产出事件，客户端（TUI / WebUI / CLI）在取消响应后**本地收尾**；事件文件的终态由会话停止时的写者补 `run_cancelled` 兜底。
- **配套约束**：
  1. `RunEventKind::RunCancelled` 的 `error` 文本仍只取三个固定文案，不因显式取消扩张。
  2. RT-018 的触发点落地为「执行队列随运行时会话起停」：`RunExecutor` 由 `RuntimeSession::start` 启动、`stop` 时先中止再取消 Run；`message/create` 通过 `service_core::runtime::notify_run` 唤醒队列，未被占用的会话只记 `info` 日志（Run 停在 `QUEUED`）。
  3. 客户端不再有任何 Run 级流：TUI 的 `client/stream` 消费在 `SessionEvent` 之外还要处理 `inquiry`（就地 `INQUIRY_ACK`）与 `client_closed`（收尾提示）。
- **状态**：`DECIDED`（RT-018 ~ RT-020 落地）。

---

## 被本阶段修订的既有决策

按仓库 `AGENTS.md` 的文档维护约束，回填写入对应文件，执行期间不改它们；**回填已在 RT-021（2026-11-20）完成**，逐项结果如下：

| 位置 | 原结论 | 修订方向 |
|---|---|---|
| `.steps/p1/decisions.md` D-007 | `api/run/stream` 按 `Blob` 进元数据，客户端自实现 SSE 消费 | 端点移除；SSE 契约转移到 `api/client/stream` |
| `.steps/p1/decisions.md` D-011 | TUI 用「单一长连接 SSE + 独立 HTTP 请求」，手动重连 | 连接管理加入注册/心跳/占用，重连判据改为“客户端未注册” |
| `.steps/p1/decisions.md` D-026 | 订阅 `api/run/stream` 即推进 `RUNNING`，消费方断开则落 `CANCELLED` | 触发点改为“会话被占用 + 消息创建”；断开语义转移到断连处理链 |
| `.steps/p2/decisions.md` D-P2-006 | Run 由 `api/run/stream` 订阅触发 | 同上；已在 P2 方案重新生成时标 `REJECTED`，新结论见 `p2/decisions.md` D-P2-028 |
| `docs/reports/p1-020-acceptance.md` E-1 | 以 `api/run/stream` 的帧格式作为判据 | 判据转移到 `api/client/stream` |
