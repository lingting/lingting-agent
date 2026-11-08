//! 生成的 Rust Client SDK（由 `cargo run -p lib-web --example gen_sdk` 写入 `packages/sdk-rust/src/`）。
//!
//! 生成物不入库，也不生成 Cargo.toml：这里用 `#[path]` 把生成文件当作本 crate 的模块引入，
//! 避免「path 依赖的 manifest 先于生成存在」的引导死锁。
//! 生成文件自带 `#![allow(warnings)]`，因此不会给 `clippy -D warnings` 制造噪声。
//! `enums` 不参与 `pub use *`：枚举运行时信息结构体与 `types` 里的强类型枚举同名（如 `RCodeKind`），
//! 一起再导出会触发 `ambiguous_glob_reexports`。

/// 接口定义、`ApiClient` trait 与调用错误（对应框架的 `RustApiBuilder` 输出）。
#[path = "../../../packages/sdk-rust/src/api.rs"]
pub mod api;

/// DTO 定义，来自服务端 `auto_type` 的类型快照（对应框架的 `RustTypeBuilder` 输出）。
#[path = "../../../packages/sdk-rust/src/types.rs"]
pub mod types;

/// 枚举运行时信息（对应框架的 `RustEnumBuilder` 输出）。
#[path = "../../../packages/sdk-rust/src/enums.rs"]
pub mod enums;

pub use api::*;
pub use types::*;
