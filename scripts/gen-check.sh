#!/bin/sh
# 生成物重建门禁（P1-019）：从零重建两份 SDK，确认「生成成功 + 产物完整 + 可编译」。
#
# 由 `make gen-check` 调用；本地与 CI 走同一条命令（CI 只调 `make ci`）。
# 生成物不入库（D-006），因此这里不做 git diff 比对，判据只有三条：
#   1. `make gen` 以 0 退出；
#   2. 关键产物存在且非空 —— 生成器「静默少产出」同样算失败；
#   3. 生成物参与编译后 `cargo check` 通过（前端静态类型检查由 `make ui-check` 负责）。
#
# 提示文案只写在这里，不写进 Makefile recipe：make 在 Windows 上按 ANSI 代码页解析 Makefile，
# recipe 里的中文会变成乱码（见 docs/develop.md「开发命令」）。

set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

GEN_DIR=packages

# 关键产物：Rust 侧被 `lib-bin-cli` 用 #[path] 引入，TS 侧被前端以 workspace 依赖消费。
KEY_ARTIFACTS='
sdk-rust/src/api.rs
sdk-rust/src/enums.rs
sdk-rust/src/types.rs
sdk-ts/package.json
sdk-ts/dist/index.js
sdk-ts/dist/index.d.ts
sdk-ts/dist/api.js
sdk-ts/dist/api.d.ts
sdk-ts/dist/types.d.ts
sdk-ts/dist/enums.d.ts
'

GEN_HINT='缺少生成物或生成失败，请运行 `make gen` 重新生成。'

fail() {
	printf '\n[gen-check] %s\n' "$1" >&2
	printf '[gen-check] %s\n' "${2:-$GEN_HINT}" >&2
	exit 1
}

printf '[gen-check] 1/4 清空 %s/（从零重建，覆盖“缺生成物”的场景）\n' "$GEN_DIR"
rm -rf "$GEN_DIR"

printf '[gen-check] 2/4 重新生成两份 SDK\n'
make gen || fail '生成失败：`make gen` 以非 0 退出。' "$GEN_HINT"

printf '[gen-check] 3/4 校验产物完整性\n'
missing=''
for artifact in $KEY_ARTIFACTS; do
	[ -s "$GEN_DIR/$artifact" ] || missing="$missing $artifact"
done
[ -z "$missing" ] || fail "产物缺失或为空：$missing" "$GEN_HINT"

printf '[gen-check] 4/4 校验生成物可编译\n'
cargo check --workspace --all-targets ||
	fail '编译失败：生成物参与编译后 cargo check 未通过。' \
		'按上面的编译错误修 API 定义或生成器，不要手改 packages/ 下的生成物；修好后重跑 `make gen`。'

printf '[gen-check] 通过：两份 SDK 可从零重建，产物完整且可编译。\n'
