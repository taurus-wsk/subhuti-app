#!/bin/bash
# 线上调试脚本 - 在宿主机使用，调试 Docker 容器内的服务
# 用法: ./scripts/debug/online-debug.sh [command] [args]

set -e

API_BASE="http://localhost:8080/subhuti/api/v1"

# 颜色
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
NC='\033[0m'

usage() {
    echo -e "${GREEN}线上调试工具${NC}"
    echo ""
    echo "用法: $0 <command> [args]"
    echo ""
    echo "命令:"
    echo "  traces              列出所有 Trace"
    echo "  trace <id>          查看 Trace 调用链"
    echo "  viewer              打开 HTML 可视化查看器"
    echo "  logs [level]        查看日志 (ERROR/WARN/INFO)"
    echo "  health              健康检查"
    echo "  health-detailed     详细健康检查"
    echo "  skills              列出所有 Skill"
    echo ""
    echo "示例:"
    echo "  $0 traces"
    echo "  $0 trace f1003e43-1d19-49aa-811b-d07b5bc12536"
    echo "  $0 logs ERROR"
    echo "  $0 health"
    exit 1
}

# 检查服务是否运行
check_service() {
    if ! curl -s "$API_BASE/health" > /dev/null 2>&1; then
        echo -e "${RED}❌ 服务未运行或无法访问${NC}"
        echo "请检查: docker ps | grep subhuti"
        exit 1
    fi
}

case "$1" in
    traces)
        check_service
        echo -e "${GREEN}📋 Trace 列表${NC}"
        echo ""
        curl -s "$API_BASE/traces" | python3 -c "
import sys, json
data = json.load(sys.stdin)
traces = data.get('data', [])
print(f'共 {len(traces)} 个 Trace')
print()
for t in traces:
    print(f'Trace ID: {t[\"trace_id\"]}')
    print(f'  输入: {t[\"input\"][:50]}')
    print(f'  耗时: {t[\"duration_ms\"]}ms')
    print(f'  Skill: {t[\"matched_skill\"]}')
    print(f'  状态: {t[\"status\"]}')
    print()
"
        ;;
    
    viewer)
        check_service
        echo -e "${GREEN}🌐 打开 Trace HTML 可视化查看器...${NC}"
        open http://localhost:8080/subhuti/test/trace-viewer.html
        ;;
    
    trace)
        if [ -z "$2" ]; then
            echo -e "${RED}❌ 请提供 Trace ID${NC}"
            echo "用法: $0 trace <trace_id>"
            exit 1
        fi
        check_service
        echo -e "${GREEN}🔍 Trace 调用链: $2${NC}"
        echo ""
        
        # 使用 format-trace.sh
        SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
        "$SCRIPT_DIR/format-trace.sh" "$2"
        ;;
    
    logs)
        check_service
        LEVEL="${2:-INFO}"
        echo -e "${GREEN}📋 日志 (级别: $LEVEL)${NC}"
        echo ""
        curl -s "$API_BASE/logs?level=$LEVEL&limit=50" | python3 -m json.tool
        ;;
    
    health)
        check_service
        echo -e "${GREEN}🏥 健康检查${NC}"
        echo ""
        curl -s "$API_BASE/health" | python3 -m json.tool
        ;;
    
    health-detailed)
        check_service
        echo -e "${GREEN}🏥 详细健康检查${NC}"
        echo ""
        curl -s "$API_BASE/health/detailed" | python3 -m json.tool
        ;;
    
    skills)
        check_service
        echo -e "${GREEN}🎯 Skill 列表${NC}"
        echo ""
        curl -s "$API_BASE/skills" | python3 -m json.tool
        ;;
    
    *)
        usage
        ;;
esac
