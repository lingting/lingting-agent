//! 由服务端 API 元数据生成 TypeScript SDK（P1-007 固化）。
//!
//! 由 `examples/gen_sdk.rs` 在 `make gen` 中调用。
//!
//! 生成物写入 `packages/sdk-ts/`（npm ESM 包：`package.json` + `dist/*.js|*.d.ts`），
//! WebUI 通过根目录的 pnpm workspace 以 `workspace:*` 引入。
//! 生成物只有抽象类 `ApiClient`，`call` 的实际 HTTP Transport 由 WebUI 自己实现（D-002）。

use framework_proc_core::{
    PackageBuilder, TypescriptApiBuilder, TypescriptEnumBuilder, TypescriptTypeBuilder, api_metadata_iter,
    enum_metadata_iter, type_metadata_iter,
};
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

// 生成器共用的暂存目录与按内容提交逻辑（P1-007）：模块声明在 `examples/support/mod.rs`。
use super::sdk_out;
/// 生成的 npm 包名。
const PACKAGE_NAME: &str = "@lingting/agent-api";
/// 生成的 npm 包版本，与服务端 workspace 版本保持一致。
const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// 生成物输出目录（相对 workspace 根）。
const OUT_DIR: &str = "packages/sdk-ts";
/// 生成的客户端抽象类名。
const CLASS_NAME: &str = "ApiClient";
/// 生成物头部标识（G-7）：每个 `.js` / `.d.ts` 都要带，且必须与实际生成方式一致。
const HEADER: &str =
    "// 本文件由 `make gen` 自动生成，请勿手动修改。\n// 服务端 API 定义变更后重新执行生成命令；生成物不入库。\n\n";

pub fn generate() -> Result<(), Box<dyn Error + Send + Sync>> {
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
    // 先写暂存目录：全部生成成功后再提交，失败时保留上一次的有效产物。
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
    mark_generated(&staging)?;
    let commit = sdk_out::commit(&staging, out_dir)?;
    println!("路由 {} 条，接口 {api_count} 条", routes.len());
    println!("TS SDK → {OUT_DIR}");
    commit.report(out_dir);
    Ok(())
}

/// 给暂存目录下的全部 `.js` / `.d.ts` 补上“自动生成、勿手改”标识（G-7）。
///
/// `PackageBuilder` 只产出代码本身，不写头部注释；`types.d.ts` 自带的 Specta 英文标识不足以覆盖
/// 其余文件，这里统一在提交前补一行中文标识，提交后逐文件比对内容，因此重复生成不会刷新 mtime。
fn mark_generated(root: &Path) -> io::Result<()> {
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            mark_generated(&path)?;
        } else if is_generated_script(&path) {
            let content = fs::read_to_string(&path)?;
            if !content.starts_with(HEADER) {
                fs::write(&path, format!("{HEADER}{content}"))?;
            }
        }
    }
    Ok(())
}

/// 生成物里的脚本文件：`.js` 与类型声明 `.d.ts`。
fn is_generated_script(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".js") || name.ends_with(".d.ts"))
}
