# P2.1 已知问题

本文件记录从 P2 阶段带出来的、需要在本阶段处置的问题。每条包含现象、复现、判断、影响与处置方向；状态只用「待处置 / 处置中 / 已结案」。

---

## K-1 `make test ARGS="--all"` 默认并发下共享分片超时

- **状态**：待处置（P2 Batch E 验收时发现）
- **发现时间**：2026-11-21，P2 Batch E（P2-019 / P2-020）验收
- **现象**：默认并发跑全量用例时，`shard-shared` 里少数长耗时用例报「未在 N 秒内收到回复」「下行流未收到 `run_completed`」，耗时恰好压在各自的内部等待上限（20s / 44.5s），用例判定失败。
- **默认并发度**：`crates/bin-tests/src/cli.rs` 的 `default_concurrency()` 取 `available_parallelism()`，本机为 18，即 18 条用例同时打同一个共享服务端（`shard-shared`），另有 isolated 用例各自起子进程。
- **复现与观测**：

| 命令 | 结果 |
|---|---|
| `make test ARGS="--all"`（第 1 次） | 57 条中 2 失败：`tui.reconnect`（20.0s）、`session.message`（44.5s） |
| `make test ARGS="--all"`（第 2 次） | 57 条中 2 失败：`session.message`（44.5s）、`tui.esc_cancel`（20.0s） |
| `make test ARGS="session tui"`（并发 18） | 18 条中 3 失败：`tui.ctrl_d_cancel`、`tui.reconnect`、`session.message` |
| `make test ARGS="session tui --concurrency 4"` | 18 条全通过（`session.message` 2.7s、`tui.reconnect` 15.1s） |
| `make test ARGS="session.message"` 单跑 | 通过（2.7s） |
| `make test ARGS="tui.reconnect"` 单跑 | 通过（15.1s，距 20s 上限仅 5s 余量） |
| `make test ARGS="--all --concurrency 4"` | **57 条全通过**（含 `web.ui` 28 项、`web.e2e` 3 条） |

- **判断**：与任何功能代码改动无关——两次 `--all` 的失败集合不同、同一用例单跑即过、降并发即全绿，失败集中在共享服务端上的长耗时用例，属**并发度导致的等待超时**，不是断言或业务行为错误。P2 Batch E 的新增 crate（`agent-context-core` / `agent-context`）未进入 `bin-agent` 依赖图，也不在失败链路上。
- **余量问题**：`tui.reconnect` 单跑 15.1s，本身已贴近 20s 上限，并发一高必然先崩；`session.message` 的 44.5s 同样是等满上限后失败，说明这两条用例的等待预算对「共享服务端 + 高并发」没有余量。
- **影响**：全量验收只能用低并发跑；CI 或他人本地用默认并发跑 `--all` 会看到与改动无关的红灯，容易误判为回归。
- **处置方向（本阶段定稿，未实施）**：
  1. 给 `bin-tests` 的默认并发加机器相关上限（例如 `min(核数, 4~8)`），或让共享分片单独限流；
  2. 或把这几条长耗时用例的内部等待上限从 20s / 45s 上调，并在用例名或报告里标注「依赖共享服务端」；
  3. 或在报告里显式记录本次并发度与「高并发下的已知抖动」，让红灯可归因。
  - 三条互不冲突，倾向第 1 条 + 第 3 条：限流治本，记录治标。
- **验收口径**：本项结案时需给出「默认并发下 `make test ARGS="--all"` 稳定全绿」的证据（连续多次运行），或明确改为「验收固定用低并发 + 报告标注」，二者择一写死。
