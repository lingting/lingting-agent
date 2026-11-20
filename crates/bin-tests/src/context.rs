//! 用例上下文：服务基地址、生成的 SDK 客户端、服务端应用目录与用例内唯一标记。

use anyhow::{Context, Result};
use lib_bin_cli::client::Client;
use lib_core::time;
use std::path::{Path, PathBuf};

/// 用例执行时拿到的上下文。
pub struct TestContext {
    name: &'static str,
    base_url: String,
    client: Client,
    home: PathBuf,
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
        })
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

    /// 用例内唯一标记：用例名 + 当前毫秒，用于隔离消息内容等业务数据。
    pub fn token(&self) -> String {
        format!("{}-{}", self.name, time::now_millis())
    }
}
