//! 用例上下文：生成的 SDK 客户端与用例内唯一标记。

use anyhow::{Context, Result};
use lib_bin_cli::client::Client;
use lib_core::time;

/// 用例执行时拿到的上下文。
pub struct TestContext {
    name: &'static str,
    client: Client,
}

impl TestContext {
    /// 由服务基地址构建上下文。
    pub fn new(name: &'static str, base_url: String) -> Result<Self> {
        let client = Client::new(base_url).context("创建客户端失败")?;
        Ok(Self { name, client })
    }

    /// 生成的 SDK 客户端。
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// 用例内唯一标记：用例名 + 当前毫秒，用于隔离消息内容等业务数据。
    pub fn token(&self) -> String {
        format!("{}-{}", self.name, time::now_millis())
    }
}
