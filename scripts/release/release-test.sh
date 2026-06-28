#!/bin/bash
# ============================================================
# Subhuti 发布验证脚本
# ============================================================
# 用途：发版本前全量验证
# 覆盖：编译 → Docker 构建 → 启动 → API 测试 → 单元测试 → 报告
# ============================================================
set -e

PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPORT_FILE="$PROJECT_DIR/RELEASE_TEST_REPORT.md"
PASS_COUNT=0
FAIL_COUNT=0
WARN_COUNT=0

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# 测试结果记录
declare -a TEST_RESULTS=()

record_test() {
    local name="$1"
    local status="$2"  # PASS, FAIL, WARN
    local detail="$3"

    TEST_RESULTS+=("$name|$status|$detail")

    case "$status" in
        PASS) PASS_COUNT=$((PASS_COUNT + 1)); echo -e "  ${GREEN}✅ PASS${NC} $detail" ;;
        FAIL) FAIL_COUNT=$((FAIL_COUNT + 1)); echo -e "  ${RED}❌ FAIL${NC} $detail" ;;
        WARN) WARN_COUNT=$((WARN_COUNT + 1)); echo -e "  ${YELLOW}⚠️  WARN${NC} $detail" ;;
    esac
}

generate_report() {
    local total_time="$1"
    local total=$((PASS_COUNT + FAIL_COUNT + WARN_COUNT))

    cat > "$REPORT_FILE" << EOF
# Subhuti 发布验证报告

**生成时间**: $(date '+%Y-%m-%d %H:%M:%S')  
**总耗时**: ${total_time}s  
**测试结果**: ${PASS_COUNT} 通过 / ${FAIL_COUNT} 失败 / ${WARN_COUNT} 警告

---

## 测试详情

EOF

    for result in "${TEST_RESULTS[@]}"; do
        IFS='|' read -r name status detail <<< "$result"
        case "$status" in
            PASS) echo "- ✅ **$name**: $detail" >> "$REPORT_FILE" ;;
            FAIL) echo "- ❌ **$name**: $detail" >> "$REPORT_FILE" ;;
            WARN) echo "- ⚠️ **$name**: $detail" >> "$REPORT_FILE" ;;
        esac
    done

    cat >> "$REPORT_FILE" << EOF

---

## 环境信息

- **Rust 版本**: $(rustc --version)
- **Cargo 版本**: $(cargo --version)
- **Docker 版本**: $(docker --version)
- **平台**: $(uname -m) $(uname -s)

EOF

    if [ "$FAIL_COUNT" -gt 0 ]; then
        echo "## ⚠️ 有测试失败，不建议发布" >> "$REPORT_FILE"
    else
        echo "## ✅ 所有测试通过，可以发布" >> "$REPORT_FILE"
    fi

    echo ""
    echo -e "${BLUE}📄 测试报告已生成: $REPORT_FILE${NC}"
}

# ============================================================
# 开始测试
# ============================================================
TOTAL_START=$(date +%s)

echo -e "${BLUE}"
echo "╔══════════════════════════════════════════════════════════╗"
echo "║           Subhuti 发布验证流程                           ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo -e "${NC}"

# ──────────────────────────────────────────────────────────────
# 阶段 1: 代码质量检查
# ──────────────────────────────────────────────────────────────
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}阶段 1: 代码质量检查${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"

echo "📝 代码格式化检查..."
if cargo fmt --all -- --check 2>/dev/null; then
    record_test "代码格式化" "PASS" "所有代码格式正确"
else
    record_test "代码格式化" "FAIL" "请运行 cargo fmt --all"
fi

echo "🔍 Clippy 检查..."
CLIPPY_OUTPUT=$(cargo clippy --workspace -- -D warnings 2>&1)
if [ $? -eq 0 ]; then
    record_test "Clippy" "PASS" "无警告"
else
    record_test "Clippy" "FAIL" "发现警告或错误"
fi

# ──────────────────────────────────────────────────────────────
# 阶段 2: Release 编译
# ──────────────────────────────────────────────────────────────
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}阶段 2: Release 编译${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"

echo "🔨 编译 release 版本..."
COMPILE_START=$(date +%s)
if cargo build --release --bin http_server 2>&1 | tail -5; then
    COMPILE_TIME=$(( $(date +%s) - COMPILE_START ))
    BINARY_SIZE=$(du -h "$PROJECT_DIR/target/release/http_server" | cut -f1)
    record_test "Release 编译" "PASS" "耗时 ${COMPILE_TIME}s, 二进制大小 ${BINARY_SIZE}"
else
    record_test "Release 编译" "FAIL" "编译失败"
fi

# ──────────────────────────────────────────────────────────────
# 阶段 3: 单元测试
# ──────────────────────────────────────────────────────────────
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}阶段 3: 单元测试${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"

echo "🧪 运行所有测试..."
TEST_OUTPUT=$(cargo test --workspace 2>&1)
if echo "$TEST_OUTPUT" | grep -q "test result: ok"; then
    TEST_COUNT=$(echo "$TEST_OUTPUT" | grep "test result: ok" | grep -oP '\d+ passed' | head -1)
    record_test "单元测试" "PASS" "$TEST_COUNT"
else
    FAIL_COUNT_INTERNAL=$(echo "$TEST_OUTPUT" | grep "test result" | grep -oP '\d+ failed' || echo "0")
    record_test "单元测试" "FAIL" "$FAIL_COUNT_INTERNAL 个测试失败"
fi

# ──────────────────────────────────────────────────────────────
# 阶段 4: Docker 构建
# ──────────────────────────────────────────────────────────────
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}阶段 4: Docker 构建${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"

echo "🐳 构建 Docker 镜像..."
DOCKER_START=$(date +%s)
DOCKER_OUTPUT=$("$PROJECT_DIR/docker.sh" build 2>&1)
if echo "$DOCKER_OUTPUT" | grep -q "镜像构建完成"; then
    DOCKER_TIME=$(( $(date +%s) - DOCKER_START ))
    IMAGE_SIZE=$(docker images subhuti:latest --format "{{.Size}}")
    record_test "Docker 构建" "PASS" "耗时 ${DOCKER_TIME}s, 镜像大小 ${IMAGE_SIZE}"
else
    record_test "Docker 构建" "FAIL" "构建失败"
fi

# ──────────────────────────────────────────────────────────────
# 阶段 5: Docker 启动
# ──────────────────────────────────────────────────────────────
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}阶段 5: Docker 容器启动${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"

# 清理旧容器
"$PROJECT_DIR/docker.sh" stop 2>/dev/null || true

echo "🚀 启动 Docker 容器..."
"$PROJECT_DIR/docker.sh" start 2>&1 > /dev/null
sleep 5

# 检查容器状态
if docker ps --format '{{.Names}}' | grep -q "subhuti-app"; then
    CONTAINER_STATUS=$(docker ps --filter "name=subhuti-app" --format '{{.Status}}')
    record_test "容器启动" "PASS" "$CONTAINER_STATUS"
else
    record_test "容器启动" "FAIL" "容器未运行"
fi

# ──────────────────────────────────────────────────────────────
# 阶段 6: API 测试
# ──────────────────────────────────────────────────────────────
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}阶段 6: API 功能测试${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"

# 健康检查
echo "💚 健康检查..."
HEALTH=$(curl -sf http://localhost:8080/subhuti/api/v1/health 2>&1)
if echo "$HEALTH" | grep -q '"status":"ok"'; then
    record_test "健康检查" "PASS" "服务正常"
else
    record_test "健康检查" "FAIL" "服务异常"
fi

# 详细健康
echo "📊 详细状态..."
DETAILED=$(curl -sf http://localhost:8080/subhuti/api/v1/health/detailed 2>&1)
OVERALL=$(echo "$DETAILED" | python3 -c "import sys,json; print(json.load(sys.stdin).get('overall_healthy','false'))" 2>/dev/null)
if [ "$OVERALL" = "True" ] || [ "$OVERALL" = "true" ]; then
    COMP_COUNT=$(echo "$DETAILED" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('components',[])))" 2>/dev/null)
    record_test "详细健康" "PASS" "$COMP_COUNT 个组件全部正常"
else
    record_test "详细健康" "FAIL" "有组件异常"
fi

# 技能列表
echo "🛠️  技能列表..."
SKILLS=$(curl -sf http://localhost:8080/subhuti/api/v1/skills 2>&1)
SKILL_COUNT=$(echo "$SKILLS" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('skills',[])))" 2>/dev/null)
if [ "$SKILL_COUNT" -gt 0 ] 2>/dev/null; then
    record_test "技能列表" "PASS" "$SKILL_COUNT 个技能"
else
    record_test "技能列表" "FAIL" "技能列表为空或解析失败"
fi

# 专家列表
echo "🧑‍⚕️  专家列表..."
EXPERTS=$(curl -sf http://localhost:8080/subhuti/api/v1/experts 2>&1)
EXPERT_COUNT=$(echo "$EXPERTS" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('data',[])))" 2>/dev/null)
if [ "$EXPERT_COUNT" -gt 0 ] 2>/dev/null; then
    record_test "专家列表" "PASS" "$EXPERT_COUNT 个专家"
else
    record_test "专家列表" "FAIL" "专家列表为空"
fi

# 人格信息
echo "🧠 人格信息..."
PERSONA=$(curl -sf http://localhost:8080/subhuti/api/v1/persona 2>&1)
if echo "$PERSONA" | grep -q '"name"'; then
    PERSONA_NAME=$(echo "$PERSONA" | python3 -c "import sys,json; print(json.load(sys.stdin).get('name',''))" 2>/dev/null)
    record_test "人格信息" "PASS" "名称: $PERSONA_NAME"
else
    record_test "人格信息" "FAIL" "人格数据异常"
fi

# Trace 列表
echo "🔍 Trace 追踪..."
TRACES=$(curl -sf http://localhost:8080/subhuti/api/v1/traces 2>&1)
if echo "$TRACES" | grep -q '"data"'; then
    record_test "Trace 追踪" "PASS" "Trace API 正常"
else
    record_test "Trace 追踪" "FAIL" "Trace API 异常"
fi

# 聊天测试
echo "💬 聊天测试..."
CHAT_START=$(date +%s)
CHAT=$(curl -sf -X POST http://localhost:8080/subhuti/api/v1/chat \
    -H "Content-Type: application/json" \
    -d '{"message": "你好，请回复一个 OK", "user_id": "release_test", "session_id": "release-test-session"}' \
    2>&1)
CHAT_TIME=$(( $(date +%s) - CHAT_START ))

if echo "$CHAT" | grep -q '"response"'; then
    RESPONSE=$(echo "$CHAT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('response','')[:50])" 2>/dev/null)
    CHAIN=$(echo "$CHAT" | python3 -c "import sys,json; d=json.load(sys.stdin); c=d.get('chain',[]); print(c[0] if c else 'unknown')" 2>/dev/null)
    record_test "聊天功能" "PASS" "响应: $RESPONSE..., 耗时 ${CHAT_TIME}s"
else
    record_test "聊天功能" "FAIL" "聊天失败"
fi

# ──────────────────────────────────────────────────────────────
# 阶段 7: 清理
# ──────────────────────────────────────────────────────────────
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}阶段 7: 清理${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"

echo "🛑 停止容器..."
"$PROJECT_DIR/docker.sh" stop 2>&1 > /dev/null
record_test "容器清理" "PASS" "容器已停止并移除"

# ──────────────────────────────────────────────────────────────
# 生成报告
# ──────────────────────────────────────────────────────────────
TOTAL_TIME=$(( $(date +%s) - TOTAL_START ))

echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}测试总结${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"

echo -e "  总计: $((PASS_COUNT + FAIL_COUNT + WARN_COUNT)) 项"
echo -e "  ${GREEN}通过: $PASS_COUNT${NC}"
echo -e "  ${RED}失败: $FAIL_COUNT${NC}"
echo -e "  ${YELLOW}警告: $WARN_COUNT${NC}"
echo -e "  耗时: ${TOTAL_TIME}s"

if [ "$FAIL_COUNT" -gt 0 ]; then
    echo -e "\n${RED}❌ 有测试失败，不建议发布${NC}"
    generate_report "$TOTAL_TIME"
    exit 1
else
    echo -e "\n${GREEN}✅ 所有测试通过，可以发布！${NC}"
    generate_report "$TOTAL_TIME"
    exit 0
fi
