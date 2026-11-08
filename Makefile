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
# 交叉编译目标平台（cargo-zigbuild 用 zig 做链接器，无需目标平台原生工具链，见 D-020）。
CROSS_TARGET ?= x86_64-unknown-linux-gnu
# 发布 binary 名称：Windows 带 .exe，其他平台无后缀。
EXE := $(if $(filter Windows_NT,$(OS)),.exe,)

.DEFAULT_GOAL := help
.PHONY: help gen build release release-cross check fmt test clean-gen ui-install ui-dev ui-build ui-lint ui-e2e-install

help:
	@cat .make/help.txt

# 生成：一次进程内顺序生成两份 SDK，任一失败即中止（make 遇错退出，非 0）。
# 生成物由 API 定义产出，不手改；详见 docs/develop.md「API 契约规范」。
# 产物与上次逐字节比对：完全一致时不写盘，避免刷新 mtime 让下游（cargo / 前端）无谓重编译。
gen:
	cargo run --quiet -p lib-web --example gen_sdk

build:
	cargo build $(ARGS)

# Release：一条命令产出可独立分发的完整 binary（P1-018）。
# 顺序：gen（SDK 生成，编译期必需）→ cargo build --release → lib-bin-web 的 build.rs 在 release 下
# 自动执行前端 `pnpm build` 并嵌入产物（D-009），因此不需要另调 ui-build。
release: gen
	cargo build --release -p bin-agent
	@echo "binary: target/release/bin-agent$(EXE)"

# 交叉编译（D-020）：Linux 目标用 cargo-zigbuild；Windows 目标走本机原生 `make release`，不用这条。
# 产物在 target/$(CROSS_TARGET)/release/，目标平台不可运行，验证需在对应平台或 CI 上做。
release-cross: gen
	rustup target add $(CROSS_TARGET)
	cargo zigbuild --release -p bin-agent --target $(CROSS_TARGET)
	@echo "binary: target/$(CROSS_TARGET)/release/bin-agent$(if $(findstring windows,$(CROSS_TARGET)),.exe,)"

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
