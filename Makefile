# ============================================================
# Subhuti Makefile - 统一开发入口
# ============================================================
# 用法: make [target]
# ============================================================

.PHONY: help build test test-watch serve docker docker-build docker-stop fmt clippy check clean install release-test

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
	@echo "$(GREEN)维护命令:$(NC)"
	@echo "  clean          清理构建产物"
	@echo "  install        安装 pre-commit hook"
	@echo ""
	@echo "$(GREEN)发布命令:$(NC)"
	@echo "  release-test   发布前全量验证（生成测试报告）"

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
