#!/bin/bash
# 从日志文件重建 Trace 树
# 用法: ./scripts/debug/rebuild-trace-from-log.sh <trace_id>

set -e

TRACE_ID="$1"

if [ -z "$TRACE_ID" ]; then
    echo "❌ 请指定 Trace ID"
    echo "用法: $0 <trace_id>"
    echo ""
    echo "示例:"
    echo "  $0 b40e9f2b-91d7-4c8a-adcb-b451b7e725c1"
    exit 1
fi

echo "🔍 从日志重建 Trace: $TRACE_ID"
echo "═══════════════════════════════════════════════════════════"
echo ""

# 在所有日志文件中搜索
LOG_FILES="./logs/subhuti.log ./logs/subhuti.log.*"

# 提取该 trace_id 的所有日志
echo "📋 查找相关日志..."
MATCHED_LOGS=$(grep -h "$TRACE_ID" $LOG_FILES 2>/dev/null || echo "")

if [ -z "$MATCHED_LOGS" ]; then
    echo "❌ 未找到该 Trace ID 的日志"
    exit 1
fi

LOG_COUNT=$(echo "$MATCHED_LOGS" | wc -l | tr -d ' ')
echo "✅ 找到 $LOG_COUNT 条相关日志"
echo ""

# 解析并显示 Trace 树
echo "🌳 Trace 调用链:"
echo ""

echo "$MATCHED_LOGS" | python3 -c "
import sys, json

logs = []
for line in sys.stdin:
    try:
        log = json.loads(line.strip())
        logs.append(log)
    except:
        pass

# 按时间排序
logs.sort(key=lambda x: x.get('timestamp', ''))

# 提取 span 信息
spans = []
for log in logs:
    span_data = log.get('span', {})
    if span_data:
        span_name = span_data.get('name', 'unknown')
        spans.append({
            'name': span_name,
            'timestamp': log.get('timestamp', ''),
            'message': log.get('fields', {}).get('message', ''),
            'level': log.get('level', 'INFO'),
            'target': log.get('target', '')
        })

# 显示 Trace 信息
if spans:
    print(f'Trace ID: {sys.argv[1]}')
    print(f'总 Spans: {len(spans)}')
    print()
    print('调用链:')
    
    for i, span in enumerate(spans, 1):
        indent = '  ' * (i - 1)
        ts = span['timestamp'][11:23]  # HH:MM:SS.mmm
        print(f'{indent}[{ts}] {span[\"name\"]}')
        if span['message'] and span['message'] != 'close':
            msg = span['message'][:80]
            print(f'{indent}  └─ {msg}')
else:
    print('未找到 span 信息')
" "$TRACE_ID"

echo ""
echo "═══════════════════════════════════════════════════════════"
echo "💡 提示: 完整的 Trace 信息都在日志文件中，无需额外存储！"
