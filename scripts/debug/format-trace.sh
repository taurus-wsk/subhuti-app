#!/bin/bash
# Trace 调用链可视化工具
# 用法: ./format-trace.sh <trace_id>

set -e

if [ -z "$1" ]; then
    echo "用法: $0 <trace_id>"
    echo "示例: $0 f1003e43-1d19-49aa-811b-d07b5bc12536"
    exit 1
fi

TRACE_ID="$1"
API_URL="http://localhost:8080/subhuti/api/v1/traces/$TRACE_ID"

# 获取 Trace 数据
TRACE_DATA=$(curl -s "$API_URL")

# 检查是否成功
SUCCESS=$(echo "$TRACE_DATA" | python3 -c "import sys, json; print(json.load(sys.stdin)['success'])")
if [ "$SUCCESS" != "True" ]; then
    echo "❌ Trace 不存在或查询失败"
    exit 1
fi

# 使用 Python 格式化输出
echo "$TRACE_DATA" | python3 -c "
import sys, json

trace = json.load(sys.stdin)['data']

# 基本信息
print(f'Trace ID: {trace[\"id\"]}')
print(f'用户输入: \"{trace[\"input\"]}\"')
print(f'总耗时: {trace[\"total_duration_ms\"]}ms ({trace[\"total_duration_ms\"]/1000:.1f}秒)')
status_icon = '✅ Success' if trace['status'] == 'Success' else '❌ ' + trace['status']
print(f'状态: {status_icon}')
print()

# 调用链
print('调用链:')

def print_span_tree(spans, span_id, is_root=True, prefix='', is_last=True):
    span = spans[span_id]
    kind = span['kind']
    name = span['name']
    duration = span['duration_ms']
    
    # 格式化名称
    kind_map = {
        'Request': '请求',
        'SkillMatch': 'Skill匹配',
        'LlmCall': 'LLM调用',
        'ToolCall': '工具调用',
        'MemorySearch': '记忆检索',
        'Response': '响应生成'
    }
    kind_cn = kind_map.get(kind, kind)
    
    # 树形符号
    if is_root:
        connector = '└──'
    elif is_last:
        connector = f'{prefix}└──'
    else:
        connector = f'{prefix}├──'
    
    # 打印当前 Span
    print(f'{connector} {name} ({kind_cn}) - {duration}ms')
    
    # 计算子节点的 prefix
    if is_root:
        child_prefix = '    '
    elif is_last:
        child_prefix = f'{prefix}    '
    else:
        child_prefix = f'{prefix}│   '
    
    # 打印输出信息（作为子节点）
    output = span.get('output', {})
    children_to_print = []
    
    if output:
        if kind == 'SkillMatch' and output.get('skill'):
            confidence = output.get('confidence', 0)
            children_to_print.append(('text', f'匹配到 {output[\"skill\"]}，置信度 {confidence}'))
        elif kind == 'LlmCall':
            prompt = output.get('prompt_tokens', 0)
            completion = output.get('completion_tokens', 0)
            total = prompt + completion
            children_to_print.append(('text', f'prompt_tokens: {prompt}'))
            children_to_print.append(('text', f'completion_tokens: {completion}'))
            children_to_print.append(('text', f'总 tokens: {total}'))
        elif kind == 'Request' and output.get('response'):
            response = output['response']
            if len(response) > 50:
                response = response[:50] + '...'
            children_to_print.append(('text', f'\"{response}\"'))
    
    # 获取子 Span
    children = span.get('children', [])
    for child_id in children:
        children_to_print.append(('span', child_id))
    
    # 打印所有子节点
    for i, (child_type, child_val) in enumerate(children_to_print):
        is_last_child = (i == len(children_to_print) - 1)
        
        if child_type == 'text':
            if is_last_child:
                conn = f'{child_prefix}└──'
            else:
                conn = f'{child_prefix}├──'
            print(f'{conn} {child_val}')
        elif child_type == 'span':
            print_span_tree(spans, child_val, False, child_prefix, is_last_child)

# 找到根 Span
root_span_id = trace['root_span_id']
spans = trace['spans']

print_span_tree(spans, root_span_id)

print()

# 其他信息
if trace.get('matched_skill'):
    print(f'匹配 Skill: {trace[\"matched_skill\"]}')
if trace.get('tools_used') and trace['tools_used']:
    print(f'使用工具: {\", \".join(trace[\"tools_used\"])}')
if trace.get('expert_id'):
    print(f'专家: {trace[\"expert_id\"]}')

print()
"
