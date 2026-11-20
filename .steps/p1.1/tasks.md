# P1.1 任务清单

**状态：已开工（Batch A 定稿完成：RT-001~RT-004；Batch B 完成：RT-005~RT-006；Batch C 完成：RT-007~RT-010；Batch D 完成：RT-011~RT-013；Batch E 完成：RT-014~RT-017；Batch F 完成：RT-018~RT-020）。** 任务状态维护规则见 `.steps/README.md` 第六节；阶段边界、协议与模块落位见本目录 [README.md](README.md)，判据见 [acceptance.md](acceptance.md)，结论见 [decisions.md](decisions.md)。

批次之间强序，批次内可并行（每批开头给出依赖与并行关系）。**每个任务的「测试要求」只允许在 `crates/bin-tests` 落地用例；其他 Crate 不得写 `#[cfg(test)]`**（仓库 `AGENTS.md` 第 28 条）。

---

## Batch A：定稿与协议落地细节（RT-001 ~ RT-004）

> 依赖：本阶段确认。**本批次不写业务代码**，结论回填 `README.md` 与 `decisions.md`。

- [x] **RT-001 上行协议细节定稿**

  **目标**：把“SSE 下行 + POST 上行”落成可实现的细节：ping 与应答的请求形态、下行流断开的服务端感知方式、`clientId` 只在查询参数/请求体（无路径占位符，D-008）。
  **涉及文件 / Crate**：`decisions.md`（D-P1.1-001）；不写代码。
  **验收标准**：
  - [x] 每个端点的入参位置、返回形态（`R<T>` / `Blob`）明确。
  - [x] 下行流断开后“先标记、后由处理链清理”的时序写明。
  - [x] 明确 `service-core` 不感知传输（事件值类型 + 通道）。
  **完成产物**：D-P1.1-001 定稿。
  **测试要求**：无（本任务不写代码）。

- [x] **RT-002 显式占用契约定稿**

  **目标**：定稿 `api/occupancy/create|delete|get` 的语义边界。
  **涉及文件 / Crate**：`decisions.md`（D-P1.1-002）；不写代码。
  **实施步骤**：确定占用与运行时会话的创建顺序（先占用成功后建会话/建文件）；确定冲突的三条固定 message；确定“客户端已占用其他会话”时是否需要客户端先 delete；确定占用释放与 Run 执行的联动（释放后仍在跑的 Run 落 `CANCELLED`）。
  **验收标准**：
  - [x] 一对一约束、冲突表达、释放与 Run 状态的联动三条结论明确。
  - [x] 与 `docs/api/readme.md` 错误契约不冲突。
  **完成产物**：D-P1.1-002 定稿。
  **测试要求**：无。

- [x] **RT-003 事件文件与轮转配置定稿**

  **目标**：定稿路径、格式、seq、写序、flush 粒度、终态事件、轮转配置默认值与触发时机。
  **涉及文件 / Crate**：`decisions.md`（D-P1.1-003）；不写代码。
  **验收标准**：
  - [x] `README.md` 第六节表格逐行有结论（含 `max_sessions` / `max_bytes` 默认值）。
  - [x] 数据量估算与“保护占用中会话”的实现方式写明。
  - [x] `[runtime.events]` 在 `ServerConfig` 中的字段与透传路径写明。
  **完成产物**：D-P1.1-003 定稿。
  **测试要求**：无。

- [x] **RT-004 Run 触发点与 `run/stream` 移除定稿**

  **目标**：`api/run/stream` 已定移除，需定稿替代触发点与迁移清单。
  **涉及文件 / Crate**：`decisions.md`（D-P1.1-004）；不写代码。
  **实施步骤**：
  1. 定触发点：会话被占用时，该会话的 `QUEUED` Run 立即推进 `RUNNING` 并启动执行；无占用时 Run 保持 `QUEUED`、不执行（与“会话运行时”语义一致）。若与 P2 期望不符，先确认再往下走。
  2. 定客户端编排顺序（`README.md` 7.1 时序）与失败回滚（占用建立后消息创建失败 → 占用保留还是释放）。
  3. 列迁移清单：`lib-web/src/session/api.rs` 的 `run_stream`、`lib-web/src/session/stream.rs`、`lib-bin-cli::client::subscribe_run`、TUI/WebUI 的流消费、`RunStreamParams` 类型、SDK 生成物。
  4. 列 P1/P2 回填项：D-007（SSE 契约）、D-011（TUI 连接管理）、D-026（run/stream 订阅即运行）、D-P2-006（Run 由订阅触发）、`docs/reports/p1-020-acceptance.md` 的 E-1 判据。
  **验收标准**：
  - [x] 触发点规则单一且无歧义。
  - [x] 迁移清单覆盖服务端、客户端、类型、文档四处。
  - [x] 已确认“移除后无订阅者的 `QUEUED` Run 永不执行”这一后果被接受。
  **完成产物**：D-P1.1-004 定稿。
  **测试要求**：无。

---

## Batch B：契约与骨架（RT-005 ~ RT-006）

> 依赖：Batch A。两个任务可并行。

- [x] **RT-005 `types-core::runtime` 契约类型**

  **目标**：一次到位定义客户端三接口 + 占用三接口 + 下行事件类型（`#[auto_*]` 标注，参与 SDK 生成）。
  **前置依赖**：RT-001、RT-002。
  **涉及文件 / Crate**：`crates/types-core/src/runtime/{mod,client,occupancy}.rs`、`crates/types-core/src/lib.rs`。
  **技术要求**：wire 约定沿用 P1（`i64` → 字符串、枚举 `SCREAMING_SNAKE_CASE`）；`api/run/stream` 相关的 `RunStreamParams` 按 RT-004 清单删除，`RunEvent` / `RunEventKind` **保留**（事件文件与下行流继续用）。
  **验收标准**：
  - [x] `make gen` 后两份 SDK 均出现新类型（`Client*` / `Occupancy*` / 下行事件）；path 常量随 RT-007 / RT-014 的 handler 落地，`RunStreamParams` 的删除按 D-P1.1-009 归 RT-018。
  - [x] 下行事件枚举 wire 值与 SSE 的 `event:` 行一致。
  **测试要求**：`bin-tests` 一条序列化快照用例。

- [x] **RT-006 `RuntimeState` 骨架与注入路径**

  **目标**：注册表 + 占用表 + 标记集合的最小可编译骨架，`lib-bin-web` 启动时构造注入。
  **前置依赖**：RT-005（可与其并行，但落地顺序在其后）。
  **涉及文件 / Crate**：`crates/service-core/src/runtime/{mod,registry,client}.rs`、`crates/lib-bin-web/src/server.rs`。
  **技术要求**：锁与 `await` 规则见 `README.md` 第五节的并发规则；`holder` 替换留到 P2 的 Repository 注入时一并做，本任务只新增并行路径。
  **验收标准**：
  - [x] `cargo check` 通过。
  - [x] 注册/查询/移除与 take 语义有覆盖。
  **测试要求**：`bin-tests` 覆盖“重复标记只处理一次”。

---

## Batch C：连接与心跳（RT-007 ~ RT-010）

> 依赖：Batch B。RT-007 与 RT-010 可并行；RT-008 依赖 RT-007；RT-009 依赖 RT-008。

- [x] **RT-007 注册与下行流端点**

  **目标**：`POST api/client/create` + `GET api/client/stream` 打通，首个 pong 落在注册响应。
  **涉及文件 / Crate**：`crates/lib-web/src/runtime/{api,stream}.rs`、`crates/service-core/src/runtime/{registry,client}.rs`。
  **实施步骤**：handler 只做解包/调用/映射；下行流用 `WebBody::Stream` + `sse::json_frame`；流 drop 时只标记 `StreamClosed`，清理交给处理链。
  **验收标准**：
  - [x] 注册返回 `clientId` 与 pong 时间戳。
  - [x] 下行流断开后注册表出现 `StreamClosed` 标记，且只出现一次。
  **测试要求**：`bin-tests` 覆盖注册 → 建流 → 断流 → 标记。

- [x] **RT-008 心跳上行与未注册拒绝**

  **目标**：`POST api/client/update {PING}` 更新 `lastSeen` 并同步返回 pong；未注册返回 **400 + 固定 message**。
  **涉及文件 / Crate**：`crates/lib-web/src/runtime/api.rs`、`crates/service-core/src/runtime/client.rs`。
  **技术要求**：错误映射集中在一处函数，便于框架支持自定义 code 后只改这里。
  **验收标准**：
  - [x] 已注册 ping 前进 `lastSeen`；已移除的 `clientId` 得到 400 `客户端未注册，请重新连接`。
  - [x] 心跳路径不产生业务事件。
  **测试要求**：`bin-tests` 覆盖已注册 / 未注册两种结果。

- [x] **RT-009 问询与应答**

  **目标**：超过 `INQUIRY_AFTER` 未心跳时下行一条 `inquiry`；`INQUIRY_WINDOW` 内收到 `INQUIRY_ACK` 清除待答状态。
  **涉及文件 / Crate**：`crates/service-core/src/runtime/client.rs`、`crates/lib-web/src/runtime/api.rs`。
  **技术要求**：同一客户端同刻最多一个未答问询；应答不重置 `lastSeen`（否则超时判定失效）。
  **验收标准**：
  - [x] 无应答 → `InquiryTimeout`；有应答 → 不标记。
  - [x] 不叠加多条问询。
  **测试要求**：`bin-tests` 覆盖两种结局。

- [x] **RT-010 定时任务（心跳扫描 + 问询投递）**

  **目标**：`framework-timer` 任务，周期扫描超时与问询，统一走 registry 的标记接口。
  **涉及文件 / Crate**：`crates/service-core/src/runtime/timer.rs`、`crates/service-core/Cargo.toml`（新增 `framework-timer` / `framework-proc-timer`）。
  **技术要求**：幂等、无阻塞；只做标记 + 通知；关闭后不再投递问询。
  **验收标准**：
  - [x] 超时客户端在 TTL + 一个扫描周期内被标记。
  - [x] 关闭后任务停止，无残留通知。
  **测试要求**：`bin-tests` 用缩短常量覆盖，不引入真实等待。

---

## Batch D：断连处理链（RT-011 ~ RT-013）

> 依赖：Batch C。三个任务强序。

- [x] **RT-011 断连通知与串行处理入口**

  **目标**：断连事件类型定义在 `types-core`；`service-core` 订阅 `framework-bus` 并**串行**执行处理链。
  **涉及文件 / Crate**：`crates/types-core/src/runtime/`、`crates/service-core/src/runtime/handler.rs`。
  **技术要求**：通知不携带数据 → 处理函数只做唤醒，原因从 `RuntimeState` 读；`Dispatcher` 必须保证同一处理函数不并发。
  **验收标准**：
  - [x] 五条来源触发同一入口，处理函数不并发。
  - [x] 通知合并时结果不变。
  **测试要求**：`bin-tests` 覆盖“合并通知只处理一次”。

- [x] **RT-012 反注册与拒绝语义**

  **目标**：移除记录 → 释放占用记录 → 关闭下行通道；此后 `update` / `delete` / `occupancy/*` 一律 400 固定 message。
  **涉及文件 / Crate**：`crates/service-core/src/runtime/{handler,registry}.rs`。
  **验收标准**：
  - [x] 重新注册后可再次占用同一会话，无残留记录与残留文件句柄。
  **测试要求**：`bin-tests` 覆盖“断开 → 旧 id 被拒 → 重新注册 → 再次占用成功”。

- [x] **RT-013 运行时会话停止与占用取消的补偿**

  **目标**：停止该客户端占用的运行时会话、追加终态事件、flush、释放占用；任一步失败都继续后续步骤并记日志。
  **涉及文件 / Crate**：`crates/service-core/src/runtime/{handler,session,occupancy}.rs`。
  **验收标准**：
  - [x] 断开后事件文件以终态事件收尾，占用表无残留。
  - [x] 单步失败不影响其余清理。
  **测试要求**：`bin-tests` 覆盖多会话同停与单点失败。

---

## Batch E：占用、事件文件与轮转（RT-014 ~ RT-017）

> 依赖：Batch B。RT-014 ~ RT-016 可与 Batch C/D 并行；RT-017 依赖 Batch D。

- [x] **RT-014 显式占用接口**

  **目标**：`api/occupancy/create|delete|get` 落地；一对一约束；成功即建运行时会话与事件文件。
  **涉及文件 / Crate**：`crates/lib-web/src/runtime/occupancy_api.rs`、`crates/service-core/src/runtime/occupancy.rs`。
  **验收标准**：
  - [x] 第二个客户端占用同一会话得到 400 `会话已被其他客户端占用`。
  - [x] 客户端已占一个会话时再占第二个得到 400 `客户端已占用其他会话`。
  - [x] 释放后同一会话可再次占用（新运行时会话、新文件）。
  **测试要求**：`bin-tests` 覆盖三类冲突与释放后重占。

- [x] **RT-015 事件按序写入与转发**

  **目标**：生产方 → 写者 → 文件 + 下行通道；seq 单调；先落盘后转发。
  **涉及文件 / Crate**：`crates/service-core/src/runtime/{journal,session}.rs`。
  **技术要求**：文件写入走 `spawn_blocking` 或专用写者任务，不占 worker；慢客户端 `try_send` 失败即标记 `StreamClosed`。
  **验收标准**：
  - [x] 文件内 seq 连续且顺序等于生产顺序。
  - [x] 客户端断开后事件仍全部落盘。
  **测试要求**：`bin-tests` 覆盖“无占用也落盘”与“慢客户端不阻塞落盘”。

- [x] **RT-016 轮转清理**

  **目标**：按 `max_sessions` / `max_bytes` 任一超限清理，最旧会话目录先删，保护占用中目录。
  **涉及文件 / Crate**：`crates/service-core/src/runtime/journal.rs`（清理）、`timer.rs`（周期任务）、`crates/lib-bin-cli/src/config.rs` + `crates/lib-bin-web/src/server.rs`（配置透传）。
  **技术要求**：统计与删除都在独立任务里做，不阻塞事件写入；删除失败跳过并继续；0 表示不限制。
  **验收标准**：
  - [x] 会话数超限、字节数超限、两者同时超限三种场景都只清到限内。
  - [x] 占用中的会话目录从不被删。
  - [x] 配置缺省时使用默认值；显式 0 关闭限制。
  **测试要求**：`bin-tests` 用临时目录覆盖三种超限场景与保护规则。

- [x] **RT-017 关闭流程集成**

  **目标**：`is_shutdown` → 停定时任务 → 按断连链处理全部客户端（`ServerShutdown`）→ 停全部运行时会话并 flush → 交给现有排空窗口。
  **涉及文件 / Crate**：`crates/lib-bin-web/src/{server,shutdown}.rs`、`crates/service-core/src/runtime/handler.rs`。
  **验收标准**：
  - [x] 关闭后无残留 `RUNNING` 运行时会话、无未 flush 文件。
  - [x] 与现有 `server_shutdown` 事件语义不冲突。
  **测试要求**：`bin-tests` 复用 P1 关闭用例口径。

---

## Batch F：契约移除与客户端接入（RT-018 ~ RT-020）

> 依赖：Batch C + Batch E。三个任务强序（客户端必须与服务端同批交付）。

- [x] **RT-018 移除 `api/run/stream` 与服务端 Run 触发改造**

  **目标**：按 RT-004 清单删除 `run_stream` 端点、`lib-web/src/session/stream.rs`、`RunStreamParams`、`Client::subscribe_run`；新增“会话被占用时推进 `QUEUED` Run”的触发逻辑。
  **涉及文件 / Crate**：`crates/lib-web/src/session/api.rs`、`crates/service-core/src/session/{service,stream,run_trigger}.rs`、`crates/lib-bin-cli/src/{client,run}.rs`、`crates/types-core/src/session/request.rs`。
  **技术要求**：Run 事件生产方（P1 的 Mock / P2 的真实执行）改为写入运行时会话，不再直接写 SSE 响应。
  **验收标准**：
  - [x] `api/run/stream` 404；SDK 生成物里无该 path。
  - [x] 占用中的会话：消息创建后 Run 进入 `RUNNING`，事件按序落盘并转发到 `api/client/stream`。
  - [x] 未被占用的会话：Run 停在 `QUEUED`，日志记录原因。
  **测试要求**：`bin-tests` 覆盖两条分支与旧端点 404。

- [x] **RT-019 TUI 接入**

  **目标**：`lib-bin-tui` 改为“注册 → 建流 → 占用 → 发消息 → 消费下行流 → 退出前 delete”。
  **涉及文件 / Crate**：`crates/lib-bin-tui/`。
  **技术要求**：沿用 P1 的手动重连（D-011 第 3 条）；识别到“客户端未注册”（400 + 固定 message）时整体重连，不做增量拼接；重连后按服务端消息整体重载。
  **验收标准**：
  - [x] 服务端重启后 `Ctrl+R` 恢复；空闲时心跳持续。
  - [x] 退出路径先 `client/delete` 再关闭工作线程。
  **测试要求**：`bin-tests` 的 `tui.*` 用例覆盖心跳与未注册后的重连迁移。

- [x] **RT-020 WebUI 接入**

  **目标**：`lingting-agent-ui/src/api/` 增加注册/心跳/下行流/占用，`useChat` 迁移到客户端级事件；移除 `run/stream` 消费代码。
  **涉及文件 / Crate**：`lingting-agent-ui/src/api/`、`lingting-agent-ui/src/pages/chat/`。
  **技术要求**：心跳定时器随页面卸载清理；不做本地半条消息拼接（D-011 第 4 条口径）。
  **验收标准**：
  - [x] 刷新页面后旧 `clientId` 靠 TTL 与流断标记回收，新注册不残留旧记录。
  **测试要求**：无（前端测试按 P1 口径不引入）。

---

## Batch G：文档与验收（RT-021 ~ RT-022）

> 依赖：全部批次。

- [ ] **RT-021 文档与既有决策回填**

  **目标**：按 `README.md` 第十一节的清单逐项回填。
  **涉及文件 / Crate**：`docs/workspace.md`、`docs/api/{readme,runtime,occupancy,session,probe}.md`、`.steps/README.md`、`.steps/p1/decisions.md`、`.steps/p2/decisions.md`、`docs/reports/p1-020-acceptance.md`。
  **验收标准**：
  - [ ] 文档与代码一致，无第二份契约定义。
  - [ ] D-007 / D-011 / D-026 / D-P2-006 的修订说明已写，`REJECTED` 记录保留。
  **测试要求**：无。

- [ ] **RT-022 端到端验收**

  **目标**：`crates/bin-tests` 运行时用例集：注册/心跳/超时/问询/主动断开/流断开/占用冲突/一对一约束/事件落盘顺序/轮转清理/关闭收尾/重连/旧端点 404。
  **验收标准**：`acceptance.md` 逐条通过。
  **测试要求**：全部用例在 `bin-tests`，其他 Crate 不写 `#[cfg(test)]`。
