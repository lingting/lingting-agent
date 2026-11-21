//! 运行观测报告：把进程内指标渲染成测试报告里的一节（P2.1-002 / 003）。
//!
//! 口径必须分开看：
//! - TUI 同步阻塞发生在**本测试进程**：`async` 上下文里的同步等待会占住一个 Tokio worker；
//! - 运行时指标只覆盖**本进程内装配**的运行时（`manager-agent` 的 holder）。共享 / 隔离用例的
//!   服务端是子进程，它们的事件落盘、清理链与锁表在子进程里，只能从各自的 `server.log`
//!   按阈值行汇总，不在本节数字内。

use lib_core::time::now_millis;

/// 渲染「观测数据」一节；`started_at_ms` 用于折算占墙钟的比例。
pub(crate) fn section(started_at_ms: i64) -> String {
    let wall = (now_millis() - started_at_ms).max(1) as u64;
    let mut text = String::new();
    text.push_str("## 观测数据\n\n");
    text.push_str(
        "> 采集需要 `metrics` 特性（`make test` 的 `bin-agent` 与 `bin-tests` 都启用）。\n\
         > 本节是**本测试进程**的累计值：服务端子进程的指标在各自的 `server.log` 里，按阈值行汇总。\n\n",
    );
    text.push_str(&blocking_section(wall));
    text.push_str(&runtime_section());
    text
}

/// TUI 同步阻塞一节（P2.1-002）。
fn blocking_section(wall: u64) -> String {
    let blocking = lib_bin_tui::testing::blocking_snapshot();
    let average = blocking.wait_millis.checked_div(blocking.wait_calls).unwrap_or(0);
    let share = blocking.total_millis * 100 / wall;
    let mut text = String::from("### TUI 同步阻塞（P2.1-002）\n\n");
    text.push_str("| 项 | 次数 | 累计耗时 | 说明 |\n|---|---|---|---|\n");
    text.push_str(&format!(
        "| 轮询等待 | {} | {}ms | 平均每次 {}ms，`Harness::wait_until` 的同步轮询 |\n",
        blocking.wait_calls, blocking.wait_millis, average
    ));
    text.push_str(&format!(
        "| 收尾等待 | {} | {}ms | `Worker::shutdown` 里 join 工作线程 |\n",
        blocking.shutdown_calls, blocking.shutdown_millis
    ));
    text.push_str(&format!(
        "| 合计 | — | {}ms | 占墙钟 {}s 的 {share}%，摊到 worker 数后更小 |\n\n",
        blocking.total_millis,
        wall / 1000
    ));
    text.push_str(
        "> 这个数字只在「所有 worker 同时被占住」时才会变成故障；单次运行量级远低于此即说明\
         同步等待不是超时原因。\n\n",
    );
    text
}

/// 运行时指标一节（P2.1-003）。
fn runtime_section() -> String {
    let metrics = manager_agent::runtime::metrics::snapshot();
    let mut text = String::from("### 运行时指标（P2.1-003）\n\n");
    text.push_str("| 指标 | 值 |\n|---|---|\n");
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
        "| 取数据库连接 | {} 次，最长 {}ms |\n\n",
        metrics.database.calls, metrics.database.max_millis
    ));
    text.push_str(
        "> 「并发水位」大于 1 表示有任务在等 `CHAIN` 锁；「会话锁表」随历史会话数增长，\
         回收见 P2.1-007。\n",
    );
    text
}
