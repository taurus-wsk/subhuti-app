# ============================================================
# Subhuti Makefile - 统一开发入口
# ============================================================
# 用法: make [target]
# ============================================================

.PHONY: help build test test-watch serve serve-status serve-logs serve-stop serve-restart docker docker-build docker-stop fmt clippy check clean install release-test trace

# 默认目标
.DEFAULT_GOAL := help

# 颜色输出
GREEN  := \033[0;32m
YELLOW := \033[0;33m
BLUE   := \033[0;34m
NC     := \033[0m

# 帮助信息
help:
	@echo "$(BLUE)Subhuti AI Agent 框架$(NC)"
	@echo ""
	@echo "$(GREEN)开发命令:$(NC)"
	@echo "  build          编译 release 版本"
	@echo "  test           运行所有测试"
	@echo "  test-watch     监控文件变化自动测试"
	@echo "  serve          启动 HTTP 服务器 (release)"
	@echo "  serve-debug    启动 HTTP 服务器 (debug模式，日志更详细)"
	@echo "  serve-status   查看服务状态"
	@echo "  serve-logs     查看服务日志"
	@echo "  serve-stop     停止服务"
	@echo "  serve-restart  重启服务"
	@echo "  fmt            格式化代码"
	@echo "  clippy         运行 clippy 检查"
	@echo "  check          fmt + clippy + test 一键检查"
	@echo ""
	@echo "$(GREEN)Docker 命令:$(NC)"
	@echo "  docker-build   构建 Docker 镜像"
	@echo "  docker         启动 Docker 容器"
	@echo "  docker-stop    停止 Docker 容器"
	@echo "  docker-logs    查看容器日志"
	@echo ""
	@echo "$(GREEN)调试命令:$(NC)"
	@echo "  trace ID=<id>  格式化 Trace 调用链"
	@echo "  debug [cmd]    线上调试工具 (traces/logs/health)"
	@echo ""
	@echo "$(GREEN)🎯 核心调试 (Orchestrate) - 单/多 Agent 调用:$(NC)"
	@echo "  orchestrate MESSAGE=<msg>       调用编排接口"
	@echo "  orchestrate MESSAGE=<msg> CHAIN=<chain>  指定策略链"
	@echo "  orchestrate MESSAGE=<msg> USER=<user>    指定用户"
	@echo "  orchestrate-debug MESSAGE=<msg>          debug模式调用"
	@echo ""
	@echo "$(GREEN)CLI 工具:$(NC)"
	@echo "  subhuti serve           启动 HTTP 服务"
	@echo "  subhuti doctor          环境诊断"
	@echo "  subhuti log-stream      实时日志流查看器"
	@echo "  subhuti db              数据库操作"
	@echo "  subhuti api             API 测试客户端"
	@echo "  subhuti flame           性能火焰图"
	@echo ""
	@echo "$(GREEN)日志查询:$(NC)"
	@echo "  log-trace ID=<trace_id>    按 trace_id 查日志"
	@echo "  log-user ID=<user_id>      按用户 ID 查日志"
	@echo "  log-recent [N=20]          查看最近 N 条日志"
	@echo "  log-errors                 查看错误日志" 
	@echo ""
	@echo "$(GREEN)维护命令:$(NC)"
	@echo "  clean          清理构建产物"
	@echo "  install        安装 pre-commit hook"
	@echo ""
	@echo "$(GREEN)发布命令:$(NC)"
	@echo "  release-test   发布前全量验证（生成测试报告）"
	@echo "  release        自动发布（测试+提交+标签+推送）"

# ============================================================
# 开发
# ============================================================

build:
	@echo "$(GREEN)🔨 编译 release...$(NC)"
	cargo build --release --bin subhuti

test:
	@echo "$(GREEN)🧪 运行测试...$(NC)"
	cargo test --workspace

test-watch:
	@echo "$(GREEN)👀 监控测试 (文件变化自动重新运行)...$(NC)"
	cargo watch -x test

BUILD_MODE ?= release

serve:
	@echo "$(GREEN)🚀 启动 HTTP 服务器 ($(BUILD_MODE))...$(NC)"
	./scripts/build/dev.sh start $(BUILD_MODE)

serve-debug:
	@echo "$(GREEN)🚀 启动 HTTP 服务器 (debug)...$(NC)"
	BUILD_MODE=debug ./scripts/build/dev.sh start debug

serve-status:
	@echo "$(GREEN)📊 服务状态...$(NC)"
	./scripts/build/dev.sh status

serve-logs:
	@echo "$(GREEN)📋 服务日志...$(NC)"
	./scripts/build/dev.sh logs

serve-stop:
	@echo "$(GREEN)🛑 停止服务...$(NC)"
	./scripts/build/dev.sh stop

serve-restart:
	@echo "$(GREEN)🔄 重启服务 ($(BUILD_MODE))...$(NC)"
	./scripts/build/dev.sh restart $(BUILD_MODE)

fmt:
	@echo "$(GREEN)📝 格式化代码...$(NC)"
	cargo fmt --all

clippy:
	@echo "$(GREEN)🔍 Clippy 检查...$(NC)"
	cargo clippy --workspace -- -D warnings

check: fmt clippy test
	@echo "$(GREEN)✅ 所有检查通过!$(NC)"

# ============================================================
# Docker
# ============================================================

docker-build:
	@echo "$(GREEN)🐳 构建 Docker 镜像...$(NC)"
	./scripts/build/docker.sh build

docker: docker-build
	@echo "$(GREEN)🐳 启动 Docker 容器...$(NC)"
	./scripts/build/docker.sh start

docker-stop:
	@echo "$(GREEN)🛑 停止 Docker 容器...$(NC)"
	./scripts/build/docker.sh stop

docker-logs:
	@echo "$(GREEN)📋 Docker 日志...$(NC)"
	./scripts/build/docker.sh logs

# ============================================================
# 维护
# ============================================================

clean:
	@echo "$(YELLOW)🧹 清理构建产物...$(NC)"
	cargo clean
	rm -f http_server_bin

install:
	@echo "$(GREEN)🔧 安装 pre-commit hook...$(NC)"
	cp scripts/pre-commit .git/hooks/pre-commit
	chmod +x .git/hooks/pre-commit
	@echo "$(GREEN)✅ pre-commit hook 已安装$(NC)"

# ============================================================
# 发布
# ============================================================

release-test:
	@echo "$(GREEN)🚀 开始发布验证流程...$(NC)"
	./scripts/release/release-test.sh

# 自动发布（测试通过后自动推送）
release:
	@if [ -z "$(VERSION)" ]; then \
		echo "$(RED)❌ 错误：请指定版本号$(NC)"; \
		echo "用法: make release VERSION=v0.2.0"; \
		echo "示例: make release VERSION=v0.2.0 MESSAGE=\"日志查询 API 增强\""; \
		exit 1; \
	fi
	@echo "$(GREEN)🚀 开始自动发布 $(VERSION)...$(NC)"
	./scripts/release/auto-release.sh $(VERSION) "$(MESSAGE)"

# ============================================================
# 调试
# ============================================================

# 格式化 Trace 调用链
trace:
	@if [ -z "$(ID)" ]; then \
		echo "$(RED)❌ 错误：请指定 Trace ID$(NC)"; \
		echo "用法: make trace ID=<trace_id>"; \
		echo "示例: make trace ID=f1003e43-1d19-49aa-811b-d07b5bc12536"; \
		exit 1; \
	fi
	@echo "$(GREEN)🔍 Trace 调用链: $(ID)$(NC)"
	@echo ""
	@./scripts/debug/format-trace.sh $(ID)

# 线上调试工具
debug:
	@./scripts/debug/online-debug.sh $(CMD) $(ID)

# ============================================================
# CLI 工具
# ============================================================

cli-build:
	@echo "$(GREEN)🔨 编译 CLI 工具...$(NC)"
	cargo build --release --bin subhuti

cli-install: cli-build
	@echo "$(GREEN)📦 安装 CLI 工具...$(NC)"
	cp target/release/subhuti /usr/local/bin/subhuti

# 实时日志流
log-stream:
	@echo "$(GREEN)📡 实时日志流查看器$(NC)"
	@cargo run --bin subhuti -- log-stream $(ARGS)

# 数据库查询
db-query:
	@echo "$(GREEN)🔌 数据库查询$(NC)"
	@cargo run --bin subhuti -- db query $(ARGS)

db-tables:
	@echo "$(GREEN)📋 数据库表列表$(NC)"
	@cargo run --bin subhuti -- db list-tables

db-schema:
	@echo "$(GREEN)📋 表结构$(NC)"
	@cargo run --bin subhuti -- db schema $(ARGS)

db-stats:
	@echo "$(GREEN)📊 数据库统计$(NC)"
	@cargo run --bin subhuti -- db stats

# ============================================================
# 🎯 核心调试 - Orchestrate (单/多 Agent 调用)
# ============================================================

MESSAGE ?=
CHAIN ?=
USER ?= test_user

orchestrate:
	@if [ -z "$(MESSAGE)" ]; then \
		echo "$(RED)❌ 请指定 MESSAGE 参数$(NC)"; \
		echo "用法: make orchestrate MESSAGE='你的问题'"; \
		echo "示例: make orchestrate MESSAGE='分析这个心理问题'"; \
		exit 1; \
	fi
	@echo "$(GREEN)🎯 Orchestrate - 编排调用单/多 Agent$(NC)"
	@echo "─────────────────────────────────────────────────────────────"
	@echo "Message: $(MESSAGE)"
	@if [ ! -z "$(CHAIN)" ]; then echo "Chain: $(CHAIN)"; fi
	@if [ ! -z "$(USER)" ]; then echo "User: $(USER)"; fi
	@echo ""
	@cargo run --bin subhuti -- api orchestrate \
		--message "$(MESSAGE)" \
		$(if $(CHAIN),--chain $(CHAIN),) \
		$(if $(USER),--user-id $(USER),)

orchestrate-debug:
	@if [ -z "$(MESSAGE)" ]; then \
		echo "$(RED)❌ 请指定 MESSAGE 参数$(NC)"; \
		echo "用法: make orchestrate-debug MESSAGE='你的问题'"; \
		exit 1; \
	fi
	@echo "$(GREEN)🎯 Orchestrate - Debug 模式$(NC)"
	@echo "─────────────────────────────────────────────────────────────"
	@echo "Message: $(MESSAGE)"
	@echo ""
	@RUST_LOG=debug,tower_http=off,hyper=off,reqwest=off,sqlx=off \
	cargo run --bin subhuti -- api orchestrate \
		--message "$(MESSAGE)" \
		$(if $(CHAIN),--chain $(CHAIN),) \
		$(if $(USER),--user-id $(USER),)

# ============================================================
# API 测试
# ============================================================

api-chat:
	@echo "$(GREEN)💬 API Chat$(NC)"
	@cargo run --bin subhuti -- api chat $(ARGS)

api-health:
	@echo "$(GREEN)🏥 API Health$(NC)"
	@cargo run --bin subhuti -- api health

api-skills:
	@echo "$(GREEN)🎯 API Skills$(NC)"
	@cargo run --bin subhuti -- api skills

api-experts:
	@echo "$(GREEN)🧑‍🔬 API Experts$(NC)"
	@cargo run --bin subhuti -- api experts

api-persona:
	@echo "$(GREEN)💎 API Persona$(NC)"
	@cargo run --bin subhuti -- api persona

# 火焰图
flame:
	@echo "$(GREEN)🔥 性能火焰图$(NC)"
	@cargo run --bin subhuti -- flame $(ARGS)

# ============================================================
# 日志查询
# ============================================================

LOG_FILE := $(shell ls -1 logs/subhuti.log.* 2>/dev/null | sort -r | head -1 || echo "logs/subhuti.log")

log-trace:
	@if [ -z "$(ID)" ]; then \
		echo "$(RED)❌ 错误：请指定 trace_id$(NC)"; \
		echo "用法: make log-trace ID=<trace_id>"; \
		exit 1; \
	fi
	@echo "$(GREEN)📋 查询 trace_id: $(ID)$(NC)"
	@echo "─────────────────────────────────────────────────────────────"
	@jq "select(.span != null and .span.trace_id == \"$(ID)\")" $(LOG_FILE)

log-user:
	@if [ -z "$(ID)" ]; then \
		echo "$(RED)❌ 错误：请指定 user_id$(NC)"; \
		echo "用法: make log-user ID=<user_id>"; \
		exit 1; \
	fi
	@echo "$(GREEN)📋 查询 user_id: $(ID)$(NC)"
	@echo "─────────────────────────────────────────────────────────────"
	@grep "$(ID)" $(LOG_FILE) | jq "select(.span != null)" | \
		jq '{timestamp, level, target, span: .span.name, trace_id: .span.trace_id, session_id: .span.session_id, message: .fields.message}'

log-recent:
	@echo "$(GREEN)📋 最近 $(or $(N),20) 条日志$(NC)"
	@echo "─────────────────────────────────────────────────────────────"
	@tail -$(or $(N),20) $(LOG_FILE) | jq "select(.span != null)" | \
		jq '{timestamp, level, target, span: .span.name, message: .fields.message}'

log-errors:
	@echo "$(GREEN)📋 错误日志$(NC)"
	@echo "─────────────────────────────────────────────────────────────"
	@grep '"level":"ERROR"' $(LOG_FILE) | jq '{timestamp, target, message: .fields.message, file: .filename, line: .line_number}'
