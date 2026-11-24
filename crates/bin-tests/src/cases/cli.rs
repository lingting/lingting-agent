//! CLI 命令面用例：`cli.async_server`（P2.2-013）。
//!
//! 断言三件事：`agent server --async` 在就绪后退出 `0` 并打印 PID / 服务地址 / 日志目录；
//! 派生的服务进程常驻且 `api/probe/health` 可用；PID 文件与打印内容一致。
//! 数据只来自命令行输出、PID 文件与探活接口——不读任何日志文本（G-3-1）。
//!
//! 用例自管服务端（`CaseKind::SelfServed`）：入口只给用例目录与空闲端口，启停时机由用例决定——
//! 子进程不是入口派生的，入口也不知道它的 PID。收尾按 PID 文件结束它（D-P2.2-004 没有 `--stop`）。
//!
//! CLI 的输出用管道读，并给读取加了时限：CLI 退出后写端必须全部关闭。派生的服务端若继承了 CLI 的
//! 可继承句柄（KI-P2.2-001），管道就永远等不到 EOF——这条用例因此也是「句柄隔离」的回归哨兵：
//! 一旦 `hide_standard_handles` 被去掉，这里会以「管道在 N 内没有到 EOF」直接失败，而不是挂到用例超时。

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt};

use anyhow::{Context, Result, bail, ensure};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::ApiClient;
use lib_bin_cli::server::PID_FILE;
use lib_core::directorys::{CONFIG_FILE, ENV_HOME};

use tokio::process::Command;

use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;
use crate::util::{paths, process, server, stress};

/// `agent server --async` 自身返回的等待上限：就绪轮询上限 20 秒，再留余量。
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);
/// 结束派生子进程后的等待上限：`taskkill` / `kill -9` 生效需要一点时间。
const KILL_TIMEOUT: Duration = Duration::from_secs(10);
/// 读完 CLI 输出的上限：CLI 已退出，管道应当立刻到 EOF；等不到就是句柄被继承出去了。
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// 父进程退出与探活的轮询间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// CLI 命令面用例：只有一条，且它自己派生了服务端进程。
pub(crate) fn cases() -> Vec<Case> {
    vec![Case {
        name: "cli.async_server",
        kind: CaseKind::SelfServed,
        prep: &[],
        run: async_server,
    }]
}

/// `cli.async_server`：派生 → 父进程退出 → 探活 → 按 PID 收尾。
fn async_server(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mock = server::shared_mock().await?;
        let home = context.home().to_path_buf();
        std::fs::create_dir_all(&home).with_context(|| format!("创建用例目录失败：{}", home.display()))?;
        // 服务端照常从 `AGENT_HOME/config.toml` 读配置：Provider 指向共享 Mock，不新增任何配置字段。
        std::fs::write(home.join(CONFIG_FILE), server::config(mock.base_url(), ""))
            .with_context(|| format!("写入用例配置失败：{}", home.display()))?;

        let port = context.port()?;
        let expected = format!("http://127.0.0.1:{port}");
        let binary = server::server_binary()?;

        // 参数组合矩阵（G-2-1）：`--async` 与 `--addr` / `--port` / `--log-level` 一起给。
        let mut child = Command::new(&binary)
            .arg("server")
            .arg("--async")
            .arg("--addr")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--log-level")
            .arg("warn")
            .env(ENV_HOME, &home)
            // 服务端按工作目录解析静态资源，与 `util::server` 一致固定到仓库根目录。
            .current_dir(paths::repo_root()?)
            .stdin(Stdio::null())
            // 管道读取：CLI 退出后写端必须全部关闭（若派生的服务端继承了它，读会等不到 EOF，
            // 由 `READ_TIMEOUT` 兜住并给出明确原因——见文件头的说明）。
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("启动 `agent server --async` 失败：{}", binary.display()))?;
        let launcher = child.id().context("读取 CLI 进程 PID 失败")?;

        // 父进程必须自己退出：`try_wait` 命中即证明它没有挂在那里等子进程（G-2-3）。
        let waiting = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().context("读取 CLI 进程状态失败")? {
                break status;
            }
            if waiting.elapsed() >= LAUNCH_TIMEOUT {
                // 超时也要收尾：PID 文件可能已经写下（子进程已派生），按它结束进程再报错，
                // 否则会把一个占着端口的遗留进程留在后面（P2.2-010）。
                if let Some(pid) = recorded_pid(&home) {
                    let _ = process::kill(pid);
                }
                bail!(
                    "`agent server --async` 在 {} 内没有返回（CLI 进程 PID {launcher}）",
                    stress::millis(LAUNCH_TIMEOUT)
                );
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        };
        let launched = waiting.elapsed();
        let stdout = read_pipe("stdout", child.stdout.take()).await?;
        let stderr = read_pipe("stderr", child.stderr.take()).await?;
        ensure!(
            status.success(),
            "`agent server --async` 应退出 0，实际 {status}；标准输出：{stdout}；标准错误：{stderr}"
        );
        context.measure(
            "父进程",
            format!("PID {launcher} 在 {} 内退出（退出码 0）", stress::millis(launched)),
        );

        // PID 文件是子进程的唯一句柄：断言与收尾都以它为准。
        let pid_path = home.join(PID_FILE);
        ensure!(
            pid_path.is_file(),
            "PID 文件不存在：{}（`--async` 应在探活通过后写入）",
            pid_path.display()
        );
        let recorded =
            std::fs::read_to_string(&pid_path).with_context(|| format!("读取 PID 文件失败：{}", pid_path.display()))?;
        let address = parse_value(&recorded, "address")?;
        let pid = parse_value(&recorded, "pid")?
            .parse::<u32>()
            .with_context(|| format!("PID 文件里的 pid 不是数字：{recorded}"))?;
        // 兜底清理：用例中途失败时 `Drop` 也会结束它。
        let spawned = Spawned { pid };

        ensure!(
            stdout.contains(&pid.to_string()),
            "命令行输出应含 PID {pid}；实际输出：{stdout}"
        );
        ensure!(
            stdout.contains(&expected),
            "命令行输出应含服务地址 {expected}；实际输出：{stdout}"
        );
        ensure!(
            stdout.contains("日志目录"),
            "命令行输出应含日志目录；实际输出：{stdout}"
        );
        ensure!(
            address == expected,
            "PID 文件里的地址应为 {expected}，实际 {address}；文件内容：{recorded}"
        );

        let health = tokio::time::timeout(LAUNCH_TIMEOUT, context.client().probe_health())
            .await
            .context("探活超时：`--async` 返回后 `api/probe/health` 应立即可用")?
            .map_err(ClientError::from_call)?;
        context.measure(
            "探活",
            format!(
                "{expected} 正常（服务版本 {}，协议 {}，运行中 Run {}）",
                health.version, health.protocol_version, health.running_run_count
            ),
        );

        spawned.stop()?;
        context.measure(
            "收尾",
            format!("按 PID 文件结束子进程 {pid}，进程已消失（D-P2.2-004 没有 --stop）"),
        );

        Ok(vec![
            format!(
                "`agent server --async`：CLI 进程 PID {launcher} 在 {} 内退出（退出码 0），派生服务进程 PID {pid}",
                stress::millis(launched)
            ),
            format!("探活：{expected} 正常（服务版本 {}）", health.version),
            format!("PID 文件：{}（pid = {pid}、address = {address}）", pid_path.display()),
            format!("收尾：结束派生子进程 {pid} 并确认进程消失，无遗留 `bin-agent` 进程"),
            "数据来源：命令行输出 + PID 文件 + 探活接口，不读日志".to_string(),
        ])
    })
}

/// 派生的服务端子进程：`Drop` 兜底结束它，[`Spawned::stop`] 负责断言它真的没了。
struct Spawned {
    /// 服务端进程号，来自 PID 文件。
    pid: u32,
}

impl Spawned {
    /// 结束子进程并确认进程消失：用例结束前必须走到这里。
    fn stop(self) -> Result<()> {
        process::kill(self.pid)?;
        process::wait_gone(self.pid, KILL_TIMEOUT)
    }
}

impl Drop for Spawned {
    fn drop(&mut self) {
        // 用例在中途失败时也要收尾：这里只发终止请求，断言留给 `stop`。
        let _ = process::kill(self.pid);
    }
}

/// 读完一条 CLI 输出管道。
///
/// CLI 已经退出，写端应当立刻全部关闭；等不到 EOF 说明有别的进程还握着这个管道（通常是派生的
/// 服务端继承了 CLI 的可继承句柄，KI-P2.2-001）。这里给一个明确的时限，让回归原因是「句柄被继承」
/// 而不是整条用例超时。
async fn read_pipe(name: &str, pipe: Option<impl AsyncRead + Unpin>) -> Result<String> {
    let Some(mut pipe) = pipe else {
        return Ok(String::new());
    };
    let mut bytes = Vec::new();
    tokio::time::timeout(READ_TIMEOUT, pipe.read_to_end(&mut bytes))
        .await
        .with_context(|| {
            format!(
                "CLI 的 {name} 管道在 {} 内没有到 EOF：派生的服务端仍握着 CLI 的句柄（KI-P2.2-001）",
                stress::millis(READ_TIMEOUT)
            )
        })?
        .context("读取 CLI 输出失败")?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// 读 PID 文件里的进程号：用于「CLI 还没返回、子进程已经派生」时的兜底收尾。
fn recorded_pid(home: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(home.join(PID_FILE)).ok()?;
    parse_value(&text, "pid").ok()?.parse().ok()
}

/// 从 PID 文件里取一行 `key = value` 的值。
fn parse_value(text: &str, key: &str) -> Result<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(&format!("{key} = ")))
        .map(|value| value.trim().to_owned())
        .with_context(|| format!("PID 文件里没有 `{key} = …` 一行：{text}"))
}
