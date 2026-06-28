#!/bin/bash
# ============================================================
# Subhuti 版本测试脚本 - v0.x.0
# ============================================================
# 用途：针对特定版本的需求进行专项测试
# 用法：./version-test-v0.x.x.sh [full|quick|api|perf]
# ============================================================
set -e

PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
BASE_URL="http://localhost:8080/subhuti/api/v1"
PASS_COUNT=0
FAIL_COUNT=0

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
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
# 前置检查
# ============================================================
check_service_running() {
    echo -e "${BLUE}检查服务状态...${NC}"
    if ! curl -sf "$BASE_URL/health" > /dev/null 2>&1; then
        echo -e "${RED}❌ 服务未运行，请先启动服务${NC}"
        echo "   运行: ./dev.sh start"
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
}

# ============================================================
# 新功能测试（根据版本需求添加）
# ============================================================
test_new_features() {
    echo ""
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}新功能测试 - v0.x.0${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    
    # 示例：专家系统测试
    echo "🧑‍⚕️ 专家列表..."
    EXPERTS=$(curl -sf "$BASE_URL/experts" 2>&1)
    if echo "$EXPERTS" | grep -q '"data"'; then
        record_test "专家列表" "PASS" "API 正常"
    else
        record_test "专家列表" "FAIL" "API 异常"
    fi
    
    # TODO: 根据版本需求添加更多测试
    # echo "🔍 新功能 1..."
    # RESULT=$(curl -sf "$BASE_URL/new-feature" 2>&1)
    # if echo "$RESULT" | grep -q '"success"'; then
    #     record_test "新功能 1" "PASS" "功能正常"
    # else
    #     record_test "新功能 1" "FAIL" "功能异常"
    # fi
}

# ============================================================
# API 回归测试
# ============================================================
test_api_regression() {
    echo ""
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}API 回归测试${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    
    # 聊天功能
    echo "💬 聊天功能..."
    CHAT=$(curl -sf -X POST "$BASE_URL/chat" \
        -H "Content-Type: application/json" \
        -d '{"message": "你好", "user_id": "test", "session_id": "test"}' \
        2>&1)
    if echo "$CHAT" | grep -q '"response"'; then
        record_test "聊天功能" "PASS" "正常响应"
    else
        record_test "聊天功能" "FAIL" "聊天失败"
    fi
    
    # Trace 追踪
    echo "🔍 Trace 追踪..."
    TRACES=$(curl -sf "$BASE_URL/traces" 2>&1)
    if echo "$TRACES" | grep -q '"data"'; then
        record_test "Trace 追踪" "PASS" "API 正常"
    else
        record_test "Trace 追踪" "FAIL" "API 异常"
    fi
}

# ============================================================
# 性能测试
# ============================================================
test_performance() {
    echo ""
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}性能测试${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    
    # API 响应时间
    echo "⚡ 健康检查响应时间..."
    START=$(date +%s%N)
    curl -sf "$BASE_URL/health" > /dev/null 2>&1
    END=$(date +%s%N)
    ELAPSED=$(( (END - START) / 1000000 ))
    
    if [ "$ELAPSED" -lt 500 ]; then
        record_test "响应时间" "PASS" "${ELAPSED}ms < 500ms"
    else
        record_test "响应时间" "FAIL" "${ELAPSED}ms >= 500ms"
    fi
    
    # TODO: 添加更多性能测试
}

# ============================================================
# 主流程
# ============================================================
TEST_MODE="${1:-full}"

echo -e "${BLUE}"
echo "╔══════════════════════════════════════════════════════════╗"
echo "║     Subhuti 版本测试 - v0.x.0                          ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo -e "${NC}"
echo "测试模式: $TEST_MODE"
echo ""

# 前置检查
check_service_running

case "$TEST_MODE" in
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
        echo "用法: $0 [full|quick|api|perf]"
        exit 1
        ;;
esac

# 测试总结
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}测试总结${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"

TOTAL=$((PASS_COUNT + FAIL_COUNT))
echo -e "  总计: $TOTAL 项"
echo -e "  ${GREEN}通过: $PASS_COUNT${NC}"
echo -e "  ${RED}失败: $FAIL_COUNT${NC}"

if [ "$FAIL_COUNT" -gt 0 ]; then
    echo -e "\n${RED}❌ 有测试失败${NC}"
    exit 1
else
    echo -e "\n${GREEN}✅ 所有测试通过${NC}"
    exit 0
fi
