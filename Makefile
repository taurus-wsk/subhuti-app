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
	@echo "  serve          启动 HTTP 服务器 (localhost:8080)"
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
	cargo build --release --bin http_server

test:
	@echo "$(GREEN)🧪 运行测试...$(NC)"
	cargo test --workspace

test-watch:
	@echo "$(GREEN)👀 监控测试 (文件变化自动重新运行)...$(NC)"
	cargo watch -x test

serve:
	@echo "$(GREEN)🚀 启动 HTTP 服务器...$(NC)"
	./scripts/build/dev.sh start

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
	@echo "$(GREEN)🔄 重启服务...$(NC)"
	./scripts/build/dev.sh restart

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
