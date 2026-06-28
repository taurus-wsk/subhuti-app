#!/bin/bash
# 专家插件生态系统测试脚本 v2
# 验证完整的插件生命周期：安装 -> 启用 -> 激活 -> 停用 -> 启用

BASE_URL="http://localhost:8080/subhuti/api/v1"

echo "=========================================="
echo "  专家插件生态系统测试 v2"
echo "=========================================="

# 1. 获取插件列表（详细）
echo ""
echo "【步骤1】获取插件详细信息..."
curl -s "$BASE_URL/experts/plugins" | jq '{
    success,
    total,
    plugins: [.data[] | {
        id,
        name,
        version,
        category,
        state,
        permissions: .permissions | {file_read, network, database},
        sandbox: .sandbox | {enabled, memory_limit_mb, daily_request_limit},
        hooks,
        enabled_at,
        activated_at
    }]
}'

# 2. 测试插件停用
echo ""
echo "【步骤2】测试停用插件..."
RESULT=$(curl -s -X POST "$BASE_URL/experts/psychology/disable")
echo "$RESULT" | jq .
if [ "$(echo "$RESULT" | jq -r '.success')" == "true" ]; then
    STATE=$(curl -s "$BASE_URL/experts/plugins" | jq -r '.data[0].state')
    if [ "$STATE" == "disabled" ]; then
        echo "✅ 插件停用成功，状态已更新为: $STATE"
    else
        echo "❌ 插件状态异常: $STATE"
    fi
fi

# 3. 测试插件启用
echo ""
echo "【步骤3】测试启用插件..."
RESULT=$(curl -s -X POST "$BASE_URL/experts/psychology/enable")
echo "$RESULT" | jq .
if [ "$(echo "$RESULT" | jq -r '.success')" == "true" ]; then
    STATE=$(curl -s "$BASE_URL/experts/plugins" | jq -r '.data[0].state')
    if [ "$STATE" == "enabled" ]; then
        echo "✅ 插件启用成功，状态已更新为: $STATE"
    else
        echo "❌ 插件状态异常: $STATE"
    fi
fi

# 4. 激活专家
echo ""
echo "【步骤4】激活心理咨询专家..."
RESULT=$(curl -s -X POST "$BASE_URL/experts/psychology/activate")
echo "$RESULT" | jq .

# 5. 验证 Persona 被覆盖
echo ""
echo "【步骤5】验证 Persona 已被专家覆盖..."
PERSONA=$(curl -s "$BASE_URL/persona")
NAME=$(echo "$PERSONA" | jq -r '.name')
if [ "$NAME" == "暖心心理咨询师" ]; then
    echo "✅ Persona 覆盖成功: $NAME"
else
    echo "❌ Persona 覆盖失败: $NAME"
fi

# 验证大五人格
AGREEABLENESS=$(echo "$PERSONA" | jq -r '.big_five.agreeableness')
if [ "$AGREEABLENESS" == "0.9" ]; then
    echo "✅ 宜人性参数正确: $AGREEABLENESS"
else
    echo "❌ 宜人性参数异常: $AGREEABLENESS"
fi

# 6. 验证技能注入
echo ""
echo "【步骤6】验证专家技能已注入..."
SKILLS=$(curl -s "$BASE_URL/skills")
MOOD_CHECK=$(echo "$SKILLS" | jq -r '.skills[] | select(.name == "mood_check") | .name')
STRESS_RELIEF=$(echo "$SKILLS" | jq -r '.skills[] | select(.name == "stress_relief") | .name')
if [ -n "$MOOD_CHECK" ]; then
    echo "✅ mood_check 技能已注入"
fi
if [ -n "$STRESS_RELIEF" ]; then
    echo "✅ stress_relief 技能已注入"
fi

# 7. 测试专家匹配
echo ""
echo "【步骤7】测试专家自动匹配..."
MATCH=$(curl -s -X POST "$BASE_URL/experts/match" \
    -H "Content-Type: application/json" \
    -d '{"input": "最近压力很大，晚上睡不着觉"}')
echo "$MATCH" | jq '.data.name'
if [ "$(echo "$MATCH" | jq -r '.data.name')" == "心理咨询专家" ]; then
    echo "✅ 专家匹配正确"
fi

# 8. 停用专家
echo ""
echo "【步骤8】停用专家..."
RESULT=$(curl -s -X POST "$BASE_URL/experts/deactivate")
echo "$RESULT" | jq .

# 9. 验证权限和沙箱配置
echo ""
echo "【步骤9】验证权限和沙箱配置..."
PLUGIN=$(curl -s "$BASE_URL/experts/plugins" | jq '.data[0]')
echo "权限检查:"
echo "$PLUGIN" | jq '.permissions'
echo "沙箱检查:"
echo "$PLUGIN" | jq '.sandbox'

# 验证权限都是 false（心理咨询不需要特殊权限）
FILE_READ=$(echo "$PLUGIN" | jq -r '.permissions.file_read')
NETWORK=$(echo "$PLUGIN" | jq -r '.permissions.network')
if [ "$FILE_READ" == "false" ] && [ "$NETWORK" == "false" ]; then
    echo "✅ 权限配置正确（默认禁止敏感操作）"
fi

# 验证沙箱配置
SANDBOX_ENABLED=$(echo "$PLUGIN" | jq -r '.sandbox.enabled')
MEMORY_LIMIT=$(echo "$PLUGIN" | jq -r '.sandbox.memory_limit_mb')
DAILY_LIMIT=$(echo "$PLUGIN" | jq -r '.sandbox.daily_request_limit')
if [ "$SANDBOX_ENABLED" == "true" ]; then
    echo "✅ 沙箱已启用"
fi
echo "   内存限制: ${MEMORY_LIMIT}MB"
echo "   每日限制: ${DAILY_LIMIT}次"

echo ""
echo "=========================================="
echo "  测试完成"
echo "=========================================="