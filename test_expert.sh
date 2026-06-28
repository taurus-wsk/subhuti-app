#!/bin/bash
# 专家插件激活测试脚本
# 验证 persona 和知识库是否正确注入

BASE_URL="http://localhost:8080/subhuti/api/v1"
EXPERT_ID="psychology"

echo "=========================================="
echo "  专家插件激活测试"
echo "=========================================="

# 1. 获取专家列表
echo ""
echo "【步骤1】获取已注册专家列表..."
curl -s "$BASE_URL/experts" | jq .

# 2. 获取当前激活的专家（应该是空的）
echo ""
echo "【步骤2】获取当前激活专家（激活前）..."
curl -s "$BASE_URL/experts/active" | jq .

# 3. 获取当前 persona（激活前）
echo ""
echo "【步骤3】获取当前 persona（激活前）..."
curl -s "$BASE_URL/persona" | jq '.name, .description, .tone'

# 4. 激活心理咨询专家
echo ""
echo "【步骤4】激活心理咨询专家 (ID: $EXPERT_ID)..."
curl -s -X POST "$BASE_URL/experts/$EXPERT_ID/activate" | jq .

# 5. 获取当前激活的专家（激活后）
echo ""
echo "【步骤5】获取当前激活专家（激活后）..."
ACTIVE_INFO=$(curl -s "$BASE_URL/experts/active")
echo "$ACTIVE_INFO" | jq .

# 验证激活的专家是否正确
ACTIVE_NAME=$(echo "$ACTIVE_INFO" | jq -r '.data.name')
if [ "$ACTIVE_NAME" == "心理咨询师" ]; then
    echo "✅ 专家激活成功！当前专家: $ACTIVE_NAME"
else
    echo "❌ 专家激活失败！期望: 心理咨询师，实际: $ACTIVE_NAME"
fi

# 6. 获取当前 persona（验证是否被专家覆盖）
echo ""
echo "【步骤6】获取当前 persona（验证是否被专家覆盖）..."
PERSONA=$(curl -s "$BASE_URL/persona")
echo "$PERSONA" | jq '.name, .description, .tone, .big_five'

# 验证 persona 是否被更新
PERSONA_NAME=$(echo "$PERSONA" | jq -r '.name')
if [ "$PERSONA_NAME" == "暖心心理咨询师" ]; then
    echo "✅ Persona 已被专家覆盖！当前名称: $PERSONA_NAME"
else
    echo "❌ Persona 未被正确更新！期望: 暖心心理咨询师，实际: $PERSONA_NAME"
fi

# 验证宜人性（心理咨询师应该是高宜人性 0.9）
AGREEABLENESS=$(echo "$PERSONA" | jq -r '.big_five.agreeableness')
if [ "$AGREEABLENESS" == "0.9" ]; then
    echo "✅ 大五人格参数正确！宜人性: $AGREEABLENESS"
else
    echo "❌ 大五人格参数异常！期望宜人性: 0.9，实际: $AGREEABLENESS"
fi

# 7. 获取 Skill 列表（验证是否注入了专家技能）
echo ""
echo "【步骤7】获取 Skill 列表（验证是否注入了专家技能）..."
SKILLS=$(curl -s "$BASE_URL/skills")
echo "$SKILLS" | jq '.skills[].name'

# 验证是否包含专家技能
MOOD_CHECK=$(echo "$SKILLS" | jq -r '.skills[] | select(.name == "mood_check") | .name' | head -1)
STRESS_RELIEF=$(echo "$SKILLS" | jq -r '.skills[] | select(.name == "stress_relief") | .name' | head -1)

if [ -n "$MOOD_CHECK" ]; then
    echo "✅ 专家技能 mood_check 已注入"
else
    echo "❌ 专家技能 mood_check 未注入"
fi

if [ -n "$STRESS_RELIEF" ]; then
    echo "✅ 专家技能 stress_relief 已注入"
else
    echo "❌ 专家技能 stress_relief 未注入"
fi

# 8. 测试专家匹配功能
echo ""
echo "【步骤8】测试专家匹配功能..."
MATCH_RESULT=$(curl -s -X POST "$BASE_URL/experts/match" \
    -H "Content-Type: application/json" \
    -d '{"input": "我最近压力很大，睡不着觉"}')
echo "$MATCH_RESULT" | jq .

MATCHED_NAME=$(echo "$MATCH_RESULT" | jq -r '.data.name')
if [ "$MATCHED_NAME" == "心理咨询师" ]; then
    echo "✅ 专家匹配正确！输入 '压力很大' 匹配到: $MATCHED_NAME"
else
    echo "⚠️ 匹配结果: $MATCHED_NAME（可能为空表示无匹配）"
fi

# 9. 停用专家
echo ""
echo "【步骤9】停用专家..."
curl -s -X POST "$BASE_URL/experts/deactivate" | jq .

# 10. 验证停用后状态
echo ""
echo "【步骤10】验证停用后状态..."
curl -s "$BASE_URL/experts/active" | jq .

echo ""
echo "=========================================="
echo "  测试完成"
echo "=========================================="