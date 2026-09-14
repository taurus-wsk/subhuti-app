# Subhuti API 使用教程

> 手把手教你使用 Subhuti 框架的所有 API

---

## 📋 目录

1. [基础 API](#基础-api)
2. [心灵宫殿 API](#心灵宫殿-api)
3. [专家插件 API](#专家插件-api)
4. [记忆管理 API](#记忆管理-api)
5. [系统 API](#系统-api)
6. [完整示例](#完整示例)

---

## 基础 API

### 1. 聊天接口

最核心的 API，发送消息获取回复。

#### 请求

```http
POST /subhuti/api/v1/orchestrate
Content-Type: application/json
```

#### 请求体

```json
{
  "message": "你好，我是小明",
  "user_id": "test_user_001",
  "session_id": "session_abc123",
  "stream": false
}
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `message` | string | ✅ | 用户消息内容 |
| `user_id` | string | ❌ | 用户 ID，用于区分不同用户 |
| `session_id` | string | ❌ | 会话 ID，用于多轮对话 |
| `stream` | boolean | ❌ | 是否流式输出，默认 false |

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "response": "你好小明！有什么我可以帮助你的吗？",
    "trace_id": "trace_abc123def456",
    "matched_skill": "default_chat",
    "expert_id": null,
    "used_memories": 2
  }
}
```

| 字段 | 说明 |
|------|------|
| `response` | AI 回复内容 |
| `trace_id` | 追踪 ID，用于排查问题 |
| `matched_skill` | 匹配到的技能 |
| `expert_id` | 当前活跃的专家插件 ID |
| `used_memories` | 使用了多少条相关记忆 |

#### curl 示例

```bash
curl -X POST http://localhost:8080/subhuti/api/v1/orchestrate \
  -H "Content-Type: application/json" \
  -d '{
    "message": "你好，今天天气怎么样？",
    "user_id": "user_001"
  }'
```

#### JavaScript 示例

```javascript
const response = await fetch('http://localhost:8080/subhuti/api/v1/orchestrate', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    message: '你好，今天天气怎么样？',
    user_id: 'user_001'
  })
});

const data = await response.json();
console.log(data.data.output);
```

---

### 2. 流式聊天

支持 Server-Sent Events (SSE) 流式输出。

#### 请求

```http
POST /subhuti/api/v1/orchestrate
Content-Type: application/json
Accept: text/event-stream
```

#### 请求体

```json
{
  "message": "写一个关于秋天的故事",
  "user_id": "user_001"
}
```

#### 响应流

```
data: {"type": "start", "session_id": "..."}

data: {"type": "step", "phase": "analyze", "message": "指定专家: rust-expert", "session_id": "..."}

data: {"type": "data", "content": "秋", "session_id": "..."}

data: {"type": "data", "content": "天", "session_id": "..."}

...

data: {"type": "done", "content": "<完整答案>", "trace_id": "trace_abc", "session_id": "..."}
```

#### JavaScript 示例

`/orchestrate` 是 POST + SSE。浏览器原生 `EventSource` 只支持 GET，所以需用 `fetch` 读取响应流：

```javascript
const res = await fetch('/subhuti/api/v1/orchestrate', {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
    'Accept': 'text/event-stream',
  },
  body: JSON.stringify({ message: '写一个关于秋天的故事', user_id: 'user_001' }),
});

const reader = res.body.getReader();
const decoder = new TextDecoder();
let buffer = '';
let fullText = '';

while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  buffer += decoder.decode(value, { stream: true });

  const lines = buffer.split('\n');
  buffer = lines.pop() ?? '';
  for (const line of lines) {
    if (!line.startsWith('data:')) continue;
    const data = JSON.parse(line.slice(5).trim());
    if (data.type === 'data') {
      fullText += data.content;
    } else if (data.type === 'done') {
      fullText = data.content;
      console.log('完成！trace_id:', data.trace_id);
    }
  }
}
```
---

## 心灵宫殿 API

### 1. 获取统计信息

查看心灵宫殿的整体状态。

#### 请求

```http
GET /subhuti/api/v1/palace/stats
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "total_count": 42,
    "zone_counts": {
      "DailyChat": 15,
      "ExpertKnowledge": 8,
      "Emotional": 5,
      "TaskProgress": 7,
      "CreativeIdeas": 4,
      "Default": 3
    },
    "importance_distribution": {
      "Trivial": 10,
      "Normal": 25,
      "Important": 5,
      "Core": 2
    },
    "short_term_count": 15,
    "archive_count": 20,
    "knowledge_count": 7
  }
}
```

#### curl 示例

```bash
curl http://localhost:8080/subhuti/api/v1/palace/stats | python3 -m json.tool
```

---

### 2. 搜索记忆

在心灵宫殿中搜索相关记忆。

#### 请求

```http
POST /subhuti/api/v1/palace/search
Content-Type: application/json
```

#### 请求体

```json
{
  "query": "天气",
  "limit": 10,
  "user_id": "user_001",
  "use_persona_bias": true
}
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `query` | string | ✅ | 搜索关键词 |
| `limit` | number | ❌ | 返回数量上限，默认 10 |
| `user_id` | string | ❌ | 用户 ID |
| `use_persona_bias` | boolean | ❌ | 是否使用人格偏好加权 |

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "results": [
      {
        "id": "mem_abc123",
        "content": "今天天气真好，适合出去散步",
        "score": 0.95,
        "zone": "DailyChat",
        "importance": "Normal",
        "created_at": "2026-06-28T10:00:00Z"
      },
      ...
    ],
    "total": 5
  }
}
```

#### curl 示例

```bash
curl -X POST http://localhost:8080/subhuti/api/v1/palace/search \
  -H "Content-Type: application/json" \
  -d '{"query": "天气", "limit": 5, "use_persona_bias": true}'
```

---

### 3. 执行遗忘周期

手动触发遗忘机制，清理弱记忆。

#### 请求

```http
POST /subhuti/api/v1/palace/forget
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "forgotten_count": 3,
    "before_count": 45,
    "after_count": 42
  }
}
```

#### curl 示例

```bash
curl -X POST http://localhost:8080/subhuti/api/v1/palace/forget
```

---

### 4. 人格分区偏好

查看当前人格对各记忆分区的偏好权重。

#### 请求

```http
GET /subhuti/api/v1/soul/persona/zone-bias
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "DailyChat": 1.0,
    "ExpertKnowledge": 0.7,
    "Emotional": 1.2,
    "TaskProgress": 0.62,
    "CreativeIdeas": 1.1,
    "Default": 1.0
  }
}
```

**说明**：
- 权重 > 1.0：偏好该分区，搜索时会加权提升
- 权重 < 1.0：不偏好该分区，搜索时会降低权重
- 权重 = 1.0：中性，不影响

---

## 专家插件 API

### 1. 列出所有插件

查看已注册的专家插件。

#### 请求

```http
GET /subhuti/api/v1/experts/list
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "plugins": [
      {
        "id": "psychological_counselor",
        "name": "心理咨询专家",
        "description": "专业的心理咨询和情绪疏导",
        "version": "1.0.0",
        "author": "Subhuti Team",
        "status": "loaded"
      },
      {
        "id": "math_tutor",
        "name": "数学家教",
        "description": "数学问题解答和学习指导",
        "version": "1.0.0",
        "author": "Subhuti Team",
        "status": "loaded"
      }
    ],
    "active_expert_id": null
  }
}
```

#### curl 示例

```bash
curl http://localhost:8080/subhuti/api/v1/experts/list | python3 -m json.tool
```

---

### 2. 激活专家插件

切换到指定的专家角色。

#### 请求

```http
POST /subhuti/api/v1/experts/activate
Content-Type: application/json
```

#### 请求体

```json
{
  "expert_id": "psychological_counselor"
}
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "expert_id": "psychological_counselor",
    "expert_name": "心理咨询专家",
    "persona_injected": true,
    "knowledge_loaded": true
  }
}
```

#### curl 示例

```bash
curl -X POST http://localhost:8080/subhuti/api/v1/experts/activate \
  -H "Content-Type: application/json" \
  -d '{"expert_id": "psychological_counselor"}'
```

---

### 3. 停用专家插件

退出当前专家，恢复默认角色。

#### 请求

```http
POST /subhuti/api/v1/experts/deactivate
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "previous_expert_id": "psychological_counselor",
    "current_expert_id": null
  }
}
```

#### curl 示例

```bash
curl -X POST http://localhost:8080/subhuti/api/v1/experts/deactivate
```

---

### 4. 获取当前活跃专家

查看当前激活的专家。

#### 请求

```http
GET /subhuti/api/v1/experts/active
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "expert_id": "psychological_counselor",
    "expert_name": "心理咨询专家",
    "activated_at": "2026-06-28T10:00:00Z"
  }
}
```

---

## 记忆管理 API

### 1. 手动添加记忆

向记忆系统添加一条记忆。

#### 请求

```http
POST /subhuti/api/v1/memory/add
Content-Type: application/json
```

#### 请求体

```json
{
  "content": "用户的生日是 1990 年 5 月 20 日",
  "layer": "archive",
  "tags": ["个人信息", "生日"]
}
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `content` | string | ✅ | 记忆内容 |
| `layer` | string | ❌ | 记忆层级：short_term/archive/knowledge |
| `tags` | string[] | ❌ | 标签列表 |

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "memory_id": "mem_abc123def456",
    "layer": "archive"
  }
}
```

---

### 2. 搜索记忆（底层 API）

直接搜索底层记忆系统。

#### 请求

```http
POST /subhuti/api/v1/memory/search
Content-Type: application/json
```

#### 请求体

```json
{
  "query": "生日",
  "limit": 5,
  "layer": "all"
}
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "results": [
      {
        "id": "mem_abc123",
        "content": "用户的生日是 1990 年 5 月 20 日",
        "score": 0.92,
        "layer": "archive"
      }
    ]
  }
}
```

---

### 3. 获取记忆统计

查看记忆系统统计。

#### 请求

```http
GET /subhuti/api/v1/memory/stats
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "short_term_count": 15,
    "archive_count": 20,
    "knowledge_count": 7,
    "total_count": 42
  }
}
```

---

## 系统 API

### 1. 健康检查

检查系统是否正常运行。

#### 请求

```http
GET /subhuti/api/v1/health
```

#### 响应

```json
{
  "healthy": true,
  "timestamp": "2026-06-28T10:00:00Z"
}
```

#### curl 示例

```bash
curl http://localhost:8080/subhuti/api/v1/health
```

---

### 2. 详细健康检查

查看每个组件的详细状态。

#### 请求

```http
GET /subhuti/api/v1/health/detailed
```

#### 响应

```json
{
  "overall_healthy": true,
  "timestamp": "2026-06-28T10:00:00Z",
  "components": [
    {
      "name": "MemoryPalace",
      "healthy": true,
      "optional": false,
      "details": {
        "total_memories": "42",
        "short_term": "15",
        "archive": "20",
        "knowledge": "7"
      }
    },
    {
      "name": "Database",
      "healthy": false,
      "optional": true,
      "details": {
        "reason": "Not configured (optional component)",
        "enabled": "false"
      }
    },
    {
      "name": "SoulLayer",
      "healthy": true,
      "optional": false,
      "details": {
        "persona_version": "1",
        "persona_name": "Subhuti",
        "total_interactions": "100"
      }
    },
    {
      "name": "ExpertPlugins",
      "healthy": true,
      "optional": false,
      "details": {
        "plugin_count": "2",
        "active_expert": "psychological_counselor"
      }
    },
    {
      "name": "Skills",
      "healthy": true,
      "optional": false,
      "details": {
        "skill_count": "4"
      }
    }
  ]
}
```

#### curl 示例

```bash
curl http://localhost:8080/subhuti/api/v1/health/detailed | python3 -m json.tool
```

---

### 3. Trace 追踪

根据 trace_id 查看请求的完整追踪链。

#### 请求

```http
GET /subhuti/api/v1/trace/{trace_id}
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "trace_id": "trace_abc123",
    "spans": [
      {
        "name": "request",
        "duration_ms": 250,
        "start_time": "2026-06-28T10:00:00Z",
        "children": [
          {
            "name": "skill_match",
            "duration_ms": 5,
            "status": "ok"
          },
          {
            "name": "memory_retrieval",
            "duration_ms": 20,
            "status": "ok"
          },
          {
            "name": "llm_call",
            "duration_ms": 200,
            "status": "ok"
          },
          {
            "name": "memory_store",
            "duration_ms": 10,
            "status": "ok"
          },
          {
            "name": "soul_update",
            "duration_ms": 15,
            "status": "ok"
          }
        ]
      }
    ]
  }
}
```

---

## 完整示例

### 示例 1：基础对话流程

```javascript
// 完整的对话流程示例
async function chatExample() {
  const baseUrl = 'http://localhost:8080';
  
  // 1. 先检查系统健康
  const health = await fetch(`${baseUrl}/subhuti/api/v1/health`);
  const healthData = await health.json();
  console.log('系统健康:', healthData.healthy);
  
  // 2. 发送第一条消息
  const chat1 = await fetch(`${baseUrl}/subhuti/api/v1/orchestrate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      message: '你好，我叫小明，是一名程序员',
      user_id: 'xiaoming_001'
    })
  });
  const data1 = await chat1.json();
  console.log('AI 回复:', data1.data.output);
  console.log('Trace ID:', data1.data.trace_id);
  
  // 3. 查看心灵宫殿状态
  const stats = await fetch(`${baseUrl}/subhuti/api/v1/palace/stats`);
  const statsData = await stats.json();
  console.log('记忆总数:', statsData.data.total_count);
  
  // 4. 发送第二条消息（测试记忆）
  const chat2 = await fetch(`${baseUrl}/subhuti/api/v1/orchestrate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      message: '我叫什么名字？我的职业是什么？',
      user_id: 'xiaoming_001'
    })
  });
  const data2 = await chat2.json();
  console.log('AI 回复:', data2.data.output);
  console.log('使用了', data2.data.used_memories, '条记忆');
}

chatExample();
```

---

### 示例 2：使用专家插件

```javascript
async function expertExample() {
  const baseUrl = 'http://localhost:8080';
  
  // 1. 查看可用专家
  const list = await fetch(`${baseUrl}/subhuti/api/v1/experts/list`);
  const listData = await list.json();
  console.log('可用专家:', listData.data.plugins.map(p => p.name));
  
  // 2. 激活心理咨询专家
  const activate = await fetch(`${baseUrl}/subhuti/api/v1/experts/activate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ expert_id: 'psychological_counselor' })
  });
  const activateData = await activate.json();
  console.log('激活专家:', activateData.data.expert_name);
  
  // 3. 与专家对话
  const chat = await fetch(`${baseUrl}/subhuti/api/v1/orchestrate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      message: '我最近工作压力很大，经常失眠，怎么办？',
      user_id: 'user_001'
    })
  });
  const chatData = await chat.json();
  console.log('专家回复:', chatData.data.output);
  
  // 4. 停用专家
  const deactivate = await fetch(`${baseUrl}/subhuti/api/v1/experts/deactivate`, {
    method: 'POST'
  });
  const deactivateData = await deactivate.json();
  console.log('已停用专家');
}

expertExample();
```

---

### 示例 3：心灵宫殿操作

```javascript
async function palaceExample() {
  const baseUrl = 'http://localhost:8080';
  
  // 1. 添加一条重要记忆
  const add = await fetch(`${baseUrl}/subhuti/api/v1/memory/add`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      content: '用户对青霉素过敏',
      layer: 'archive',
      tags: ['健康', '过敏']
    })
  });
  const addData = await add.json();
  console.log('添加记忆 ID:', addData.data.memory_id);
  
  // 2. 查看分区统计
  const stats = await fetch(`${baseUrl}/subhuti/api/v1/palace/stats`);
  const statsData = await stats.json();
  console.log('分区统计:', statsData.data.zone_counts);
  
  // 3. 搜索相关记忆
  const search = await fetch(`${baseUrl}/subhuti/api/v1/palace/search`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      query: "过敏",
      limit: 5,
      use_persona_bias: true
    })
  });
  const searchData = await search.json();
  console.log('搜索到', searchData.data.total, '条记忆');
  searchData.data.results.forEach(r => {
    console.log(`  - [${r.zone}] ${r.content.substring(0, 30)}... (score: ${r.score})`);
  });
  
  // 4. 查看人格分区偏好
  const bias = await fetch(`${baseUrl}/subhuti/api/v1/soul/persona/zone-bias`);
  const biasData = await bias.json();
  console.log('人格偏好:', biasData.data);
}

palaceExample();
```

---

### 示例 4：Python 调用

```python
import requests
import json

BASE_URL = "http://localhost:8080"

def chat(message, user_id="user_001"):
    """发送聊天消息"""
    response = requests.post(
        f"{BASE_URL}/subhuti/api/v1/orchestrate",
        json={
            "message": message,
            "user_id": user_id
        }
    )
    return response.json()

def palace_stats():
    """获取心灵宫殿统计"""
    response = requests.get(f"{BASE_URL}/subhuti/api/v1/palace/stats")
    return response.json()

def health_check():
    """健康检查"""
    response = requests.get(f"{BASE_URL}/subhuti/api/v1/health/detailed")
    return response.json()

# 使用示例
if __name__ == "__main__":
    # 健康检查
    health = health_check()
    print(f"系统健康: {health['overall_healthy']}")
    
    # 发送消息
    result = chat("你好，介绍一下你自己")
    print(f"AI: {result['data']['response']}")
    
    # 查看记忆
    stats = palace_stats()
    print(f"记忆总数: {stats['data']['total_count']}")
```

---

## 🎯 Orchestrate 调度 API（多 Agent / 工作流编排）

Subhuti 的核心编排能力：接收用户问题后，通过 **Layer 1 任务理解 → Layer 2 调度决策 → Layer 3 执行监控** 三层流程决定调用哪个专家 / 哪条工作流，并最终生成回答。

HTTP 前缀：`/subhuti/api/v1/orchestrate`

> 所有接口都提供等价的 **`make` 命令**（适合本地调试，直接在项目根目录运行），无需手写 JSON。

---

### 0. 完整编排（一次性跑三层 + LLM）

最常用的接口。内部会依次走：**图路由匹配 → RuleEngine 三层调度 → 命中 blender 专家 → 调 glm-4-flash 生成回答**。

#### 请求

```http
POST /subhuti/api/v1/orchestrate
Content-Type: application/json
```

#### 请求体

```json
{
  "message": "帮我用 Blender 做一个 5 秒的弹跳球动画",
  "user_id": "hezenghui",
  "session_id": "debug-1"
}
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `message` | string | ✅ | 用户输入 |
| `user_id` | string | ❌ | 用户 ID，默认 `debug-user` |
| `session_id` | string | ❌ | 会话 ID，默认 `orch-session-1` |
| `chain` | string[] | ❌ | 强制指定策略链（未指定时由调度器自动决定） |

#### 响应

```json
{
  "success": true,
  "session_id": "debug-1",
  "chain": ["graph:blender_workflow"],
  "expert_chain": ["blender"],
  "expert_outputs": [],
  "output": "（glm-4-flash 生成的完整 Blender 操作说明 + Python bpy 脚本）",
  "tokens": { "input": 128, "output": 512 }
}
```

| 字段 | 说明 |
|------|------|
| `chain` | 实际命中的策略 / 图名（`graph:xxx` 表示走工作流图，`rule_engine:xxx` 表示走 RuleEngine） |
| `expert_chain` | 最终被实际调用到的专家 ID 列表 |
| `output` | LLM 生成的最终回答 |
| `success` | 整次编排是否成功 |

#### ⭐ 你最常用的调试命令（等价 curl）

```bash
make orch-run \
  ORCH_MSG='帮我用 Blender 做一个 5 秒的弹跳球动画' \
  ORCH_USER=hezenghui \
  ORCH_SESSION=debug-1
```

等价 curl：

```bash
curl -X POST http://localhost:8080/subhuti/api/v1/orchestrate \
  -H "Content-Type: application/json" \
  -d '{
    "message": "帮我用 Blender 做一个 5 秒的弹跳球动画",
    "user_id": "hezenghui",
    "session_id": "debug-1"
  }' | python3 -m json.tool
```

---

### 1. 列出所有已注册专家快照（Layer 0）

查看 Orchestrator 当前注册了哪些专家，每个专家有哪些 skills / tags。**不打网络，秒回**。

#### 请求 / 响应

```http
GET /subhuti/api/v1/orchestrate/experts
```

```json
{
  "success": true,
  "total": 1,
  "data": [
    {
      "id": "blender",
      "name": "Blender 动画专家",
      "tags": ["blender", "3D", "动画", "建模", "渲染", "材质", "节点", "粒子"],
      "skills": [
        { "id": "blender-chat", "name": "聊天对话", "parameters": ["问题"] },
        { "id": "blender-modeling", "name": "3D建模", "parameters": ["模型类型", "风格"] },
        { "id": "blender-animation", "name": "动画制作", "parameters": ["动画类型", "时长"] },
        { "id": "blender-render", "name": "渲染输出", "parameters": ["渲染器", "分辨率", "采样"] }
      ]
    }
  ]
}
```

#### Make 调试命令

```bash
make orch-experts
```

---

### 2. Layer 1 任务分析（`analyze_task`）

只调第一层：把用户问题解析成 `TaskProfile`（领域标签、任务类型、主谓宾结构化分解）。**不打网络，秒回**。

#### 请求

```http
POST /subhuti/api/v1/orchestrate/analyze
Content-Type: application/json
```

```json
{ "message": "帮我用 Blender 做一个 5 秒的弹跳球动画" }
```

#### 响应

```json
{
  "success": true,
  "suggested_strategy": "SimpleDispatch",
  "profile": {
    "task_type": "chat",
    "domain_tags": ["blender"],
    "subject": "帮我用",
    "predicate": "Blender",
    "object": "做一个"
  }
}
```

| 字段 | 说明 |
|------|------|
| `suggested_strategy` | RuleEngine 建议使用的调度策略（SimpleDispatch / ParallelDispatch / GraphFirst ...） |
| `profile.domain_tags` | 提取到的领域标签（命中 blender 就会填 `["blender"]`） |
| `profile.task_type` | `chat` / `task` / `code` / `review` 等，由 `analysis_rule` 产出 |
| `profile.subject/predicate/object` | 中文 SVO 主谓宾初步切分（用于后续语义路由打分） |

#### Make 调试命令

```bash
make orch-analyze ORCH_MSG='帮我用 Blender 做一个 5 秒的弹跳球动画'
```

---

### 3. Layer 1 + Layer 2 专家匹配（`match_expert`）

先 analyze 拿 TaskProfile，再 `decide_strategy` 按关键词打分 + 过滤 + 限量，最后返回命中的专家列表（含 skills、tags）。**不打网络，秒回**。

#### 请求

```http
POST /subhuti/api/v1/orchestrate/match
Content-Type: application/json
```

```json
{ "message": "帮我用 Blender 做一个 5 秒的弹跳球动画" }
```

#### 响应

```json
{
  "success": true,
  "total": 1,
  "matches": [
    {
      "id": "blender",
      "name": "Blender 动画专家",
      "tags": ["blender", "3D", "动画", ...],
      "skills": [
        { "id": "blender-chat", "name": "聊天对话", "parameters": ["问题"] },
        ...
      ]
    }
  ]
}
```

返回顺序 = RuleEngine `DispatchPlan.steps` 的顺序（已经过打分和 Top-K 过滤）。

#### Make 调试命令

```bash
make orch-match ORCH_MSG='帮我用 Blender 做一个 5 秒的弹跳球动画'
```

---

### 4. 全链路一键调试

等价顺序跑：① `orch-experts` → ② `orch-analyze` → ③ `orch-match` → ④ `orch-run`。前三层失败时可以快速看到哪一层先出错，避免白跑 LLM。

```bash
make orch-all ORCH_MSG='帮我用 Blender 做一个 5 秒的弹跳球动画'
```

可选参数（所有 orch-xxx 命令都支持）：

| 参数 | 默认值 | 说明 |
|---|---|---|
| `ORCH_MSG='你的问题'` | `帮我用 Blender 做一个 5 秒的弹跳球动画` | 用户输入 |
| `ORCH_USER=<id>` | `debug-user` | user_id |
| `ORCH_SESSION=<id>` | `orch-session-1` | session_id |
| `HTTP_ADDR=<url>` | `http://localhost:8080` | 服务地址 |

---

### 典型调试流程（推荐）

```bash
# Step 1: 启动服务（首次）
make serve-debug

# Step 2: 先看专家库有啥
make orch-experts

# Step 3: 只调"任务分析"，避免浪费 token
make orch-analyze ORCH_MSG='渲染一张产品 4K 360° 环绕镜头图'

# Step 4: 看专家匹配对不对
make orch-match   ORCH_MSG='渲染一张产品 4K 360° 环绕镜头图'

# Step 5: 确认前面 OK 再跑完整编排（真正打 LLM）
make orch-run     ORCH_MSG='渲染一张产品 4K 360° 环绕镜头图' ORCH_USER=hezenghui ORCH_SESSION=prod-1

# 或者一键跑完整链路
make orch-all ORCH_MSG='帮我用 Blender 做一个 5 秒的弹跳球动画'
```

---

## ❓ 常见问题

### Q: API 返回 500 错误怎么办？

**A**: 查看响应中的 trace_id，然后使用 Trace API 查看详细错误：

```bash
curl http://localhost:8080/subhuti/api/v1/trace/{trace_id}
```

### Q: 如何启用流式输出？

**A**: 调用 `/orchestrate` 时带上请求头 `Accept: text/event-stream`（响应为 SSE 流，逐 delta 下发）。

### Q: 心灵宫殿和记忆系统有什么区别？

**A**:
- **记忆系统**：底层存储，简单的增删改查
- **心灵宫殿**：上层封装，包含分区、重要性、遗忘、联想激活、人格影响等高级功能

### Q: orchestrate 命中 blender_workflow 的条件是什么？

**A**: Orchestrator.dispatch() 按以下优先级决定路径：

1. 语义路由（SemanticRouter）— 当前默认 disabled
2. **图匹配 `graph_registry.find_matching_graph(input)`** — 当前注册 1 个图（`blender_workflow`）时，任意输入都会命中 blender；注册图≥2 个时，会按"图名 contains 关键词 / 分类关键词"命中。
3. RuleEngine 三层调度（analyze → decide_strategy → execute） — 图匹配失败才走到这层。

对应代码：`crates/subhuti-core/src/orchestrator/mod.rs#L396-L475`

---

## 📚 更多资源

- [Quickstart 快速上手](QUICKSTART.md) — 5 分钟体验 + Orchestrate Make 速查表
- [架构详解](ARCHITECTURE.md) — 深入理解框架
- [用户指南](USER_GUIDE.md) — 完整功能说明
- [调试工具指南](DEBUG_TOOLS_GUIDE.md) — 开发调试
- [调度器设计](ORCHESTRATOR_DESIGN.md) — Layer 1/2/3 三层调度原理

---

*本文档对应 Subhuti v1.0 API*
