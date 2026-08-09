# ============================================================
# Subhuti Makefile - 统一开发入口
# ============================================================
# 用法: make [target]
# ============================================================

.PHONY: help build test test-watch serve serve-debug serve-mock serve-status serve-logs serve-stop serve-restart docker docker-build docker-stop fmt clippy check clean install release-test trace orch-experts orch-analyze orch-match orch-run orch-all cache-clean cache-stats routes _py_init

# 默认目标
.DEFAULT_GOAL := help

# 颜色输出
GREEN  := \033[0;32m
YELLOW := \033[0;33m
BLUE   := \033[0;34m
CYAN   := \033[0;36m
RED    := \033[0;31m
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
	@echo "  serve-debug    启动 HTTP 服务器 (debug模式，默认使用 config/subhuti.md mock数据)"
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
	@echo "  orchestrate                    从 subhuti.md 读取消息调用"
	@echo "  orchestrate MESSAGE=<msg>      指定消息调用"
	@echo "  orchestrate CHAIN=<chain>      指定策略链"
	@echo "  orchestrate USER=<user>        指定用户"
	@echo "  orchestrate-debug              debug模式（从 subhuti.md 读取）"
	@echo ""
	@echo "$(GREEN)🔌 HTTP 调试 - 通过 HTTP API 测试 (需先启动 serve-debug):$(NC)"
	@echo "  http-orchestrate               从 subhuti.md 读取消息发送 HTTP Orchestrate 请求"
	@echo "  http-orchestrate HTTP_MESSAGE=<msg> 指定消息测试多 Agent 调度"
	@echo "  http-orchestrate HTTP_CHAIN=<chain> 指定策略链"
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
	@echo ""
	@echo "$(GREEN)🚀 服务启动:$(NC)"
	@echo "  serve-debug                                本地调试 (debug build)"
	@echo "  serve                                      生产模式 (release build)"
	@echo "  serve-mock                                 Mock 模式 (不打真实 API, 调 HTTP/编排逻辑用)"
	@echo ""
	@echo "$(GREEN)🎯 Orchestrate 调度调试 (HTTP, 需先启动服务 make serve-debug):$(NC)"
	@echo "  orch-experts                               列出所有已注册专家快照"
	@echo "  orch-analyze ORCH_MSG=<msg>                Layer1 任务分析（返回 domain_tags/task_type/...）"
	@echo "  orch-match   ORCH_MSG=<msg>                Layer1+Layer2 专家匹配（返回匹配到的专家列表）"
	@echo "  orch-run     ORCH_MSG=<msg>                Layer1+Layer2+Layer3 完整编排（走 LLM 生成最终回答）"
	@echo "  orch-all     ORCH_MSG=<msg>                一键跑完整链路：experts → analyze → match → run"
	@echo "  routes                                     列出所有已注册路由（inventory 自动收集）"
	@echo ""
	@echo "$(GREEN)🧪 LLM 调试缓存 (相同输入不打真实 API，秒回):$(NC)"
	@echo "  cache-stats                                查看缓存条目数 / 文件大小 / 最近 key"
	@echo "  cache-clean                                清空缓存文件 (.llm-cache.json)"
	@echo ""
	@echo "$(CYAN)📝 复制即用的调试示例:$(NC)"
	@echo "  make serve-debug                          # 启动调试服务"
	@echo ""
	@echo "$(CYAN)所有命令可选覆盖:$(NC)"
	@echo "  HTTP_ADDR=http://127.0.0.1:8615            服务地址（默认 localhost:8615）"
	@echo "  ORCH_USER=debug-user ORCH_SESSION=s1       user_id / session_id"
	@echo "  SUBHUTI_MOCK=1                             强制 Mock 模式（不打真实 API）"

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

# 🧪 Mock 模式启动（用 MockLLM 替代真实 LLM，不打网络，适合调试 HTTP/编排逻辑）
serve-mock:
	@echo "$(GREEN)🚀 启动 HTTP 服务器 (debug, Mock 模式)...$(NC)"
	BUILD_MODE=debug SUBHUTI_MOCK=1 ./scripts/build/dev.sh start debug

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
	@MESSAGE="$(MESSAGE)"; \
	if [ -z "$$MESSAGE" ]; then \
		if [ -f subhuti.md ]; then \
			MESSAGE=$$(cat subhuti.md); \
			echo "$(CYAN)📖 从 subhuti.md 读取消息$(NC)"; \
		else \
			echo "$(RED)❌ 未指定 MESSAGE 且 subhuti.md 不存在$(NC)"; \
			echo "用法: make orchestrate MESSAGE='你的问题'"; \
			echo "或创建 subhuti.md 文件写入默认消息"; \
			exit 1; \
		fi; \
	fi; \
	echo "$(GREEN)🎯 Orchestrate - 编排调用单/多 Agent$(NC)"; \
	echo "─────────────────────────────────────────────────────────────"; \
	echo "Message: $$MESSAGE"; \
	if [ ! -z "$(CHAIN)" ]; then echo "Chain: $(CHAIN)"; fi; \
	if [ ! -z "$(USER)" ]; then echo "User: $(USER)"; fi; \
	echo ""; \
	cargo run --bin subhuti -- api orchestrate \
		--message "$$MESSAGE" \
		$(if $(CHAIN),--chain $(CHAIN),) \
		$(if $(USER),--user-id $(USER),)

orchestrate-debug:
	@MESSAGE="$(MESSAGE)"; \
	if [ -z "$$MESSAGE" ]; then \
		if [ -f subhuti.md ]; then \
			MESSAGE=$$(cat subhuti.md); \
			echo "$(CYAN)📖 从 subhuti.md 读取消息$(NC)"; \
		else \
			echo "$(RED)❌ 未指定 MESSAGE 且 subhuti.md 不存在$(NC)"; \
			echo "用法: make orchestrate-debug MESSAGE='你的问题'"; \
			echo "或创建 subhuti.md 文件写入默认消息"; \
			exit 1; \
		fi; \
	fi; \
	echo "$(GREEN)🎯 Orchestrate - Debug 模式$(NC)"; \
	echo "─────────────────────────────────────────────────────────────"; \
	echo "Message: $$MESSAGE"; \
	if [ ! -z "$(CHAIN)" ]; then echo "Chain: $(CHAIN)"; fi; \
	if [ ! -z "$(USER)" ]; then echo "User: $(USER)"; fi; \
	echo ""; \
	@RUST_LOG=debug,tower_http=off,hyper=off,reqwest=off,sqlx=off \
	cargo run --bin subhuti -- api orchestrate \
		--message "$$MESSAGE" \
		$(if $(CHAIN),--chain $(CHAIN),) \
		$(if $(USER),--user-id $(USER),)

# ============================================================
# API 测试
# ============================================================

api-health:
	@echo "$(GREEN)🏥 API Health$(NC)"
	@cargo run --bin subhuti -- api health

api-skills:
	@echo "$(GREEN)🎯 API Skills$(NC)"
	@cargo run --bin subhuti -- api skills

api-experts:
	@echo "$(GREEN)🧑‍🔬 API Experts$(NC)"
	@cargo run --bin subhuti -- api experts

# ============================================================
# HTTP 调试 - 通过 HTTP API 测试多 Agent 调度
# ============================================================

HTTP_MESSAGE ?=
HTTP_CHAIN ?=
HTTP_USER ?= test_user
HTTP_ADDR ?= http://localhost:8615

http-orchestrate:
	@MESSAGE="$(HTTP_MESSAGE)"; \
	if [ -z "$$MESSAGE" ]; then \
		if [ -f subhuti.md ]; then \
			MESSAGE=$$(cat subhuti.md); \
			echo "$(CYAN)📖 从 subhuti.md 读取消息$(NC)"; \
		else \
			echo "$(RED)❌ 未指定 HTTP_MESSAGE 且 subhuti.md 不存在$(NC)"; \
			echo "用法: make http-orchestrate HTTP_MESSAGE='你的问题'"; \
			echo "或创建 subhuti.md 文件写入默认消息"; \
			exit 1; \
		fi; \
	fi; \
	echo "$(GREEN)🎯 HTTP Orchestrate - 通过 HTTP API 测试多 Agent 调度$(NC)"; \
	echo "─────────────────────────────────────────────────────────────"; \
	echo "Message: $$MESSAGE"; \
	if [ ! -z "$(HTTP_CHAIN)" ]; then echo "Chain: $(HTTP_CHAIN)"; fi; \
	if [ ! -z "$(HTTP_USER)" ]; then echo "User: $(HTTP_USER)"; fi; \
	echo ""; \
	curl -s -X POST "$(HTTP_ADDR)/subhuti/api/v1/orchestrate" \
		-H "Content-Type: application/json" \
		-d "{\"message\":\"$$MESSAGE\"}" | python3 -m json.tool

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

# ============================================================
# 🎯 Orchestrate 调度调试 (HTTP, 需先启动服务: make serve-debug)
# ============================================================
# 注意：每个 target 的脚本全部写成单行 shell 续行，
#       Python 格式化脚本全部写到临时文件再调用，避免 Make 把
#       Python 代码的物理换行解析成新的 recipe line（missing separator）。

ORCH_MSG     ?= 帮我用 Blender 做一个 5 秒的弹跳球动画
ORCH_USER    ?= debug-user
ORCH_SESSION ?= orch-session-1
PY_TMP_DIR   := $(shell mktemp -d)

# ── 小型 Python 格式化脚本：写入临时目录，各 target 复用 ──
_py_init:
	@printf '%s\n' \
	  "import sys,json" \
	  "d=json.load(sys.stdin)" \
	  "c=sys.argv[1]" \
	  "dd=d.get('data',d)" \
	  "if c=='experts':" \
	  "  data=dd.get('experts',d.get('data',[]))" \
	  "  if not data: print('  (empty)')" \
	  "  for e in data:" \
	  "    print('  ✅ 专家 id=%-20s name=%-20s  skills=%d  tags=%s' %(e.get('id',''),e.get('name',''),len(e.get('skills',[])),e.get('tags',[])))" \
	  "elif c=='analyze':" \
	  "  p=dd.get('profile',{})" \
	  "  print('  建议策略: ' + str(dd.get('suggested_strategy','-')))" \
	  "  if not p: print('    (empty)')" \
	  "  for k,v in p.items():" \
	  "    print('    %-15s = %s' %(k,v))" \
	  "  if 'error' in dd: print('    error: '+dd['error'])" \
	  "elif c=='match':" \
	  "  ms=dd.get('matches',[])" \
	  "  if not ms: print('    (empty)')" \
	  "  for i,e in enumerate(ms,1):" \
	  "    sk=e.get('skills',[]); top=sk[0].get('name','-') if sk else '-'" \
	  "    print('    #%d id=%-20s name=%-20s  skills=%d  top_skill=%s' %(i,e.get('id',''),e.get('name',''),len(sk),top))" \
	  "elif c=='run_header':" \
	  "  print('  状态: %s  ·  调度链: %s' %('OK' if d.get('success') else 'FAIL',' -> '.join(dd.get('chain',[])) or '-'))" \
	  "elif c=='run_body':" \
	  "  out=dd.get('output',dd.get('response',''))" \
	  "  for line in out.splitlines():" \
	  "    print('    '+line)" \
	  "  print('')" \
	  "  print('  详情链路:')" \
	  "  print('    session_id    :', dd.get('session_id','-'))" \
	  "  print('    expert_chain  :', dd.get('expert_chain',[]))" \
	  "  print('    expert_outputs: %d 项' % len(dd.get('expert_outputs',[])))" \
	> $(PY_TMP_DIR)/fmt.py

# ① GET /orchestrate/experts - 列出所有已注册专家
orch-experts: _py_init
	@echo "$(GREEN)🧑‍🔬 Orchestrate - 列出所有已注册专家$(NC)"
	@echo "─────────────────────────────────────────────────────────────"
	@echo "  HTTP : $(HTTP_ADDR)/subhuti/api/v1/orchestrate/experts"
	@TMP=$$(mktemp); \
	curl -s -m 10 -X GET "$(HTTP_ADDR)/subhuti/api/v1/orchestrate/experts" > $$TMP || true; \
	CODE=$$(python3 -c "import sys,json;d=json.load(open('$$TMP'));print('OK' if d.get('success') else 'FAIL')" 2>/dev/null || echo FAIL); \
	TOTAL=$$(python3 -c "import sys,json;d=json.load(open('$$TMP'));print(d.get('total',0))" 2>/dev/null || echo '?'); \
	echo "  响应状态: $$CODE  ·  专家总数: $$TOTAL"; \
	echo ""; \
	cat $$TMP | python3 $(PY_TMP_DIR)/fmt.py experts 2>/dev/null || (python3 -m json.tool $$TMP 2>/dev/null || cat $$TMP); \
	rm -f $$TMP
	@echo ""

# ② POST /orchestrate/analyze - Layer1 任务分析
orch-analyze: _py_init
	@echo "$(GREEN)🧠 Orchestrate - Layer1 任务分析 analyze_task$(NC)"
	@echo "─────────────────────────────────────────────────────────────"
	@echo "  HTTP : POST $(HTTP_ADDR)/subhuti/api/v1/orchestrate/analyze"
	@echo "  MSG  : $(ORCH_MSG)"
	@TMP=$$(mktemp); \
	curl -s -m 30 -X POST "$(HTTP_ADDR)/subhuti/api/v1/orchestrate/analyze" \
	  -H "Content-Type: application/json" \
	  -d "{\"message\":\"$(ORCH_MSG)\"}" > $$TMP || true; \
	SUCCESS=$$(python3 -c "import sys,json;d=json.load(open('$$TMP'));print('OK' if d.get('success') else 'FAIL')" 2>/dev/null || echo FAIL); \
	echo "  状态: $$SUCCESS"; \
	echo ""; \
	echo "  分析结果 (TaskProfile):"; \
	cat $$TMP | python3 $(PY_TMP_DIR)/fmt.py analyze 2>/dev/null || (python3 -m json.tool $$TMP 2>/dev/null || cat $$TMP); \
	rm -f $$TMP
	@echo ""

# ③ POST /orchestrate/match - Layer1+Layer2 专家匹配
orch-match: _py_init
	@echo "$(GREEN)🎯 Orchestrate - Layer1+Layer2 专家匹配 match_expert$(NC)"
	@echo "─────────────────────────────────────────────────────────────"
	@echo "  HTTP : POST $(HTTP_ADDR)/subhuti/api/v1/orchestrate/match"
	@echo "  MSG  : $(ORCH_MSG)"
	@TMP=$$(mktemp); \
	curl -s -m 30 -X POST "$(HTTP_ADDR)/subhuti/api/v1/orchestrate/match" \
	  -H "Content-Type: application/json" \
	  -d "{\"message\":\"$(ORCH_MSG)\"}" > $$TMP || true; \
	SUCCESS=$$(python3 -c "import sys,json;d=json.load(open('$$TMP'));print('OK' if d.get('success') else 'FAIL')" 2>/dev/null || echo FAIL); \
	TOTAL=$$(python3 -c "import sys,json;d=json.load(open('$$TMP'));print(d.get('total',0))" 2>/dev/null || echo '?'); \
	echo "  状态: $$SUCCESS  ·  匹配到专家数: $$TOTAL"; \
	echo ""; \
	echo "  匹配到的专家:"; \
	cat $$TMP | python3 $(PY_TMP_DIR)/fmt.py match 2>/dev/null || (python3 -m json.tool $$TMP 2>/dev/null || cat $$TMP); \
	rm -f $$TMP
	@echo ""

# ④ POST /orchestrate - Layer1+Layer2+Layer3 完整编排（走 LLM）
orch-run: _py_init
	@echo "$(GREEN)🚀 Orchestrate - 完整编排 (走 LLM, 默认 glm-4-flash)$(NC)"
	@echo "─────────────────────────────────────────────────────────────"
	@echo "  HTTP    : POST $(HTTP_ADDR)/subhuti/api/v1/orchestrate"
	@echo "  MSG     : $(ORCH_MSG)"
	@echo "  USER    : $(ORCH_USER)"
	@echo "  SESSION : $(ORCH_SESSION)"
	@TMP=$$(mktemp); START=$$(date +%s); \
	curl -s -m 120 -X POST "$(HTTP_ADDR)/subhuti/api/v1/orchestrate" \
	  -H "Content-Type: application/json" \
	  -d "{\"message\":\"$(ORCH_MSG)\",\"user_id\":\"$(ORCH_USER)\",\"session_id\":\"$(ORCH_SESSION)\"}" > $$TMP || true; \
	END=$$(date +%s); ELAPSED=$$((END-START)); \
	printf "  耗时: %ds  ·  " $$ELAPSED; \
	cat $$TMP | python3 $(PY_TMP_DIR)/fmt.py run_header 2>/dev/null || true; \
	echo ""; \
	echo "  最终回答:"; \
	cat $$TMP | python3 $(PY_TMP_DIR)/fmt.py run_body 2>/dev/null || (python3 -m json.tool $$TMP 2>/dev/null || cat $$TMP); \
	rm -f $$TMP

# 一键跑完整链路：experts → analyze → match → run
orch-all: orch-experts orch-analyze orch-match orch-run
	@echo "$(GREEN)✅ Orchestrate 全链路调试完成$(NC)"

# 列出所有已注册路由（方案 C：inventory 自动注册，需先启动服务）
routes:
	@echo "$(GREEN)📋 已注册路由列表（inventory 自动收集）$(NC)"
	@echo "─────────────────────────────────────────────────────────────"
	@echo "  HTTP : GET $(HTTP_ADDR)/subhuti/api/v1/_debug/routes"
	@TMP=$$(mktemp); \
	curl -s -m 10 -X GET "$(HTTP_ADDR)/subhuti/api/v1/_debug/routes" > $$TMP || true; \
	TOTAL=$$(python3 -c "import json;d=json.load(open('$$TMP'));print(d.get('total',0))" 2>/dev/null || echo '?'); \
	echo "  路由总数: $$TOTAL"; \
	echo ""; \
	python3 -c "import json;d=json.load(open('$$TMP'));[print('  %-8s %-45s%s' % (r.get('method','?'),r.get('path','?'),' 🔵trace' if r.get('trace_enabled') else '')) for r in d.get('routes',[])]" 2>/dev/null || (python3 -m json.tool $$TMP 2>/dev/null || cat $$TMP); \
	rm -f $$TMP
	@echo ""

# ============================================================
# 🧪 LLM 调试缓存管理
# ============================================================
LLM_CACHE_FILE ?= .llm-cache.json

# 清空 LLM 缓存文件
cache-clean:
	@echo "$(GREEN)🧹 清空 LLM 调试缓存$(NC)"
	@if [ -f "$(LLM_CACHE_FILE)" ]; then \
		rm -f "$(LLM_CACHE_FILE)"; \
		echo "  已删除: $(LLM_CACHE_FILE)"; \
	else \
		echo "  缓存文件不存在: $(LLM_CACHE_FILE)"; \
	fi

# 查看缓存文件统计（条数、大小）
cache-stats:
	@echo "$(GREEN)📊 LLM 调试缓存统计$(NC)"
	@echo "─────────────────────────────────────────────────────────────"
	@if [ -f "$(LLM_CACHE_FILE)" ]; then \
		SIZE=$$(wc -c < "$(LLM_CACHE_FILE)" | tr -d ' '); \
		COUNT=$$(python3 -c "import json;d=json.load(open('$(LLM_CACHE_FILE)'));print(len(d))" 2>/dev/null || echo '?'); \
		echo "  文件: $(LLM_CACHE_FILE)"; \
		echo "  条目数: $$COUNT / 100"; \
		echo "  文件大小: $$SIZE bytes"; \
		if [ "$$COUNT" != "?" ]; then \
			echo ""; \
			echo "  最近缓存的 5 条 (按 seq 倒序):"; \
			TMP_PY=$$(mktemp /tmp/subhuti-cache-stats.XXXXXX.py); \
			printf '%s\n' \
			  "import json" \
			  "d=json.load(open('$(LLM_CACHE_FILE)'))" \
			  "items=sorted(d.items(), key=lambda kv: kv[1].get('seq',0), reverse=True)[:5]" \
			  "for k,v in items:" \
			  "    print('    seq=%-6d key=%s..' % (v.get('seq',0), k[:16]))" \
			> $$TMP_PY; \
			python3 $$TMP_PY 2>/dev/null; \
			rm -f $$TMP_PY; \
		fi; \
	else \
		echo "  缓存文件不存在: $(LLM_CACHE_FILE)"; \
		echo "  提示: 启动 debug 服务后，缓存会自动生成"; \
	fi
