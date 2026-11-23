//! 用例上下文：服务基地址、生成的 SDK 客户端、服务端应用目录、用例内唯一标记，
//! 以及报告用的度量条目与服务端快照（P2.2-005 / P2.2-006）。

use anyhow::{Context, Result};
use lib_bin_cli::client::Client;
use lib_core::time;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::util::observability::Capture;

/// 用例执行时拿到的上下文。
pub struct TestContext {
    name: &'static str,
    base_url: String,
    client: Client,
    home: PathBuf,
    /// 用例自报的度量条目：报告「运行数据」一节的数据来源之一（P2.2-006）。
    measures: Mutex<Vec<(String, String)>>,
    /// 用例在服务端停止前自己取到的快照（P2.2-005）。
    ///
    /// 自管用例的服务端在用例返回前就已停掉（`kill_on_drop` 或用例自己 `stop`），入口此时
    /// 再取只能得到「未采集」，因此采集点必须留在服务端还活着的地方。
    observed: Mutex<Option<Capture>>,
}

impl TestContext {
    /// 由服务基地址与服务端应用目录构建上下文。
    ///
    /// `home` 是服务端进程的 `AGENT_HOME`：数据目录（含事件文件）与日志都落在它下面，
    /// 用例据此断言落盘结果。
    pub fn new(name: &'static str, base_url: String, home: PathBuf) -> Result<Self> {
        let client = Client::new(base_url.clone()).context("创建客户端失败")?;
        Ok(Self {
            name,
            base_url,
            client,
            home,
            measures: Mutex::new(Vec::new()),
            observed: Mutex::new(None),
        })
    }

    /// 记一条用例度量（如压力用例的 `T` / `L0` / `N`）：只进报告，不进日志。
    pub fn measure(&self, key: impl Into<String>, value: impl Into<String>) {
        let mut measures = self.measures.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        measures.push((key.into(), value.into()));
    }

    /// 取出用例记录过的度量条目（保持记录顺序）。
    pub fn measures(&self) -> Vec<(String, String)> {
        self.measures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 记录一份在服务端仍存活时取到的快照：入口在用例返回后取不到时会退回这一份。
    pub fn observe_metrics(&self, capture: Capture) {
        *self.observed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(capture);
    }

    /// 取出已记录的快照；没有记录时为 `None`（入口会再直接取一次）。
    pub fn observed_metrics(&self) -> Option<Capture> {
        self.observed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 服务基地址，写入用例报告。
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 生成的 SDK 客户端。
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// 服务端的应用根目录（子进程的 `AGENT_HOME`）。
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// 服务端监听端口：从基地址解析，自管用例重启时复用同一端口。
    pub fn port(&self) -> Result<u16> {
        self.base_url
            .rsplit(':')
            .next()
            .and_then(|port| port.parse::<u16>().ok())
            .with_context(|| format!("无法从基地址解析端口：{}", self.base_url))
    }

    /// 用例内唯一标记：用例名 + 当前毫秒，用于隔离消息内容等业务数据。
    pub fn token(&self) -> String {
        format!("{}-{}", self.name, time::now_millis())
    }
}
