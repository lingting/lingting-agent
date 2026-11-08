//! 由服务端 API 元数据生成 Rust Client SDK（P1-007 固化）。
//!
//! 由 `examples/gen_sdk.rs` 在 `make gen` 中调用。
//!
//! 生成物写入 `packages/sdk-rust/src/`（可用环境变量 `AGENT_SDK_RUST_OUT_DIR` 覆盖），包含
//! `types.rs`（DTO 定义快照）、`enums.rs`（枚举运行时信息）与 `api.rs`（接口定义 + `ApiClient` trait）。
//! 该目录不入库，也**不生成 Cargo.toml**：`lib-bin-cli` 用 `#[path]` 把这三个文件当作自己的模块引入，
//! 从而避免「生成物先于 workspace manifest 存在」的引导死锁。
//!
//! DTO 策略为“自带快照定义”：生成物自包含，不依赖服务端 crate，客户端不会 link 服务端代码。

use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use framework_proc_core::{
    RustApiBuilder, RustBuilder, RustEnumBuilder, RustTypeBuilder, api_metadata_iter, enum_metadata_iter,
    type_metadata_iter,
};

// 生成器共用的暂存目录与按内容提交逻辑（P1-007）：模块声明在 `examples/support/mod.rs`。
use super::sdk_out;
/// 生成物默认输出目录（相对 workspace 根）。
const DEFAULT_OUT_DIR: &str = "packages/sdk-rust/src";
/// 覆盖输出目录的环境变量。
const OUT_DIR_ENV: &str = "AGENT_SDK_RUST_OUT_DIR";
/// 生成的客户端 trait 名。
const TRAIT_NAME: &str = "ApiClient";
/// 接口模块引用 DTO 的模块路径。
const TYPES_PATH: &str = "crate::sdk::types";

pub fn generate() -> Result<(), Box<dyn Error + Send + Sync>> {
    // 链接锚点：必须引用 lib-web 的符号，否则 `inventory` 注册项会被链接器丢弃，元数据为空。
    let routes = lib_web::web_routes();
    let apis = api_metadata_iter().collect::<Vec<_>>();
    if apis.is_empty() {
        return Err(format!(
            "未收集到 API 元数据（路由 {} 条）：请确认 lib-web 开启了 collect",
            routes.len()
        )
        .into());
    }
    let out_dir = out_dir();
    println!("路由 {} 条，接口 {} 条", routes.len(), apis.len());
    for api in &apis {
        println!("  {:6} {:24} {}", api.method, api.path, api.name);
    }

    let types = RustTypeBuilder::new().types(type_metadata_iter()).build()?;
    let enums = RustEnumBuilder::new()
        .enums(enum_metadata_iter())
        .type_import_from(TYPES_PATH)
        .build()?;
    let api = RustApiBuilder::new()
        .apis(apis)
        .trait_name(TRAIT_NAME)
        .type_import_from(TYPES_PATH)
        .build()?;

    // 先写暂存目录：全部生成成功后再提交，失败时保留上一次的有效产物。
    let staging = sdk_out::prepare_staging(&out_dir)?;
    write_module(&staging, "types", &types.code)?;
    write_module(&staging, "api", &api.code)?;
    if !enums.code.trim().is_empty() {
        write_module(&staging, "enums", &enums.code)?;
    }
    format_modules(&staging)?;
    let commit = sdk_out::commit(&staging, &out_dir)?;
    println!("Rust SDK → {}", out_dir.display());
    commit.report(&out_dir);
    Ok(())
}

/// 解析输出目录：环境变量优先，其次默认值。
fn out_dir() -> PathBuf {
    env::var_os(OUT_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_OUT_DIR))
}

/// 生成文件头部：自动生成标记 + 关闭本文件全部 lint 告警。
///
/// `#![allow(warnings)]` 是模块级内层属性，必须写在任何 item 之前；它的优先级高于命令行的
/// `-D warnings`，因此生成代码不会成为 fmt / clippy / build 的告警来源（真正的编译错误仍会暴露）。
fn header() -> String {
    [
        "// 本文件由 `make gen` 自动生成，请勿手动修改。",
        "// 服务端 API 定义变更后重新执行生成命令；生成物不入库。",
        "",
        "#![allow(warnings)]",
        "",
        "",
    ]
    .join("\n")
}

fn write_module(out_dir: &Path, name: &str, code: &str) -> io::Result<()> {
    fs::write(out_dir.join(format!("{name}.rs")), format!("{}{code}", header()))
}

/// 用 rustfmt 规范化生成物：框架输出由 `stringify!` 快照拼接，且这些文件会被 `cargo fmt --check` 检查。
fn format_modules(out_dir: &Path) -> io::Result<()> {
    let files = fs::read_dir(out_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect::<Vec<_>>();
    let mut command = Command::new("rustfmt");
    command.arg("--edition").arg("2024").args(&files);
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => {
            println!("rustfmt 退出码 {status}：生成物保持框架原始格式");
            Ok(())
        }
        Err(error) => {
            println!("无法执行 rustfmt（{error}）：生成物保持框架原始格式");
            Ok(())
        }
    }
}
