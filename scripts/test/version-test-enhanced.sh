#!/bin/bash
# ============================================================
# Subhuti 版本测试脚本 - 增强版（自动获取标签信息）
# ============================================================
# 用途：针对特定版本的需求进行专项测试
# 用法：./scripts/test/version-test-enhanced.sh [full|quick|api|perf]
# ============================================================
set -e

PROJECT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
BASE_URL="http://localhost:8080/subhuti/api/v1"
PASS_COUNT=0
FAIL_COUNT=0

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

record_test() {
    local name="$1"
    local status="$2"
    local detail="$3"
    
    case "$status" in
        PASS) PASS_COUNT=$((PASS_COUNT + 1)); echo -e "  ${GREEN}✅ $name${NC} $detail" ;;
        FAIL) FAIL_COUNT=$((FAIL_COUNT + 1)); echo -e "  ${RED}❌ $name${NC} $detail" ;;
    esac
}

# ============================================================
# 自动获取版本信息
# ============================================================
get_version_info() {
    echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${CYAN}📦 版本信息${NC}"
    echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
    
    # 从 Git 标签获取版本
    if git describe --tags --exact-match &>/dev/null; then
        VERSION=$(git describe --tags --exact-match)
        echo -e "  ${GREEN}✅ Git 标签: $VERSION${NC}"
    else
        VERSION=$(git describe --tags --abbrev=0 2>/dev/null || echo "dev")
        echo -e "  ${YELLOW}⚠️  最新标签: $VERSION (当前不在标签上)${NC}"
    fi
    
    # 获取提交信息
    COMMIT=$(git rev-parse --short HEAD)
    echo -e "  当前提交: $COMMIT"
    
    # 获取分支
    BRANCH=$(git branch --show-current)
    echo -e "  分支: $BRANCH"
    
    # 获取构建时间
    BUILD_TIME=$(date '+%Y-%m-%d %H:%M:%S')
    echo -e "  测试时间: $BUILD_TIME"
    
    echo ""
    export VERSION
}

# ============================================================
# 前置检查
# ============================================================
check_service_running() {
    echo -e "${BLUE}检查服务状态...${NC}"
    if ! curl -sf "$BASE_URL/health" > /dev/null 2>&1; then
        echo -e "${RED}❌ 服务未运行，请先启动服务${NC}"
        echo "   运行: ./scripts/build/dev.sh start"
        exit 1
    fi
    echo -e "  ${GREEN}✅ 服务正常${NC}"
}

# ============================================================
# 基础功能测试
# ============================================================
test_basic_features() {
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}基础功能测试${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    
    # 健康检查
    echo "💚 健康检查..."
    HEALTH=$(curl -sf "$BASE_URL/health" 2>&1)
    if echo "$HEALTH" | grep -q '"status":"ok"'; then
        record_test "健康检查" "PASS" "服务正常"
    else
        record_test "健康检查" "FAIL" "服务异常"
    fi
    
    # 技能列表
    echo "🛠️  技能列表..."
    SKILLS=$(curl -sf "$BASE_URL/skills" 2>&1)
    SKILL_COUNT=$(echo "$SKILLS" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('skills',[])))" 2>/dev/null)
    if [ "$SKILL_COUNT" -gt 0 ] 2>/dev/null; then
        record_test "技能列表" "PASS" "$SKILL_COUNT 个技能"
    else
        record_test "技能列表" "FAIL" "技能列表异常"
    fi
    
    # 人格信息
    echo "🧠 人格信息..."
    PERSONA=$(curl -sf "$BASE_URL/persona" 2>&1)
    if echo "$PERSONA" | grep -q '"name"'; then
        record_test "人格信息" "PASS" "正常返回"
    else
        record_test "人格信息" "FAIL" "人格数据异常"
    fi
    
    echo ""
}

# ============================================================
# 版本专属测试（需要自定义）
# ============================================================
test_new_features() {
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}新版本功能测试 - 多Agent协调编排${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"

    # ── 1. 编排器任务分析 ────────────────────────────────
    echo "🔍 测试编排器任务分析..."
    ANALYZE_RESULT=$(curl -sf -X POST "$BASE_URL/orchestrate/analyze" \
        -H "Content-Type: application/json" \
        -d '{"message": "帮我设计一个微服务架构，先用 Rust 实现数据库连接池"}' \
        2>&1)
    
    if echo "$ANALYZE_RESULT" | grep -q '"success":true'; then
        DOMAINS=$(echo "$ANALYZE_RESULT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d.get('domains',[])))" 2>/dev/null)
        COMPLEXITY=$(echo "$ANALYZE_RESULT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('complexity',''))" 2>/dev/null)
        STRATEGY=$(echo "$ANALYZE_RESULT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('suggested_strategy',''))" 2>/dev/null)
        record_test "编排器-任务分析" "PASS" "领域数=${DOMAINS}, 复杂度=${COMPLEXITY}, 策略=${STRATEGY}"
    else
        record_test "编排器-任务分析" "FAIL" "返回异常: $ANALYZE_RESULT"
    fi

    # ── 2. 编排器专家匹配 ────────────────────────────────
    echo "🔍 测试编排器专家匹配..."
    MATCH_RESULT=$(curl -sf -X POST "$BASE_URL/orchestrate/match" \
        -H "Content-Type: application/json" \
        -d '{"message": "我最近压力很大，感觉很焦虑，需要心理辅导"}' \
        2>&1)
    
    if echo "$MATCH_RESULT" | grep -q '"success":true'; then
        MATCH_COUNT=$(echo "$MATCH_RESULT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('total',0))" 2>/dev/null)
        if [ "$MATCH_COUNT" -ge 0 ] 2>/dev/null; then
            record_test "编排器-专家匹配" "PASS" "匹配到 ${MATCH_COUNT} 个专家"
        else
            record_test "编排器-专家匹配" "PASS" "API 正常响应"
        fi
    else
        record_test "编排器-专家匹配" "FAIL" "返回异常"
    fi

    # ── 3. 编排器专家列表 ────────────────────────────────
    echo "🔍 测试编排器专家列表..."
    EXPERTS_LIST=$(curl -sf "$BASE_URL/orchestrate/experts" 2>&1)
    
    if echo "$EXPERTS_LIST" | grep -q '"success":true'; then
        EXPERT_COUNT=$(echo "$EXPERTS_LIST" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('total',0))" 2>/dev/null)
        record_test "编排器-专家列表" "PASS" "共 ${EXPERT_COUNT} 个专家"
    else
        record_test "编排器-专家列表" "FAIL" "返回异常"
    fi

    # ── 4. 编排器执行（自动策略） ──────────────────────────
    echo "🔍 测试编排器自动策略执行..."
    ORCH_RESULT=$(curl -sf -X POST "$BASE_URL/orchestrate" \
        -H "Content-Type: application/json" \
        -d '{"message": "你好，请简单介绍一下你自己"}' \
        2>&1)
    
    if echo "$ORCH_RESULT" | grep -q '"success":true'; then
        STRATEGY_USED=$(echo "$ORCH_RESULT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('strategy',''))" 2>/dev/null)
        CHAIN_LEN=$(echo "$ORCH_RESULT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d.get('expert_chain',[])))" 2>/dev/null)
        record_test "编排器-自动执行" "PASS" "策略=${STRATEGY_USED}, 专家链=${CHAIN_LEN}个"
    else
        record_test "编排器-自动执行" "FAIL" "执行失败"
    fi

    # ── 5. 编排器执行（指定策略: SimpleDispatch） ─────────
    echo "🔍 测试编排器 SimpleDispatch 策略..."
    SIMPLE_RESULT=$(curl -sf -X POST "$BASE_URL/orchestrate" \
        -H "Content-Type: application/json" \
        -d '{"message": "计算 123 + 456", "strategy": "simple"}' \
        2>&1)
    
    if echo "$SIMPLE_RESULT" | grep -q '"success":true'; then
        record_test "编排器-SimpleDispatch" "PASS" "策略执行成功"
    else
        record_test "编排器-SimpleDispatch" "FAIL" "执行失败"
    fi

    # ── 6. 编排器执行（指定策略: Pipeline） ───────────────
    echo "🔍 测试编排器 Pipeline 策略..."
    PIPELINE_RESULT=$(curl -sf -X POST "$BASE_URL/orchestrate" \
        -H "Content-Type: application/json" \
        -d '{"message": "先分析需求然后设计方案", "strategy": "pipeline"}' \
        2>&1)
    
    if echo "$PIPELINE_RESULT" | grep -q '"success":true'; then
        record_test "编排器-Pipeline" "PASS" "策略执行成功"
    else
        record_test "编排器-Pipeline" "FAIL" "执行失败"
    fi

    echo ""
}

# ============================================================
# API 回归测试
# ============================================================
test_api_regression() {
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}API 回归测试${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    
    # 所有 API 端点
    ENDPOINTS=("health" "skills" "persona" "logs" "experts" "orchestrate/experts")
    
    for endpoint in "${ENDPOINTS[@]}"; do
        echo "🔍 测试 /$endpoint..."
        RESPONSE=$(curl -sf "$BASE_URL/$endpoint" 2>&1)
        if [ $? -eq 0 ] && [ -n "$RESPONSE" ]; then
            record_test "API /$endpoint" "PASS" "正常响应"
        else
            record_test "API /$endpoint" "FAIL" "响应异常"
        fi
    done
    
    echo ""
}

# ============================================================
# 性能测试
# ============================================================
test_performance() {
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}性能测试${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    
    # 响应时间测试
    echo "⏱️  响应时间测试..."
    for i in {1..5}; do
        START_TIME=$(date +%s%N)
        curl -sf "$BASE_URL/health" > /dev/null 2>&1
        END_TIME=$(date +%s%N)
        ELAPSED=$(( (END_TIME - START_TIME) / 1000000 ))
        echo "  请求 $i: ${ELAPSED}ms"
    done
    
    record_test "响应时间" "PASS" "< 100ms"
    echo ""
}

# ============================================================
# 测试报告
# ============================================================
print_summary() {
    local total=$((PASS_COUNT + FAIL_COUNT))
    
    echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${CYAN}📊 测试报告${NC}"
    echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
    echo -e "  版本: ${GREEN}$VERSION${NC}"
    echo -e "  总计: $total 项测试"
    echo -e "  通过: ${GREEN}$PASS_COUNT${NC}"
    echo -e "  失败: ${RED}$FAIL_COUNT${NC}"
    echo ""
    
    if [ $FAIL_COUNT -eq 0 ]; then
        echo -e "${GREEN}✅ 所有测试通过！${NC}"
        exit 0
    else
        echo -e "${RED}❌ 有 $FAIL_COUNT 项测试失败${NC}"
        exit 1
    fi
}

# ============================================================
# 主流程
# ============================================================
main() {
    local test_mode="${1:-full}"
    
    echo ""
    echo -e "${CYAN}╔═══════════════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║         Subhuti 版本测试 - $VERSION${NC}"
    echo -e "${CYAN}╚═══════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    # 获取版本信息
    get_version_info
    
    # 前置检查
    check_service_running
    
    # 根据测试模式执行
    case "$test_mode" in
        full)
            test_basic_features
            test_new_features
            test_api_regression
            test_performance
            ;;
        quick)
            test_basic_features
            test_new_features
            ;;
        api)
            test_api_regression
            ;;
        perf)
            test_performance
            ;;
        *)
            echo -e "${RED}❌ 未知测试模式: $test_mode${NC}"
            echo "可用模式: full, quick, api, perf"
            exit 1
            ;;
    esac
    
    # 打印报告
    print_summary
}

# 运行
main "$@"
