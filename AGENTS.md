# AGENTS.md

本文件声明 Coding Agent 在本仓库必须遵守的约束。**只写约束，不写解释**；需要理由时引用对应文档。

## 文档权威来源

- Workspace 架构与依赖关系以 `docs/workspace.md` 为准。
- 开发规范以 `docs/develop.md` 为准。
- 接口文档入口为 `docs/api/readme.md`，按领域一文件（`docs/api/<领域>.md`）。
- 阶段开发计划以 `.steps/README.md` 及对应阶段文档为准。

## 强制约束

### Crate 结构与依赖

1. 修改 Crate 结构、职责或依赖关系时，必须同步更新 `docs/workspace.md`。
2. 不得擅自新增已明确禁止的 `manager-core`、`types-api`。
3. 当前不创建 `manager-core`。
4. 同一个 Service 内部的 Manager 放在当前 Crate 的 `manager` 模块。
5. 只有跨多个独立 Service Crate 的业务协调，才创建独立的 `manager-<domain>`（按业务领域命名，不建 `manager-core`）。
6. Service Crate 不得反向依赖 Manager Crate。
7. `repository` 只由当前 Service Crate 内部的 Service 调用，不得作为其他 Crate 直接调用的公共业务接口。
8. 禁止循环依赖。
9. 基础 Crate 不得反向依赖上层业务 Crate。
10. Agent Core 不依赖具体 Web 框架、TUI、CLI。
11. 业务 Crate 不依赖 TUI / CLI / WebUI。
12. Binary 只负责组装与启动，不承载业务逻辑。
13. 不允许因为方便就把通用能力放进 `lib-core`。
14. 不允许用 `types-core` 替代各领域 Crate 的类型组织。
15. 不允许通过共享全局状态绕过 Crate 依赖边界。
16. 新增依赖必须考虑 Crate 的职责与依赖方向。
17. 第三方框架与协议尽量通过 Adapter 隔离，不得让协议类型渗透 Agent Runtime。
18. 不得擅自拆分微服务。

### 类型与 API

19. 不创建 `types-api`，不为 HTTP API 维护第二套 DTO 类型体系。
20. API 类型通过所属领域 Crate 中的 `#[auto_*]` 框架宏标记参与生成；内部类型不要求使用这些宏。
21. 客户端不得手写与服务端重复的 API 定义。
22. 所有客户端 API 请求必须使用由服务端 API 定义生成的 SDK。
23. 不得让客户端直接调用 Agent Runtime。
24. 不得让第三方协议类型污染 Agent Core。

### 阶段范围

25. P1 只实现 `bin-agent`，不创建 P1 之外的其他 Binary。
26. 未经明确规划，不得提前创建 `bin-tui`、`bin-web`、`bin-mock-server`。
27. 不得超出当前阶段的开发范围。

## 文档维护约束

- `README.md` 仅保留功能介绍与文档索引。
- `AGENTS.md` 仅保留约束。
- `docs/develop.md` 用于后续补充开发规范，未正式进入实施前保持占位。
- `docs/workspace.md` 是架构与依赖关系的唯一事实来源。
- 未经明确要求，不得主动同步或修改文档。
