//! 由服务端 API 元数据生成 Rust / TS 两份客户端 SDK（`make gen` 的唯一入口）。
//!
//! 两个生成器在同一次进程里顺序执行：省掉一次 cargo 启动，并保证两份 SDK 基于同一份元数据快照。
//! 任一步失败即中止（非 0 退出）；失败方的写盘发生在暂存目录，上一次的有效产物保持不变。
//!
//! 产物与上次完全一致时不写盘（见 `support/sdk_out.rs`），避免刷新 mtime 让下游无谓重编译。

use std::error::Error;

mod support;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    support::gen_rust_sdk::generate()?;
    support::gen_ts_sdk::generate()
}
