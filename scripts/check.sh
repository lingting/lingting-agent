#!/usr/bin/env bash
# 检查：格式检查 + Clippy 静态检查。不执行任何测试。
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
