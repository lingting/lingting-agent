#!/usr/bin/env bash
# 构建：编译 workspace 全部成员。额外参数透传给 cargo build（如 --release）。
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

cargo build "$@"
