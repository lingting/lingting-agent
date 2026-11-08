# lingting-agent 开发命令入口。
#
# 约定：
#   1. recipe 使用 POSIX sh，需在 Git Bash 中执行 make。
#   2. check 只做格式与静态检查；需要执行测试用 test（会调用测试入口）。

# 前端工程目录（同时是 pnpm workspace 根）。
UI_DIR := lingting-agent-ui
# 生成物根目录（不入库，见 .steps/p1/decisions.md D-006）。
GEN_DIR := packages
# 透传参数，例如 `make build ARGS="--release"`。
ARGS ?=

.DEFAULT_GOAL := help
.PHONY: help gen build check fmt test clean-gen ui-install ui-dev ui-build ui-lint ui-e2e-install

help:
	@cat .make/help.txt

# 生成：两条命令顺序执行，任一失败即中止（make 遇错退出，非 0）。
# 生成物由 API 定义产出，不手改；详见 docs/develop.md「API 契约规范」。
gen:
	cargo run --quiet -p lib-web --example gen_rust_sdk
	cargo run --quiet -p lib-web --example gen_ts_sdk

build:
	cargo build $(ARGS)

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings

fmt:
	cargo fmt --all

# 测试：ARGS 必填，传用例名前缀或 --all，例如 make test ARGS="--all"。
# 本目标只是入口：`gen` 必须在构建之前完成（`lib-bin-cli` 用 `#[path]` 引入生成物，缺生成物
# 连 bin-tests 都编译不出来），其余运行期准备（如 ui-build）由 bin-tests 按选中用例自行执行。
test: gen
	cargo build --quiet -p bin-agent
	cargo run --quiet -p bin-tests -- $(ARGS)

clean-gen:
	rm -rf $(GEN_DIR)

ui-install:
	cd $(UI_DIR) && pnpm install

ui-dev:
	cd $(UI_DIR) && pnpm dev

ui-build:
	cd $(UI_DIR) && pnpm build

# 端到端用例需要 Playwright 的浏览器二进制；只装 chromium，避免下载全部浏览器。
ui-e2e-install:
	cd $(UI_DIR) && pnpm exec playwright install chromium

ui-lint:
	cd $(UI_DIR) && pnpm run lint
