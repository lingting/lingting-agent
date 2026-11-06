# Personal Agent

用 Rust 实现的 **Personal Agent**：不是 ChatGPT 的包装壳，而是一个可独立安装、可单体部署、可扩展的 **Agent Runtime**。

## 项目介绍

本项目以 Native Binary 分发：产物是可直接运行的可执行文件，不要求用户预装 JVM / Python / Node 运行时；服务端以单体进程运行，不提前拆分微服务。

当前仓库处于**路线图阶段**（见 `.steps/`），尚未开始编码。下面严格区分「计划能力」与「已实现能力」。

### 计划能力

**入口形态**

- **CLI**：单次任务执行、状态查询、帮助与版本信息。
- **TUI**：终端内的会话界面，支持流式输出展示与中断。
- **WebUI**：浏览器内的对话界面，静态资源随 Binary 分发，单文件即可部署。
- **Server**：本地常驻进程，承载全部业务逻辑与 Agent 执行。
- 以上形态由**同一个可执行文件**通过命令行参数切换。

**Agent 能力**

- **多 LLM Provider**：Provider / Model 配置管理，支持多家 OpenAI 兼容供应商。
- **Agent Runtime**：自研 Agent Loop、Run 生命周期（创建 / 运行 / 取消 / 失败）、流式事件、超时与取消。
- **Context**：系统提示、会话历史、Skill 上下文、Tool 定义上下文、token 预算与上下文裁剪。
- **Tool**：统一 Tool Trait / Registry / Executor，模型可调用的执行能力，含权限与审计。
- **MCP**：接入外部工具生态，MCP 工具被适配为内部统一 Tool。
- **Skills**：`SKILL.md` 发现、解析与渐进披露，为 Agent 提供任务知识。
- **Memory**：跨会话长期记忆、上下文压缩、RAG 与向量检索（规划能力）。

**工程能力**

- 服务端 API 契约与客户端 SDK 生成链路，含生成物一致性门禁。
- 本地持久化，会话与历史可恢复。
- 单 Binary Release 构建与跨平台分发（规划能力）。
- 可观测性与评测框架（规划能力）。

### 已实现能力

暂无。当前仓库仅有 `.steps/` 路线图文档，没有源码、没有 Cargo Workspace、没有可运行的 Binary。

## 文档索引

| 文档 | 说明 |
|---|---|
| [docs/workspace.md](docs/workspace.md) | Workspace 架构和依赖规范 |
| [docs/develop.md](docs/develop.md) | 开发规范 |
| [.steps/README.md](.steps/README.md) | 项目开发路线图 |
