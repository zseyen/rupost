# RuPost 本地开发门禁（DX 批，规格见 doc/plans/2026-10-27-dev-workflow-dx-prd.md §4.1）
#
# 用法：`make help` 查看目标清单。流程规范见 doc/dev/development_workflow.md；
# 进度事实源：doc/plans/progress_summary.md（本文件不记录任何进度）。
#
# 非目标（PRD 裁决）：不引 just/nextest；不做 jj hook 强制；CI 三 job 结构不动——
# 本文件仅服务本地 DX，Windows CI 路径不依赖它。

.PHONY: help gate gate-fast gate-all examples features prd build coverage

help: ## 目标清单（注释即文档）
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  \033[36mmake %-12s\033[0m %s\n", $$1, $$2}'

gate: ## 完整门禁：fmt --check → clippy -D warnings → cargo test（fail-fast）
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings
	cargo test --workspace

gate-fast: ## 编辑中间态快速自检：仅 fmt + clippy
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings

gate-all: ## gate + 两个 L3 脚本（功能特性 + examples 外网冒烟；发版前必跑）
	$(MAKE) gate
	./tests/verify_features.sh
	./examples/run_all.sh

examples: ## 单独跑 examples 全量冒烟（tests/verify_features.sh 的外网部分）
	./examples/run_all.sh

features: ## 单独跑功能特性校验脚本
	./tests/verify_features.sh

prd: ## 新建 PRD：make prd NAME=<名> → doc/plans/<今日>-<名>.md（拒绝覆盖）
	@if [ -z "$(NAME)" ]; then \
		echo "用法: make prd NAME=<名>"; exit 1; fi
	@out="doc/plans/$$(date +%F)-$(NAME).md"; \
	if [ -e "$$out" ]; then \
		echo "已存在: $$out（拒绝覆盖；续写请直接编辑）"; exit 1; fi; \
	sed "s/\$$(date)/$$(date +%F)/" doc/templates/prd-template.md > "$$out" && \
	echo "已创建: $$out"

build: ## cargo build（自动规避 CommandLineTools SDK 的 .tbd 损坏，见注释）
	@# SDKROOT 坑升格为代码：CommandLineTools 自带的 SDK 缺 .tbd 会导致链接失败
	@# （progress_summary 2026-10 记录的环境问题）。检测到默认 SDK 位于
	@# CommandLineTools 时，自动改用完整 Xcode 的 MacOSX SDK；其余环境原样。
	@if [ "$$(uname)" = "Darwin" ]; then \
		defsdk=$$(xcrun --show-sdk-path 2>/dev/null); \
		case "$$defsdk" in \
			*CommandLineTools*) \
				xcodesdk=$$(xcode-select -p 2>/dev/null)/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk; \
				if [ -d "$$xcodesdk" ]; then \
					echo "==> 默认 SDK 位于 CommandLineTools（.tbd 损坏），改用: $$xcodesdk"; \
					SDKROOT="$$xcodesdk" cargo build; \
				else \
					echo "==> 检测到 CommandLineTools SDK 且未安装完整 Xcode，按原样构建"; \
					cargo build; \
				fi ;; \
			*) cargo build ;; \
		esac; \
	else \
		cargo build; \
	fi

coverage: ## 覆盖率汇总（cargo-llvm-cov，本地可选；维持不进 CI 门禁的裁决）
	@if command -v cargo-llvm-cov >/dev/null 2>&1; then \
		cargo llvm-cov --workspace --summary-only; \
	else \
		echo "cargo-llvm-cov 未安装。一键安装:"; \
		echo "  cargo install cargo-llvm-cov"; \
		echo "详见 doc/guides/testing_guide.md §4 覆盖率"; \
		exit 1; \
	fi
