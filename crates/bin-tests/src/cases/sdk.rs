//! 生成器用例：非法元数据必须报错，合法输入必须幂等（R-4）。
//!
//! 直接调用 `framework-proc-core` 的导出构建器，不经过 `make gen` 的文件系统路径：这与生成器
//! 内部用的是同一段校验逻辑，而写盘行为（内容一致不刷新 mtime）由 `make gen` 的日志体现。

use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::ensure;
use framework_proc_core::{
    ApiMetadata, ApiReturnType, RustApiBuilder, RustBuilder, TypescriptApiBuilder, TypescriptBuilder,
};

/// SDK 生成领域用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![Case {
        name: "sdk.generator",
        kind: CaseKind::Shared,
        prep: &[],
        run: generator,
    }]
}

/// 合法接口：唯一的 method + path。
const ALPHA: ApiMetadata = ApiMetadata {
    name: "alpha",
    namespace: "test",
    method: "GET",
    path: "api/test/alpha",
    parameters: &[],
    return_type: ApiReturnType::void(),
};

/// 与 [`ALPHA`] 同名的接口（不同 namespace / method / path）。
const ALPHA_DUPLICATE_NAME: ApiMetadata = ApiMetadata {
    name: "alpha",
    namespace: "test::other",
    method: "POST",
    path: "api/test/beta",
    parameters: &[],
    return_type: ApiReturnType::void(),
};

/// 与 [`ALPHA`] 重复 method + path 的接口（不同名）。
const ALPHA_DUPLICATE_ROUTE: ApiMetadata = ApiMetadata {
    name: "beta",
    namespace: "test::other",
    method: "GET",
    path: "api/test/alpha",
    parameters: &[],
    return_type: ApiReturnType::void(),
};

fn generator(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();

        // 重名接口：两份 SDK 都要拒绝，并指出是哪两个定义。
        let rust_name_error = rust_error([ALPHA, ALPHA_DUPLICATE_NAME]);
        ensure!(
            rust_name_error.contains("方法名重复") && rust_name_error.contains("alpha"),
            "Rust 生成器未拒绝重名接口：{rust_name_error}"
        );
        let ts_name_error = ts_error([ALPHA, ALPHA_DUPLICATE_NAME]);
        ensure!(
            ts_name_error.contains("方法名重复") && ts_name_error.contains("alpha"),
            "TS 生成器未拒绝重名接口：{ts_name_error}"
        );
        notes.push(format!("重名接口：{rust_name_error}"));

        // 重复 method + path：同样要拒绝，并指出冲突的路由。
        let rust_route_error = rust_error([ALPHA, ALPHA_DUPLICATE_ROUTE]);
        ensure!(
            rust_route_error.contains("请求方法和地址重复") && rust_route_error.contains("GET api/test/alpha"),
            "Rust 生成器未拒绝重复路由：{rust_route_error}"
        );
        let ts_route_error = ts_error([ALPHA, ALPHA_DUPLICATE_ROUTE]);
        ensure!(
            ts_route_error.contains("请求方法和地址重复") && ts_route_error.contains("GET api/test/alpha"),
            "TS 生成器未拒绝重复路由：{ts_route_error}"
        );
        notes.push(format!("重复路由：{rust_route_error}"));

        // 幂等：同一份元数据连续构建两次，产物必须逐字节相同。
        let rust = RustApiBuilder::new().apis([ALPHA]).trait_name("ApiClient");
        let rust_first = rust
            .build()
            .map_err(|error| anyhow::anyhow!("Rust 生成失败：{error}"))?;
        let rust_second = rust
            .build()
            .map_err(|error| anyhow::anyhow!("Rust 生成失败：{error}"))?;
        ensure!(rust_first.code == rust_second.code, "Rust 生成器不幂等");

        let ts = TypescriptApiBuilder::new().apis([ALPHA]).class_name("ApiClient");
        let ts_first = ts.build().map_err(|error| anyhow::anyhow!("TS 生成失败：{error}"))?;
        let ts_second = ts.build().map_err(|error| anyhow::anyhow!("TS 生成失败：{error}"))?;
        ensure!(ts_first.js == ts_second.js, "TS 生成器不幂等（js）");
        ensure!(ts_first.dts == ts_second.dts, "TS 生成器不幂等（d.ts）");
        notes.push(format!(
            "合法输入幂等：Rust {} 字节，TS js {} 字节 / d.ts {} 字节",
            rust_first.code.len(),
            ts_first.js.len(),
            ts_first.dts.len()
        ));

        Ok(notes)
    })
}

/// 用 Rust 生成器构建一组元数据，返回错误消息；成功则返回生成的代码。
fn rust_error<const N: usize>(metadata: [ApiMetadata; N]) -> String {
    match RustApiBuilder::new().apis(metadata).trait_name("ApiClient").build() {
        Ok(result) => format!("（未报错，产出 {} 字节）", result.code.len()),
        Err(error) => error.to_string(),
    }
}

/// 用 TS 生成器构建一组元数据，返回错误消息；成功则返回生成的声明。
fn ts_error<const N: usize>(metadata: [ApiMetadata; N]) -> String {
    match TypescriptApiBuilder::new()
        .apis(metadata)
        .class_name("ApiClient")
        .build()
    {
        Ok(result) => format!("（未报错，产出 {} 字节声明）", result.dts.len()),
        Err(error) => error.to_string(),
    }
}
