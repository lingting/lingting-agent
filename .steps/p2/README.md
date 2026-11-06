# P2：基础 Agent Runtime

**状态：未开始（仅阶段级规划，暂不拆任务）**

> 本阶段在 P1 验收通过后、正式开工前才展开为 `tasks.md` / `acceptance.md` / `decisions.md`。以下只定义范围与方向。

---

## 一、阶段目标

在 P1 的通信骨架上，接入真实 LLM，形成**可以持续多轮对话、可流式、可隔离、可恢复**的 Agent 基础运行时。

关键点：Agent Runtime 由项目自己实现，Rig 只提供 LLM Provider 与 Completion 能力。`AgentLoop`、`ContextBuilder`、`Run` 状态机都在项目内。

---

## 二、主要能力

| 能力 | 说明 |
|------|------|
| Rig Provider Adapter | 把 Rig 的模型客户端适配为项目内的 `LlmProvider` 抽象，隔离第三方类型 |
| Agent Loop | 单轮循环：构建上下文 → 调用模型 → 解析输出 → 追加消息 → 判断是否继续 |
| Session Management | 会话创建、查询、删除；Session 之间强隔离 |
| Message Management | 用户/助手/系统消息的存储与顺序保证；消息状态（进行中/完成/失败） |
| Context Builder | 从 Session 历史构建模型上下文（系统提示、历史裁剪、消息角色映射） |
| Context Window Budget | token 预算计算与超限策略（早期只做截断 + 保留最近 N 轮） |
| Conversation History | 历史读取与分页；上下文与展示分离 |
| Streaming Response | 模型 token 流经 SSE 推给客户端，复用 P1 的 `RunEvent` 契约 |
| SQLite 持久化 | Session / Message / Run 落盘；Repository 抽象 + SQLite 实现 |
| Run 生命周期 | `pending → running → completed / cancelled / failed`；状态机唯一权威在服务端 |
| Run Cancellation | 取消信号传递到模型流，释放资源，落库终态 |

---

## 三、核心技术

- **Rig**：Provider（OpenAI 兼容 / 其他）、Completion 调用、流式响应。
- **Tokio**：任务隔离（每个 Run 一个任务）、取消（`CancellationToken` 风格）、超时。
- **SQLite**：驱动选型在阶段开始时决定（`sqlx` vs `rusqlite`），迁移方案一并确定。
- **Serde**：消息与事件序列化，与 P1 的 DTO 契约保持字段稳定。
- **SSE**：复用 P1 的 `WebResponse::stream` 通路，只替换事件生产方。

---

## 四、与前一阶段（P1）的依赖

| 依赖项 | 用途 |
|--------|------|
| 生成的 Rust / TS SDK | 客户端调用新接口时无需手写定义 |
| `RunEvent` 事件契约 | 真实 token 流直接复用，只增事件类型不改已有语义 |
| 统一错误码清单 | 新增 Provider 错误、限流、上下文超限等错误码（只追加） |
| Server 生命周期与优雅关闭 | Run 取消与关机联动 |
| WebUI / TUI 的流式渲染能力 | 无需改造即可显示真实输出 |

---

## 五、主要交付物

1. `LlmProvider` 抽象与 Rig Adapter 实现。
2. Agent Loop 与 Run 状态机。
3. Context Builder 与 token 预算策略。
4. SQLite Repository 实现与迁移脚本。
5. 真实 LLM 的 Streaming 通路（替换 P1 的 Mock SSE）。
6. 服务端重启后会话与历史可恢复。
7. 配置项：模型、API Key、Base URL、超时、并发上限。

---

## 六、阶段验收方向

1. 可进行多轮对话，模型回复流式可见。
2. 两个 Session 并发对话，消息互不串扰。
3. 服务端重启后，历史会话与消息完整恢复。
4. 取消进行中的 Run 后，状态为 `cancelled`，资源释放，客户端收到终止事件。
5. 模型调用失败（鉴权错误、超时、限流）时，客户端收到可读错误，Run 落为 `failed`。
6. API 定义变更后重新生成 SDK 与一致性检查仍然通过。

---

## 七、Non-goals（本阶段明确不做）

- Tool 调用（P3）。
- MCP（P4）、Skills（P5）。
- Multi-Agent 与 Workflow（P6）。
- 复杂 RAG、向量检索、长期记忆（P6）。
- 上下文压缩（除截断外的高级策略，P6）。
- 多用户与权限体系。

---

## 八、风险与待讨论问题

1. **Rig 抽象粒度**：Rig 的 Agent / Completion 抽象与我们自研 Agent Loop 的职责边界如何切分，避免“用 Rig 的循环又自己写循环”。
2. **SQLite 驱动与并发模型**：`sqlx`（异步、编译期检查）vs `rusqlite`（同步、需 `spawn_blocking`），在单进程多 Run 并发下的写入争用。
3. **Context Budget 策略**：按 token 精确计数（需要 tokenizer，增加依赖）还是按字符估算（误差大）。初期可估算，但要预留替换点。
4. **Run 并发上限**：是否限制同时运行的 Run 数；超限排队还是拒绝。
5. **API Key 存储**：配置文件明文 / 环境变量 / 系统密钥库；影响分发形态。
6. **流式与持久化的一致性**：流中断时已产生的部分内容是否落库（影响“重连后能看到多少”）。
7. **契约稳定性**：P2 会新增较多 DTO 字段，需确认 P1 的 DTO 是否留够扩展空间。
