//! 构建脚本：release 构建时把 WebUI 产物嵌入二进制（D-009）。
//!
//! 流程：
//! 1. 在前端工程执行 `pnpm build` 产出 `lingting-agent-ui/dist`；
//! 2. 文本资源 gzip 压缩、其余原样复制到 `OUT_DIR/webui`，保持相对路径；
//! 3. 生成资源表 `generated.rs` 写入 `OUT_DIR`，由 `src/webui/assets.rs` 引入。
//!
//! debug 构建不嵌入：资源在运行期从磁盘读取，前端改动不必重编 Rust，
//! 因此这里只写一个空文件，也不监视前端源码，避免改前端触发 Rust 重编。

use flate2::Compression;
use flate2::write::GzEncoder;
use std::env;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 前端工程目录名（仓库根目录下）。
const UI_DIR: &str = "lingting-agent-ui";
/// 前端构建产物目录名。
const DIST_DIR: &str = "dist";
/// 压缩产物在 `OUT_DIR` 下的子目录名，生成代码按相对路径引用。
const ASSET_DIR: &str = "webui";
/// 生成代码文件名（写入 `OUT_DIR`）。
const GENERATED_FILE: &str = "generated.rs";
/// 文本扩展名：命中则 gzip 压缩后嵌入，其余原样嵌入。
const TEXT_EXTENSIONS: &[&str] = &[
    "html",
    "htm",
    "css",
    "js",
    "mjs",
    "json",
    "map",
    "svg",
    "txt",
    "xml",
    "webmanifest",
];

/// 不嵌入资源时写入的内容：只留一行说明，保证 `assets.rs` 的 `include!` 始终成立。
///
/// 说明用普通注释：本文件由 `include!` 引入到模块内部，模块内不允许 `//!`。
const NOT_EMBEDDED: &str = "// debug 构建不嵌入前端资源：运行期从磁盘读取 dist 目录。\n";

/// 生成代码头部。
const HEADER: &str = "\
// 由 lib-bin-web/build.rs 生成的内嵌 WebUI 资源表，请勿手动修改。

";

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let project_dir = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("无法推导仓库根目录")?
        .to_path_buf();
    let ui_dir = project_dir.join(UI_DIR);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    // 嵌入与否以 cfg 交给代码：自定义 profile 下 PROFILE 与 debug_assertions 可能不一致，
    // 只有同一份判定才能保证生成物与代码分支对得上。
    let embed = should_embed();
    println!("cargo::rustc-check-cfg=cfg(webui_embed)");
    if embed {
        println!("cargo::rustc-cfg=webui_embed");
    }
    watch(&manifest_dir, &ui_dir, embed);

    if !embed {
        fs::write(out_dir.join(GENERATED_FILE), NOT_EMBEDDED)?;

        return Ok(());
    }

    build_frontend(&ui_dir)?;
    let resources = prepare(&ui_dir.join(DIST_DIR), &out_dir.join(ASSET_DIR))?;
    fs::write(out_dir.join(GENERATED_FILE), generated(&resources))?;

    Ok(())
}

/// 是否嵌入资源：只有 release 构建才嵌入并驱动前端构建。
fn should_embed() -> bool {
    env::var("PROFILE").is_ok_and(|profile| profile == "release")
}

/// 声明重跑条件。
///
/// 一旦声明 `rerun-if-changed`，Cargo 就不再默认监视整个包，包内文件必须一并列出。
/// 前端源码只在嵌入构建时监视：非嵌入构建不嵌入资源，改前端不该触发 Rust 重编。
fn watch(manifest_dir: &Path, ui_dir: &Path, embed: bool) {
    for path in [
        manifest_dir.join("src"),
        manifest_dir.join("build.rs"),
        manifest_dir.join("Cargo.toml"),
    ] {
        watch_path(&path);
    }
    println!("cargo:rerun-if-env-changed=PROFILE");

    if !embed {
        return;
    }

    for path in [
        ui_dir.join("src"),
        ui_dir.join("public"),
        ui_dir.join("index.html"),
        ui_dir.join("package.json"),
        ui_dir.join("pnpm-lock.yaml"),
        ui_dir.join("pnpm-workspace.yaml"),
        ui_dir.join("vite.config.ts"),
        ui_dir.join("tsconfig.json"),
    ] {
        watch_path(&path);
    }
}

/// 监视存在的路径；不存在的路径只会带来无意义重跑，直接跳过。
fn watch_path(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

/// 在前端工程执行 `pnpm build`。
fn build_frontend(ui_dir: &Path) -> Result<(), Box<dyn Error>> {
    if !ui_dir.is_dir() {
        return Err(format!("未找到前端工程目录：{}", ui_dir.display()).into());
    }

    // Windows 下 pnpm 只有 `.cmd` 与无扩展名脚本，`Command` 不会补全扩展名，需要交给 cmd 执行。
    let status = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", "pnpm build"])
            .current_dir(ui_dir)
            .status()
    } else {
        Command::new("pnpm").args(["build"]).current_dir(ui_dir).status()
    }
    .map_err(|error| format!("执行 pnpm build 失败（先执行 make ui-install 安装前端依赖）：{error}"))?;
    if !status.success() {
        return Err(format!("前端构建失败，退出码：{status}").into());
    }

    Ok(())
}

/// 压缩或复制全部构建产物，返回 (相对路径, 是否 gzip)。
fn prepare(dist_dir: &Path, output_dir: &Path) -> Result<Vec<(String, bool)>, Box<dyn Error>> {
    if !dist_dir.is_dir() {
        return Err(format!(
            "未找到前端构建产物：{}；请确认前端工程可正常执行 pnpm build",
            dist_dir.display()
        )
        .into());
    }

    let mut files = Vec::new();
    collect_files(dist_dir, "", &mut files)?;
    if files.is_empty() {
        return Err(format!("前端构建产物为空：{}", dist_dir.display()).into());
    }

    // 前端重新构建后文件名可能变化，先清空输出目录，避免残留旧资源。
    if output_dir.exists() {
        fs::remove_dir_all(output_dir)?;
    }
    fs::create_dir_all(output_dir)?;

    let mut resources = Vec::with_capacity(files.len());
    for (relative, source) in files {
        let bytes = fs::read(&source)?;
        let compressed = is_text(&relative);
        let target = output_dir.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        if compressed {
            fs::write(&target, gzip(&bytes)?)?;
        } else {
            fs::write(&target, &bytes)?;
        }
        resources.push((relative, compressed));
    }

    Ok(resources)
}

/// 收集目录下的全部文件，`prefix` 为相对 `dist` 的路径。
fn collect_files(dir: &Path, prefix: &str, files: &mut Vec<(String, PathBuf)>) -> Result<(), Box<dyn Error>> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.file_type()?.is_dir() {
            collect_files(&entry.path(), &relative, files)?;
        } else {
            files.push((relative, entry.path()));
        }
    }

    Ok(())
}

/// gzip 压缩；不写入修改时间，保证同一输入产出相同结果。
fn gzip(bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes)?;
    encoder.finish()
}

/// 生成资源表源码。
fn generated(resources: &[(String, bool)]) -> String {
    let mut code = String::from(HEADER);
    code.push_str(&format!(
        "/// 内嵌资源数量。\npub(super) const COUNT: usize = {};\n\n",
        resources.len()
    ));
    code.push_str("/// 按相对路径查找内嵌资源：返回（响应字节, 是否 gzip 压缩）。\n");
    code.push_str("pub(super) fn lookup(relative: &str) -> Option<(&'static [u8], bool)> {\n");
    code.push_str("    match relative {\n");
    for (relative, compressed) in resources {
        code.push_str(&format!(
            "        \"{relative}\" => Some((include_bytes!(\"{ASSET_DIR}/{relative}\"), {compressed})),\n"
        ));
    }
    code.push_str("        _ => None,\n    }\n}\n");

    code
}

/// 是否为按文本压缩的扩展名。
fn is_text(relative: &str) -> bool {
    extension(relative).is_some_and(|extension| TEXT_EXTENSIONS.contains(&extension))
}

/// 取扩展名（不含 `.`）。
fn extension(relative: &str) -> Option<&str> {
    relative.rsplit_once('.').map(|(_, extension)| extension)
}
