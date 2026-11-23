//! 运行观测报告：把内存里的快照渲染成测试报告（P2.1-002 / P2.2-005 / P2.2-006）。
//!
//! 本模块只做渲染，不认识日志、也不读文件。数字只有两个来源：
//!
//! - **本测试进程**：`manager-agent` 的 holder 装配，直接读内存（[`crate::util::support`]）；
//! - **服务端子进程**：用例自己的服务端，经 `GET api/probe/metrics` 取一份结构化快照
//!   （[`crate::util::server`]）。
//!
//! 拿不到快照就写「未采集：原因」——由用例与报告显式呈现，不猜、不翻日志（P2.2-005）。
//! 指标表的 Markdown 只在本模块生成一次，README 与用例章节共用。

use lib_core::time::now_millis;
use manager_agent::runtime::metrics::MetricsSnapshot;

use crate::util::support;

/// 一次快照采集的结果：`Ok` 是数字，`Err` 是「未采集」的原因。
pub type Capture = Result<MetricsSnapshot, String>;

/// 快照来源：本测试进程的装配。
pub const SOURCE_LOCAL: &str = "本进程（holder 装配）";
/// 快照来源：用例自己的服务端。
pub const SOURCE_OWN: &str = "子进程（用例自己的服务端）";
/// 快照来源：多个用例共用的服务端，数字是累计值。
pub const SOURCE_SHARED: &str = "子进程（共享服务端，累计值）";

/// 渲染 README 的「观测数据」一节：本测试进程的累计值；`started_at_ms` 用于折算占墙钟的比例。
///
/// 服务端子进程的快照写在各用例章节的「运行数据」里，不混进本节。
pub(crate) fn section(started_at_ms: i64) -> String {
    let wall = (now_millis() - started_at_ms).max(1) as u64;
    let mut text = String::new();
    text.push_str("## 观测数据\n\n");
    text.push_str(
        "> 采集需要 `metrics` 特性（`make test` 的 `bin-agent` 与 `bin-tests` 都启用）。\n\
         > 本节是**本测试进程**的累计值；服务端子进程的快照写在各用例章节的「运行数据」里。\n\n",
    );
    text.push_str(&blocking_section(wall));
    text.push_str(&format!("### 运行时指标：{SOURCE_LOCAL}\n\n"));
    text.push_str(&snapshot_table(&support::local_metrics()));
    text.push_str(
        "> 「并发水位」大于 1 表示多个客户端在并行清理（P2.1-005 之前是等同一把全局锁）；\
         「会话锁表」在会话停止 / 占用释放后回收（P2.1-007），不再随历史会话数增长；\
         「会话锁持有」只覆盖临界区，收尾在锁外（P2.1-006）。\n\n",
    );
    text
}

/// 渲染单个用例的「运行数据」一节：服务端快照 + 用例自报的度量。
///
/// 缺失快照时写「未采集：原因」，不留空、不跳过。
pub(crate) fn case_section(source: &str, snapshot: &Capture, measures: &[(String, String)]) -> String {
    let mut text = String::from("### 运行数据\n\n");
    text.push_str("> 数字全部来自内存：服务端快照由 `GET api/probe/metrics` 采集，用例度量由用例自报；不读日志。\n\n");
    text.push_str(&format!("#### {source}\n\n"));
    match snapshot {
        Ok(metrics) => text.push_str(&snapshot_table(metrics)),
        Err(reason) => {
            text.push_str("未采集：");
            text.push_str(reason);
            text.push_str("\n\n");
        }
    }
    text.push_str("#### 用例度量\n\n");
    if measures.is_empty() {
        text.push_str("（无）\n");
    } else {
        text.push_str("| 项 | 值 |\n|---|---|\n");
        for (key, value) in measures {
            text.push_str(&format!("| {key} | {value} |\n"));
        }
    }
    text
}

/// TUI 等待一节（P2.1-002 / 004）：同步阻塞与异步等待分开，前者才是 worker 占用。
fn blocking_section(wall: u64) -> String {
    let blocking = lib_bin_tui::testing::blocking_snapshot();
    let average = blocking.wait_millis.checked_div(blocking.wait_calls).unwrap_or(0);
    let share = blocking.total_millis * 100 / wall;
    let mut text = String::from("### TUI 等待与阻塞（P2.1-002 / 004）\n\n");
    text.push_str("| 项 | 次数 | 累计耗时 | 说明 |\n|---|---|---|---|\n");
    text.push_str(&format!(
        "| 同步轮询等待 | {} | {}ms | 平均每次 {}ms，`Harness::wait_until` 的同步轮询 |\n",
        blocking.wait_calls, blocking.wait_millis, average
    ));
    text.push_str(&format!(
        "| 同步收尾等待 | {} | {}ms | `Drop` 里 join 工作线程 |\n",
        blocking.shutdown_calls, blocking.shutdown_millis
    ));
    text.push_str(&format!(
        "| 同步合计 | — | {}ms | 占墙钟 {}s 的 {share}%，这部分才占住 worker |\n",
        blocking.total_millis,
        wall / 1000
    ));
    text.push_str(&format!(
        "| 异步轮询等待 | {} | {}ms | `Harness::wait_until_async`，不占 worker（P2.1-004） |\n",
        blocking.async_wait_calls, blocking.async_wait_millis
    ));
    text.push_str(&format!(
        "| 异步收尾等待 | {} | {}ms | `Harness::close`，join 在阻塞线程池里（P2.1-004） |\n\n",
        blocking.async_shutdown_calls, blocking.async_shutdown_millis
    ));
    text.push_str(
        "> 同步合计只在「所有 worker 同时被占住」时才会变成故障；单次运行量级远低于此即说明\
         同步等待不是超时原因。P2.1-004 之后 `tui.*` 用例的等待全部记在异步两行。\n\n",
    );
    text
}

/// 指标表：本进程与服务端子进程共用这一段渲染逻辑（P2.2-006 的唯一渲染点）。
fn snapshot_table(metrics: &MetricsSnapshot) -> String {
    let mut text = String::from("| 指标 | 值 |\n|---|---|\n");
    text.push_str(&format!(
        "| 断连清理链 | {} 次，最长 {}ms，并发水位 {}，清理客户端 {} |\n",
        metrics.chain.runs, metrics.chain.max_millis, metrics.chain.concurrent_max, metrics.chain.clients
    ));
    text.push_str(&format!(
        "| 会话停止 | {} 次，最长 {}ms（库 {}ms｜执行队列 {}ms｜写者 {}ms） |\n",
        metrics.stop.calls,
        metrics.stop.max_millis,
        metrics.stop.database_millis,
        metrics.stop.executor_millis,
        metrics.stop.writer_millis
    ));
    text.push_str(&format!(
        "| 事件落盘 | {} 帧，最长 {}ms |\n",
        metrics.journal.appends, metrics.journal.max_millis
    ));
    text.push_str(&format!("| 下行通道丢弃 | {} 次 |\n", metrics.downlink_dropped));
    text.push_str(&format!(
        "| 会话锁表 | {} 条，观察到上限 {} 条 |\n",
        metrics.session_locks.entries, metrics.session_locks.entries_max
    ));
    text.push_str(&format!(
        "| 会话锁持有 | {} 次，最长 {}ms（累计 {}ms） |\n",
        metrics.session_lock_hold.calls, metrics.session_lock_hold.max_millis, metrics.session_lock_hold.total_millis
    ));
    text.push_str(&format!(
        "| 取会话锁等待 | {} 次，最长 {}ms（累计 {}ms） |\n",
        metrics.session_lock_wait.calls, metrics.session_lock_wait.max_millis, metrics.session_lock_wait.total_millis
    ));
    text.push_str(&format!(
        "| 注册表整表锁 | 读 {} 次（最长等 {}ms）｜写 {} 次（最长等 {}ms） |\n",
        metrics.registry_lock.reads,
        metrics.registry_lock.read_wait_max_millis,
        metrics.registry_lock.writes,
        metrics.registry_lock.write_wait_max_millis
    ));
    text.push_str(&format!(
        "| 下行转发 | {} 帧，单帧最长 {}ms |\n",
        metrics.registry_lock.forwards, metrics.registry_lock.forward_max_millis
    ));
    text.push_str(&format!(
        "| 取数据库连接 | {} 次，最长 {}ms，写锁争用 {} 次 |\n",
        metrics.database.calls, metrics.database.max_millis, metrics.database.errors
    ));
    text.push_str(&format!(
        "| Run 排队 | {} 次，同时在排队峰值 {}，排队超时 {}，取消 {}，最长等 {}ms |\n",
        metrics.limit.queued,
        metrics.limit.queued_max,
        metrics.limit.timeouts,
        metrics.limit.cancelled,
        metrics.limit.wait_max_millis
    ));
    text.push_str(&format!(
        "| Run 执行 | 同时执行峰值 {}，读取瞬间 {} 个在跑 |\n",
        metrics.limit.running_max, metrics.limit.running_now
    ));
    text.push_str(&format!(
        "| 日志审计 | 事件 {}（ERROR {}｜WARN {}），敏感串命中 {}，panic {} |\n\n",
        metrics.audit.total, metrics.audit.error, metrics.audit.warn, metrics.audit.secret_hits, metrics.audit.panics
    ));
    text
}
