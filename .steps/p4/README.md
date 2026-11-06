# P4：MCP Integration

**状态：未开始（仅阶段级规划，暂不拆任务）**

---

## 一、阶段目标

通过 MCP（Model Context Protocol）让 Agent 接入外部工具生态，且**不污染 Agent Core**。

核心约束：MCP 只是工具的一种来源。MCP 工具进入系统后，必须被适配成 P3 的 `ToolDefinition`，由 P3 的 `ToolExecutor` 统一执行。Agent Core 不出现任何 MCP 协议类型。

---

## 二、主要能力

| 能力 | 说明 |
|------|------|
| MCP Client Manager | 管理多个 MCP Client 实例的生命周期与连接状态 |
| MCP Server Registry | MCP Server 配置的注册、启用/禁用、持久化 |
| Server Configuration | 传输方式、启动命令、环境变量、超时、重连策略 |
| Tool Discovery | 连接后拉取工具列表，映射为项目内 ToolDefinition |
| Tool Adapter | MCP Tool ↔ 内部 Tool 的双向适配（名称、schema、结果） |
| stdio Transport | 作为子进程启动 MCP Server，走 stdin/stdout |
| Streamable HTTP Transport | 通过 HTTP 连接远程 MCP Server |
| Process Lifecycle | 子进程启动、健康检查、异常退出、优雅终止、孤儿进程清理 |
| Connection Management | 连接池、重连退避、连接状态对外可见 |
| Error Handling | 传输错误、协议错误、工具执行错误统一归入 P3 的错误语义 |

---

## 三、核心技术

- **rmcp**：MCP Client 实现、transport（stdio / streamable HTTP）、工具发现与调用。
- **Tokio process**：stdio 子进程管理、输出读取、异常退出检测。
- **P3 Tool System**：工具注册与执行入口（MCP 只是 provider）。
- **SQLite**：MCP Server 配置与连接状态持久化（复用 P2 的存储层）。
- **SSE 事件**：工具调用事件（P3 已定义）无需新增 MCP 专属事件。

---

## 四、与前一阶段的依赖

| 依赖项 | 用途 |
|--------|------|
| P3 的 Tool Trait / Registry / Executor | MCP 工具必须适配为内部工具，不允许旁路执行 |
| P3 的权限与审计 | MCP 工具同样受权限约束、同样进审计 |
| P2 的持久化 | MCP Server 配置落库 |
| P2 的 Run 生命周期 | MCP 工具调用属于 Run 的一部分，随 Run 取消 |
| P1 的 API / SDK 生成 | 管理接口（列表、启用、探测、重连）的客户端 |

---

## 五、主要交付物

1. MCP Client Manager 与多 Server 并发管理。
2. MCP Server Registry 与配置管理接口。
3. stdio 与 streamable HTTP 两种 transport 的接入。
4. 工具发现与 Schema 映射（MCP schema → 内部 ToolDefinition）。
5. 子进程生命周期管理与孤儿进程清理。
6. 连接失败 / 断开 / 重连的状态可见性与错误归一化。

---

## 六、阶段验收方向

1. 配置一个 stdio MCP Server 后，其工具自动出现在可用工具列表中，且能被 Agent 调用成功。
2. 配置一个 HTTP MCP Server 后行为一致。
3. MCP Server 进程崩溃或断开时，系统给出明确状态，Agent 调用得到可读错误而不是挂死。
4. 取消 Run 或关闭 Server 时，MCP 子进程被终止，无孤儿进程残留。
5. MCP 工具调用同样进入审计记录与权限校验。
6. Agent Core 代码中不出现 MCP 协议类型（以依赖方向检查为证据）。

---

## 七、Non-goals（本阶段明确不做）

- 把本项目实现为 MCP **Server**（对外暴露自身工具）——如需要，排到 P6 之后单独评估。
- MCP 的 prompts / resources / sampling 等非工具能力（仅在有明确需求时评估）。
- MCP Server 的安装与包管理（只负责连接已存在的 Server）。
- 跨机器的 MCP 服务编排与认证体系。

---

## 八、风险与待讨论问题

1. **rmcp 版本与 transport 支持范围**：需在阶段开始时核实 rmcp 当前版本对 streamable HTTP 的实现状态，以及 API 稳定性。
2. **协议类型隔离的具体手段**：Adapter 层的类型边界如何强制（crate 依赖方向、`#[deny]` 规则），避免 MCP 类型顺着 Result 泄漏进 Agent Core。
3. **工具命名冲突**：不同 MCP Server 提供同名工具时的命名空间策略（前缀、别名、冲突拒绝）。
4. **Schema 兼容性**：MCP 工具 schema 中不被现有 Tool Schema 机制支持的部分（联合类型、复杂嵌套）如何降级。
5. **安全边界**：stdio MCP Server 是任意可执行程序，启动即代表授权执行；是否需要显式确认、是否限制可执行路径。
6. **资源占用**：多个 stdio Server 常驻子进程的内存与句柄开销；是否按需启动。
7. **故障隔离**：单个 MCP Server 卡死不能拖垮整个 Run 或 Server 进程。
