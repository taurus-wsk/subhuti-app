#!/bin/bash
# Orchestrate 调试验证脚本
# 用法: ./scripts/debug/verify-orchestrate.sh [message]

set -e

MESSAGE="${1:-测试编排流程}"
SESSION_ID="test-orchestrate-$(date +%s)"

echo "🎯 Orchestrate 调试验证"
echo "═══════════════════════════════════════════════════════════"
echo "消息: $MESSAGE"
echo "Session: $SESSION_ID"
echo ""

# 1️⃣ 调用 Orchestrate API (直接调用 HTTP，不通过 CLI)
echo "📤 调用 Orchestrate API..."
RESPONSE=$(curl -s -X POST http://localhost:8080/subhuti/api/v1/orchestrate \
  -H "Content-Type: application/json" \
  -d "{
    \"message\": \"$MESSAGE\",
    \"session_id\": \"$SESSION_ID\"
  }")

TRACE_ID=$(echo "$RESPONSE" | python3 -c "import sys, json; print(json.load(sys.stdin).get('trace_id', 'N/A'))")
DURATION=$(echo "$RESPONSE" | python3 -c "import sys, json; print(json.load(sys.stdin).get('duration_ms', 'N/A'))")
CHAIN=$(echo "$RESPONSE" | python3 -c "import sys, json; print(json.load(sys.stdin).get('chain', 'N/A'))")

echo "✅ 调用成功"
echo "  Trace ID: $TRACE_ID"
echo "  耗时: ${DURATION}ms"
echo "  策略链: $CHAIN"
echo ""

# 2️⃣ 验证日志记录
echo "🔍 验证日志记录..."
LOG_COUNT=$(curl -s "http://localhost:8080/subhuti/api/v1/logs?keyword=$SESSION_ID&limit=100" | python3 -c "import sys, json; print(len(json.load(sys.stdin).get('logs', [])))")

if [ "$LOG_COUNT" -gt 0 ]; then
    echo "✅ 找到 $LOG_COUNT 条相关日志"
    echo ""
    echo "📋 日志列表:"
    curl -s "http://localhost:8080/subhuti/api/v1/logs?keyword=$SESSION_ID&limit=10" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for log in data.get('logs', []):
    ts = log['timestamp'][:19]
    msg = log['message'][:100]
    level = log['level']
    print(f'  [{ts}] [{level}] {msg}')
"
else
    echo "❌ 未找到相关日志"
fi
echo ""

# 3️⃣ 验证 Trace（如果启用）
echo "🔍 验证 Trace 记录..."
TRACE_DATA=$(curl -s "http://localhost:8080/subhuti/api/v1/traces/$TRACE_ID")
TRACE_EXISTS=$(echo "$TRACE_DATA" | python3 -c "import sys, json; d=json.load(sys.stdin).get('data', {}); print('yes' if d and d.get('id') else 'no')")

if [ "$TRACE_EXISTS" = "yes" ]; then
    echo "✅ Trace 找到！"
    echo "$TRACE_DATA" | python3 -c "
import sys, json
data = json.load(sys.stdin)
trace = data.get('data', {})
print(f'  ID: {trace[\"id\"]}')
print(f'  输入: {trace.get(\"input\", \"-\")}')
print(f'  输出: {trace.get(\"output\", \"-\")[:80]}...')
print(f'  总耗时: {trace.get(\"total_duration_ms\", \"-\")}ms')
print(f'  状态: {trace.get(\"status\", \"-\")}')
print(f'  Spans: {len(trace.get(\"spans\", {}))} 个')
"
else
    echo "❌ Trace 未记录（Trace 持久化可能未启用）"
fi
echo ""

# 4️⃣ 验证 Session 记录
echo "🔍 验证 Session 记录..."
SESSION_DATA=$(curl -s "http://localhost:8080/subhuti/api/v1/sessions/$SESSION_ID")
SESSION_EXISTS=$(echo "$SESSION_DATA" | python3 -c "import sys, json; d=json.load(sys.stdin).get('data', {}); print('yes' if d and d.get('session_id') else 'no')")

if [ "$SESSION_EXISTS" = "yes" ]; then
    echo "✅ Session 找到！"
    echo "$SESSION_DATA" | python3 -c "
import sys, json
data = json.load(sys.stdin)
session = data.get('data', {})
print(f'  Session ID: {session[\"session_id\"]}')
print(f'  User ID: {session.get(\"user_id\", \"-\")}')
print(f'  总请求数: {session.get(\"total_requests\", 0)}')
for i, req in enumerate(session.get('requests', []), 1):
    print(f'  请求 #{i}:')
    print(f'    输入: {req[\"input\"]}')
    print(f'    耗时: {req.get(\"duration_ms\", \"-\")}ms')
    print(f'    Skill: {req.get(\"matched_skill\", \"-\")}')
"
else
    echo "❌ Session 未记录"
fi
echo ""

# 5️⃣ 总结
echo "═══════════════════════════════════════════════════════════"
echo "📊 验证结果汇总:"
echo "═══════════════════════════════════════════════════════════"
echo ""
echo "✅ 日志记录: $( [ $LOG_COUNT -gt 0 ] && echo '正常' || echo '未找到' )"
echo "   - 日志文件: ./logs/subhuti.log.*"
echo "   - 相关日志: $LOG_COUNT 条"
echo ""
echo "✅ Session 记录: $( [ $SESSION_EXISTS = 'yes' ] && echo '正常' || echo '未记录' )"
echo ""
echo "💡 查看完整 Trace 调用链:"
echo "   ./scripts/debug/rebuild-trace-from-log.sh $TRACE_ID"
echo ""
echo "或查看日志详情:"
echo "   grep '$SESSION_ID' ./logs/subhuti.log.* | python3 -m json.tool"
echo ""
echo "═══════════════════════════════════════════════════════════"
