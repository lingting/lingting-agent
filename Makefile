# lingting-agent 开发命令入口。
#
# 约定：
#   1. recipe 使用 POSIX sh，需在 Git Bash 中执行 make。
#   2. 本文件不包含任何测试命令；check 只做格式与静态检查。

# 前端工程目录（同时是 pnpm workspace 根）。
UI_DIR := lingting-agent-ui
# 生成物根目录（不入库，见 .steps/p1/decisions.md D-006）。
GEN_DIR := packages
# 透传参数，例如 `make build ARGS="--release"`。
ARGS ?=

.DEFAULT_GOAL := help
.PHONY: help gen build check fmt clean-gen ui-install ui-dev ui-build ui-lint

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

clean-gen:
	rm -rf $(GEN_DIR)

ui-install:
	cd $(UI_DIR) && pnpm install

ui-dev:
	cd $(UI_DIR) && pnpm dev

ui-build:
	cd $(UI_DIR) && pnpm build

ui-lint:
	cd $(UI_DIR) && pnpm run lint
