//! `agent server --async`：派生一个常驻服务进程，探活通过后返回（P2.2-012）。
//!
//! 派生方式：重新执行当前可执行文件，命令行由已解析的 [`ServerConfig`] 反向构造（**不带** `--async`），
//! `AGENT_HOME` 等环境原样继承；`stdin` 置空，`stdout` / `stderr` 追加写入
//! `<AGENT_HOME>/logs/stdout.log`。父进程不等待、也不 kill 子进程：子进程句柄用完即弃，
//! 全程不启用任何 kill-on-drop 语义。
//!
//! 就绪期间的输出转发：CLI 不立即返回，而是在轮询探活接口的同时，把子进程输出文件里**本次新增**的
//! 内容实时转发到自己的 `stderr`——启动失败的原因就在那几行里，用户不必再去翻文件；探活通过后
//! 打印 PID、服务地址、日志目录与 PID 文件并退出 `0`。转发写 `stderr` 而不是 `stdout`，
//! 免得污染那几行可被脚本解析的接入信息。
//!
//! 句柄隔离：派生前把本进程的标准句柄置为**不可继承**（Windows 走 kernel32 的
//! `SetHandleInformation`，见 [`hide_standard_handles`]）。否则常驻进程会继承 CLI 的输出管道写端，
//! `agent server -a | grep …` 与 `$(agent server -a …)` 都会挂到服务端退出（KI-P2.2-001）。
//!
//! 平台差异（D-P2.2-005）：Windows 用 `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` 创建，
//! 父 shell 结束后子进程照样常驻；Unix 只做 stdio 重定向，终端会话关闭时子进程可能收到 SIGHUP
//! （本阶段不为此引入 `libc` / `setsid`，需要更强语义时用 `nohup` 或系统服务）。
//!
//! 停服不提供子命令（D-P2.2-004）：靠 PID 文件或系统工具回收。

use crate::client::Client;
use crate::config::ServerConfig;
use crate::result::CommandError;
use crate::runtime;
use crate::sdk::ApiClient;

use anyhow::{Context, Result, bail};
use lib_core::directorys::directorys;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// 服务端输出文件名：派生进程的 `stdout` / `stderr` 都追加到这里。
pub const STDOUT_FILE: &str = "stdout.log";
/// PID 文件名，位于应用根目录（与 `config.toml` 同级）。
///
/// 内容是 `pid = <进程号>` 与 `address = <服务地址>` 两行：前者供人工与用例收尾，后者供核对。
pub const PID_FILE: &str = "server.pid";
/// 就绪等待上限：与测试侧启动服务端的等待同量级。
const READY_TIMEOUT: Duration = Duration::from_secs(20);
/// 就绪探测与输出转发的间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 派生子进程：就绪后返回，失败路径只有一行原因。
///
/// 退出码口径沿用 [`CommandError`]：配置与参数错误 `2`、运行期错误 `1`。
pub fn spawn_async(config: &ServerConfig) -> Result<(), CommandError> {
    launch(config).map_err(CommandError::runtime)
}

/// 派生、转发启动输出、等就绪、写 PID 文件并打印接入信息。
fn launch(config: &ServerConfig) -> Result<()> {
    let directories = directorys().context("读取应用目录失败")?;
    let root = directories
        .config_file
        .parent()
        .context("无法从配置文件推导应用根目录")?;
    let output_path = directories.logs_server.join(STDOUT_FILE);
    let pid_path = root.join(PID_FILE);
    let base_url = format!("http://{}:{}", config.host, config.port);

    // 派生之前先确认端口可用：探活只回答「这个地址上有没有健康的服务」，回答不了
    // 「它是不是我刚派生的那个」——端口已被占用时子进程会因 bind 失败退出，而父进程可能
    // 先探到了别人的服务并误判成功（实测过：第二次 `--async` 撞端口仍退出 0）。
    ensure_port_free(&format!("{}:{}", config.host, config.port))?;

    // 输出文件是追加写：只转发本次派生之后新增的内容，不重复上一次运行的输出。
    let forwarded_from = std::fs::metadata(&output_path)
        .map(|metadata| metadata.len())
        .unwrap_or_default();
    let mut child = spawn(config, &output_path)?;
    let pid = child.id();
    let mut forwarder = Forwarder::new(output_path.clone(), forwarded_from);

    if let Err(error) = wait_ready(&mut child, &base_url, &mut forwarder) {
        // 失败前写出的输出也转发一份：启动失败的原因通常在最后几行。
        forwarder.pump();
        return Err(error.context(format!(
            "派生的服务端未就绪（PID {pid}，输出文件：{}）",
            output_path.display()
        )));
    }
    forwarder.pump();
    write_pid_file(&pid_path, pid, &base_url)?;

    println!("PID：{pid}");
    println!("服务地址：{base_url}");
    println!("日志目录：{}", directories.logs_server.display());
    println!("PID 文件：{}", pid_path.display());
    Ok(())
}

/// 派生服务端进程：参数由配置反向构造，输出落文件，stdin 置空。
fn spawn(config: &ServerConfig, output: &Path) -> Result<Child> {
    let binary = std::env::current_exe().context("获取当前可执行文件路径失败")?;
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(output)
        .with_context(|| format!("创建服务端输出文件失败：{}", output.display()))?;
    let stderr = stdout.try_clone().context("复制服务端输出句柄失败")?;

    let mut command = Command::new(&binary);
    command
        .arg("server")
        .arg("--addr")
        .arg(&config.host)
        .arg("--port")
        .arg(config.port.to_string())
        .arg("--log-level")
        .arg(config.log_level.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    // 先切断本进程标准句柄的继承，再让子进程与当前会话脱钩（两件事互不相关，顺序无关）。
    hide_standard_handles();
    detach(&mut command);

    command
        .spawn()
        .with_context(|| format!("派生服务端进程失败：{}", binary.display()))
}

/// 让子进程与父进程的会话脱钩。
#[cfg(windows)]
fn detach(command: &mut Command) {
    /// 与父进程的控制台脱钩：父 shell 关闭不会带走子进程。
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    /// 独立进程组：父进程收到的 Ctrl+C 不会转发给子进程。
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    use std::os::windows::process::CommandExt;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

/// Unix 上只做 stdio 重定向，不做 `setsid`（D-P2.2-005）。
#[cfg(not(windows))]
fn detach(_command: &mut Command) {}

/// 把本进程的标准句柄置为**不可继承**：派生的常驻服务端不该握住 CLI 的 `stdout` / `stderr`。
///
/// Windows 的 `CreateProcess` 默认把父进程里所有可继承句柄一并传给子进程，而 `std` 没有稳定接口
/// 关掉它（`CommandExt::inherit_handles` 仍是 unstable）。不处理的话，CLI 退出后那个常驻进程仍然
/// 持有 CLI 的输出管道写端，`agent server -a | grep …`、`$(agent server -a …)` 会一直挂到服务端退出
/// （KI-P2.2-001）。这里直接调 kernel32：取出三个标准句柄，清掉「可继承」这一位。
///
/// 句柄本身不受影响——调用后本进程照常读写 `stdout` / `stderr`，只是不再传给子进程。
#[cfg(windows)]
fn hide_standard_handles() {
    use std::ffi::c_void;

    /// `GetStdHandle` 的句柄编号：标准输入 / 输出 / 错误（`-10` / `-11` / `-12` 的无符号形式）。
    const STD_INPUT_HANDLE: u32 = 0xFFFF_FFF6;
    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;
    const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4;
    /// `SetHandleInformation` 的掩码与取值：只操作「可继承」这一位。
    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;

    unsafe extern "system" {
        /// 读取本进程的标准句柄；无控制台时返回空句柄或 `INVALID_HANDLE_VALUE`。
        fn GetStdHandle(std_handle: u32) -> *mut c_void;
        /// 修改句柄标志位；只改可继承位，不动句柄的使用权。
        fn SetHandleInformation(object: *mut c_void, mask: u32, flags: u32) -> i32;
    }

    for std_handle in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: 两个调用都只针对本进程的标准句柄，不解引用指针、不转移所有权，也不修改
        // 句柄指向的对象；无效句柄（空值或 INVALID_HANDLE_VALUE）先被跳过，失败只清不掉标志位。
        unsafe {
            let handle = GetStdHandle(std_handle);
            if handle.is_null() || handle as isize == -1 {
                continue;
            }
            let _ = SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
        }
    }
}

/// Unix 上子进程继承的是文件描述符副本，且 CLI 的管道写端不会跨进程保活，无需额外动作。
#[cfg(not(windows))]
fn hide_standard_handles() {}

/// 端口预检：派生之前确认这个地址能绑定。
///
/// 绑上就立刻放开，真正的监听仍由子进程完成；这里只保证「立刻要 bind 的那个端口没人占」，
/// 免得探活探到端口上已有的别的服务（见 [`launch`] 的注释）。
fn ensure_port_free(address: &str) -> Result<()> {
    match std::net::TcpListener::bind(address) {
        Ok(listener) => {
            drop(listener);
            Ok(())
        }
        Err(error) => bail!("{address} 端口不可用（{error}）：换一个 --port，或先结束占用它的进程"),
    }
}

/// 就绪期间的输出转发：把子进程输出文件里本次新增的内容写到本进程的 `stderr`。
///
/// 子进程输出始终落文件（常驻期间需要它），这里只是让「还没返回的这段时间」的输出也能被看见：
/// 启动失败的原因就在文件末尾。写 `stderr` 是为了不污染 `stdout` 上的接入信息。
struct Forwarder {
    /// 子进程输出文件。
    path: PathBuf,
    /// 已经转发到的字节位置。
    offset: u64,
}

impl Forwarder {
    /// 从 `offset` 开始转发；追加写的文件里 `offset` 之前的内容属于上一次运行。
    fn new(path: PathBuf, offset: u64) -> Self {
        Self { path, offset }
    }

    /// 转发新增内容：读失败只跳过这一轮，真正的原因由就绪失败给出。
    fn pump(&mut self) {
        let Ok(metadata) = std::fs::metadata(&self.path) else {
            return;
        };
        if metadata.len() <= self.offset {
            return;
        }
        let Ok(mut file) = File::open(&self.path) else {
            return;
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut buffer = Vec::new();
        let Ok(read) = file.read_to_end(&mut buffer) else {
            return;
        };
        self.offset += read as u64;
        if buffer.is_empty() {
            return;
        }
        // 转发是尽力而为：下游管道已经关闭时不应该让父进程失败。
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(&buffer);
        let _ = stderr.flush();
    }
}

/// 轮询探活接口直到服务端可访问，同时把启动输出转发出去；子进程提前退出或超时都直接给出原因。
fn wait_ready(child: &mut Child, base_url: &str, forwarder: &mut Forwarder) -> Result<()> {
    let client = Client::new(base_url).context("创建探活客户端失败")?;
    runtime::block_on(async {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            forwarder.pump();
            if let Some(status) = child.try_wait().context("读取服务端进程状态失败")? {
                bail!("服务端进程启动后立即退出（{status}）");
            }
            if client.probe_health().await.is_ok() {
                // 探活成功也可能是别人的服务：认这次就绪之前再确认子进程还活着。
                if let Some(status) = child.try_wait().context("读取服务端进程状态失败")? {
                    bail!("服务端进程启动后立即退出（{status}）");
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("服务端在 {} 秒内未就绪", READY_TIMEOUT.as_secs());
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    })?
}

/// 写 PID 文件：探活成功之后才写，避免记录一个未就绪的进程。
fn write_pid_file(path: &Path, pid: u32, base_url: &str) -> Result<()> {
    std::fs::write(path, format!("pid = {pid}\naddress = {base_url}\n"))
        .with_context(|| format!("写入 PID 文件失败：{}", path.display()))
}
