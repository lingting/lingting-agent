# Personal Agent

用 Rust 实现的 **Personal Agent**：不是 ChatGPT 的包装壳，而是一个可独立安装、可单体部署、可扩展的 **Agent Runtime**。

## 项目介绍

本项目以 Native Binary 分发：产物是可直接运行的可执行文件，不要求用户预装 JVM / Python / Node 运行时；服务端以单体进程运行，不提前拆分微服务。

## 文档索引

| 文档 | 说明 |
|---|---|
| [docs/workspace.md](docs/workspace.md) | Workspace 架构和依赖规范 |
| [docs/develop.md](docs/develop.md) | 开发规范 |
| [docs/api/readme.md](docs/api/readme.md) | 接口文档索引（按领域分文件，含通用约定） |
| [.steps/README.md](.steps/README.md) | 项目开发路线图 |
| [docs/reports/p1-001-environment.md](docs/reports/p1-001-environment.md) | P1-001 仓库与开发环境勘察报告 |
| [docs/reports/p1-004-api-probe.md](docs/reports/p1-004-api-probe.md) | P1-004 服务端 API 定义验证报告 |
| [docs/reports/p1-005-ts-sdk.md](docs/reports/p1-005-ts-sdk.md) | P1-005 TypeScript SDK 生成链路验证报告 |
| [docs/reports/p1-006-rust-sdk.md](docs/reports/p1-006-rust-sdk.md) | P1-006 Rust Client SDK 生成验证报告 |
