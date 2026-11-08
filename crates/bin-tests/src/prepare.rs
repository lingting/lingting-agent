//! 用例前置准备：按选中用例声明的要求，在启动服务端之前一次性准备好环境。
//!
//! 只处理**运行期**准备（如构建 WebUI 产物）。SDK 生成不在这里：`lib-bin-cli` 用 `#[path]`
//! 引入生成物，缺生成物时 `bin-tests` 自己编译不出来，所以生成必须发生在构建之前，留在
//! `Makefile` 的 `test` 前置里（见 `.plan/p1-016-webui-tests.md`「既定约束」）。

use crate::case::{Case, Prep};
use crate::frontend;
use anyhow::Result;

/// 按选中用例的要求依次执行准备动作，同类准备只做一次。
pub async fn ensure(cases: &[Case]) -> Result<()> {
    let mut needed: Vec<Prep> = cases.iter().flat_map(|case| case.prep.iter().copied()).collect();
    needed.sort();
    needed.dedup();

    for prep in needed {
        match prep {
            // 具体动作自带「构建 / 跳过」日志，这里不再重复打印。
            Prep::WebUiBuild => frontend::build().await?,
        }
    }
    Ok(())
}
