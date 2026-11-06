#!/usr/bin/env bash
# 生成：由服务端 API 定义生成 Rust Client SDK 与 TypeScript SDK。
# 生成物不入库（D-006 已修订）：
#   Rust  → packages/sdk-rust/src/（lib-bin-cli 用 #[path] 引入）
#   TS    → packages/sdk-ts/（pnpm workspace 消费）
# 生成命令的最终形态在 P1-007 固化（独立 crate 二进制 / cargo xtask）。
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

cargo run --quiet -p lib-web --example gen_rust_sdk
cargo run --quiet -p lib-web --example gen_ts_sdk
