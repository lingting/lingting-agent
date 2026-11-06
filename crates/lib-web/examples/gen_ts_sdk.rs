//! 由服务端 API 元数据生成 TypeScript SDK（P1-007 固化）。
//!
//! 运行：`make gen`（内部执行 `cargo run -p lib-web --example gen_ts_sdk`）。
//!
//! 生成物写入 `packages/sdk-ts/`（npm ESM 包：`package.json` + `dist/*.js|*.d.ts`），
//! WebUI 通过根目录的 pnpm workspace 以 `workspace:*` 引入。
//! 生成物只有抽象类 `ApiClient`，`call` 的实际 HTTP Transport 由 WebUI 自己实现（D-002）。

use std::path::Path;

use framework_proc_core::{
    PackageBuilder, TypescriptApiBuilder, TypescriptEnumBuilder, TypescriptTypeBuilder, api_metadata_iter,
    enum_metadata_iter, type_metadata_iter,
};

/// 生成器共用的暂存目录与整体替换逻辑（P1-007）。
#[path = "support/sdk_out.rs"]
mod sdk_out;

/// 生成的 npm 包名。
const PACKAGE_NAME: &str = "@lingting/agent-api";
/// 生成的 npm 包版本，与服务端 workspace 版本保持一致。
const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// 生成物输出目录（相对 workspace 根）。
const OUT_DIR: &str = "packages/sdk-ts";
/// 生成的客户端抽象类名。
const CLASS_NAME: &str = "ApiClient";

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 链接锚点：必须引用 lib-web 的符号，否则 `inventory` 注册项会被链接器丢弃，元数据为空。
    let routes = lib_web::web_routes();
    let api_count = api_metadata_iter().count();
    if api_count == 0 {
        return Err(format!(
            "未收集到 API 元数据（路由 {} 条）：请确认 lib-web 开启了 collect",
            routes.len()
        )
        .into());
    }

    let out_dir = Path::new(OUT_DIR);
    // 先写暂存目录：全部生成成功后再整体替换目标目录，失败时保留上一次的有效产物。
    let staging = sdk_out::prepare_staging(out_dir)?;
    let mut package = PackageBuilder::new(PACKAGE_NAME, PACKAGE_VERSION, &staging);
    package.push(
        "types",
        TypescriptTypeBuilder::new()
            .types(type_metadata_iter())
            .type_import_from("."),
    );
    package.push(
        "enums",
        TypescriptEnumBuilder::new()
            .enums(enum_metadata_iter())
            .type_import_from("."),
    );
    package.push(
        "api",
        TypescriptApiBuilder::new()
            .apis(api_metadata_iter())
            .class_name(CLASS_NAME)
            .type_import_from("."),
    );
    package.write()?;
    sdk_out::commit_staging(&staging, out_dir)?;
    println!("路由 {} 条，接口 {api_count} 条，生成完成 → {OUT_DIR}", routes.len());
    Ok(())
}
