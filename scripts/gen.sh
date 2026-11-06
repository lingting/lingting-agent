#!/usr/bin/env bash
# 生成：由服务端 API 定义生成 TypeScript SDK 与 Rust Client SDK。
# 生成器形态（独立 crate 二进制 / cargo xtask / 脚本）在 P1-007 固化。
# 本阶段仅占位：明确失败，避免被误当成“已生成”。
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

echo "SDK 生成尚未接入：生成器形态在 P1-007 固化（见 .steps/p1/tasks.md）" >&2
exit 1
