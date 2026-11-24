# P2.2 已知问题

本文件记录 P2.2 落地过程中发现的、需要单独决策处置的问题。每条包含现象、复现、判断、影响与处置方向；
状态只用「待处置 / 处置中 / 已结案」。

---

## KI-P2.2-001 Windows 上派生的服务端会继承 CLI 的可继承句柄

- **状态**：已结案（2026-11-24，Batch D 验收后按需求调整修掉根因：Windows 分支引入 kernel32 FFI 把自身标准句柄置为非继承，见 D-P2.2-015；用例改回管道读取并加读取时限作为回归哨兵）
- **发现时间**：2026-11-23，P2.2 Batch D 验收
- **现象**：`agent server --async` 派生的常驻服务端会继承父进程（CLI）的**可继承句柄**，包括 CLI 自己的
  `stdout` / `stderr` 写端。CLI 退出后这些句柄仍被服务端持有，于是「读 CLI 输出的那一端」等不到 EOF。
- **复现**：

  ```bash
  AGENT_HOME=$(mktemp -d /tmp/x) ./target/debug/bin-agent.exe server --async --port 20204 | cat
  # PID / 地址 / 日志目录都打印了，但 cat 不返回：管道写端仍被常驻服务端持有
  timeout 20 sh -c '... | cat'   # → 退出码 124（超时）
  ```

  同一现象在 `bin-tests` 的 `cli.async_server` 上表现为**用例 60 秒超时**：用例先 `Stdio::piped()` 起 CLI，
  CLI 退出后读管道等 EOF，而写端被派生的服务端持有，于是直到用例调度超时才结束；随后 `make ... | tail`
  也因为同一个句柄拿不到 EOF 而挂住（实测一次 `make test ARGS="cli"` 卡了 20 分钟以上才被外部超时终止）。
- **判断**：Windows 上 `std::process::Command` 默认 `bInheritHandles = TRUE`，凡父进程里可继承的句柄都会
  传给子进程；把子进程的 `stdin` / `stdout` / `stderr` 重定向到 `null` / 文件并不阻止**其它**可继承句柄
  （例如父进程自己的 `stdout` 管道）一并被继承。控制该行为的 `std::os::windows::process::CommandExt::inherit_handles`
  目前仍是 unstable（`windows_process_extensions_inherit_handles`），稳定版没有等价开关。
- **影响**：
- **影响（修复前）**：
  1. 用户侧：`agent server --async | grep PID` 这类**管道场景**会挂住，直到服务端退出；重定向到文件不受影响
     （`> run.log`），因为文件没有 EOF 语义。脚本里用命令替换（`$(agent server --async …)`）同样会挂住。
  2. 测试侧：`cli.async_server` 曾被拖到 60 秒用例超时，随后 `make ... | tail` 也因为同一个句柄拿不到 EOF
     而挂住（实测一次 `make test ARGS="cli"` 卡了 20 分钟以上才被外部超时终止）。
  3. 不影响服务端自身运行、PID 文件、探活与收尾。
- **处置（已落地，D-P2.2-015）**：
  1. `lib-bin-cli::server` 的 Windows 分支新增 `hide_standard_handles()`：经 kernel32 的
     `GetStdHandle` + `SetHandleInformation(HANDLE_FLAG_INHERIT, 0)` 把本进程的标准句柄置为非继承，再派生。
     仅两个系统调用，不新增第三方依赖；`unsafe` 块只针对自身标准句柄，附 SAFETY 说明。
  2. 派生后 CLI 不再立即返回：就绪期间把子进程输出文件的新增内容转发到自身 `stderr`（见 D-P2.2-015 第 1 条），
     启动失败的原因直接可见。
  3. `cli.async_server` 改回**管道读取**并加 5 秒读取时限：句柄隔离一旦被去掉，用例会以
     「CLI 的 stdout 管道在 5s 内没有到 EOF」失败，成为这条修复的回归哨兵。
- **复核证据（2026-11-24）**：
  - `agent server -a --port 20210 2>&1 | cat` **立即返回**（`pipeline_exit=0`），输出含转发的启动日志与四行接入信息；
  - 仅 stdout 接管道时只得到四行接入信息（stderr 不入管道）；
  - `make test ARGS="cli"` 全绿（3.7s）；`make test ARGS="--all"` 75 条全绿。

---

## KI-P2.2-002 `stress.boundary` 在 `--all` 并发下偶发破「② 总耗时」上界

- **状态**：待处置（P2.2-016 回归基线前需定口径；与 Batch D 改动无关）
- **发现时间**：2026-11-24，P2.2 Batch D 验收复跑 `--all` 时
- **现象**：`make test ARGS="--all"` 中 `stress.boundary` 判红，失败原因：

  ```text
  阶段 ② 内边界：期望最后一个终态不晚于 T + L0 = 3054ms（T = 3000ms、L0 = 54ms），
  实际 3106ms（超出 51ms）；快照 limit：queued 59、queued_max 54、running_max 1、
  wait_max 2913ms、timeouts 0、cancelled 0
  ```

- **判断**：门槛本身没被击穿——`timeouts == 0`、`wait_max 2913ms < T = 3000ms`、`running_max == 1`，
  ② 的 `N = 55` 全部完成；超出的只是**末个 Run 收尾段**：`L0` 在用例独占服务端时标定（55ms），
  而 `--all` 默认并发 18 跑时 CPU 被别的用例抢占，末个 Run 自己跑的那一段被拉长，于是破 `T + L0` 上界。
  这与 D-P2.2-009「不许对 `N` 放水」无关：`N` 未变，破的是 D-P2.2-013 引入的耗时上界。
- **证据**：

| 命令 | 结果 |
|---|---|
| `make test ARGS="--all"`（第 1 次） | 75 条中 1 红（上表；报告 `target/tests/run-1795482594828-3412`） |
| `make test ARGS="--all"`（第 2 次） | 75 条全绿 |
| `make test ARGS="stress"` 连续 3 次 | 全绿（9.5s / 9.4s / 9.4s） |

- **影响**：`--all` 回归会偶发红灯，容易被当成功能回归；单跑与低并发稳定。
- **处置方向（待决策）**：
  1. 把 `assert_inner` 的上界改成 `T + L0 + 收尾余量`，余量按「并发下末段被拉长的量级」定并写进
     `decisions.md`（`N` 不动，仍符合 D-P2.2-009 的口径）；
  2. 让 `self` 型用例独占运行（调度层串行化），代价是墙钟变长；
  3. 不改，接受偶发红并在阶段报告里注明「单跑为准」。
