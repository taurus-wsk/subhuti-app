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
POST /subhuti/api/v1/chat
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
curl -X POST http://localhost:8080/subhuti/api/v1/chat \
  -H "Content-Type: application/json" \
  -d '{
    "message": "你好，今天天气怎么样？",
    "user_id": "user_001"
  }'
```

#### JavaScript 示例

```javascript
const response = await fetch('http://localhost:8080/subhuti/api/v1/chat', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    message: '你好，今天天气怎么样？',
    user_id: 'user_001'
  })
});

const data = await response.json();
console.log(data.data.response);
```

---

### 2. 流式聊天

支持 Server-Sent Events (SSE) 流式输出。

#### 请求

```http
POST /subhuti/api/v1/chat/stream
Content-Type: application/json
Accept: text/event-stream
```

#### 请求体

```json
{
  "message": "写一个关于秋天的故事",
  "user_id": "user_001",
  "stream": true
}
```

#### 响应流

```
data: {"type": "token", "content": "秋", "index": 0}

data: {"type": "token", "content": "天", "index": 1}

data: {"type": "token", "content": "来", "index": 2}

...

data: {"type": "done", "content": "", "trace_id": "trace_abc"}
```

#### JavaScript 示例

```javascript
const eventSource = new EventSource('/subhuti/api/v1/chat/stream');

let fullText = '';

eventSource.onmessage = (event) => {
  const data = JSON.parse(event.data);
  
  if (data.type === 'token') {
    fullText += data.content;
    console.log('收到 token:', data.content);
  } else if (data.type === 'done') {
    console.log('完成！trace_id:', data.trace_id);
    eventSource.close();
  }
};

eventSource.onerror = (error) => {
  console.error('流错误:', error);
  eventSource.close();
};
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
  const chat1 = await fetch(`${baseUrl}/subhuti/api/v1/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      message: '你好，我叫小明，是一名程序员',
      user_id: 'xiaoming_001'
    })
  });
  const data1 = await chat1.json();
  console.log('AI 回复:', data1.data.response);
  console.log('Trace ID:', data1.data.trace_id);
  
  // 3. 查看心灵宫殿状态
  const stats = await fetch(`${baseUrl}/subhuti/api/v1/palace/stats`);
  const statsData = await stats.json();
  console.log('记忆总数:', statsData.data.total_count);
  
  // 4. 发送第二条消息（测试记忆）
  const chat2 = await fetch(`${baseUrl}/subhuti/api/v1/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      message: '我叫什么名字？我的职业是什么？',
      user_id: 'xiaoming_001'
    })
  });
  const data2 = await chat2.json();
  console.log('AI 回复:', data2.data.response);
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
  const chat = await fetch(`${baseUrl}/subhuti/api/v1/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      message: '我最近工作压力很大，经常失眠，怎么办？',
      user_id: 'user_001'
    })
  });
  const chatData = await chat.json();
  console.log('专家回复:', chatData.data.response);
  
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
        f"{BASE_URL}/subhuti/api/v1/chat",
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

## ❓ 常见问题

### Q: API 返回 500 错误怎么办？

**A**: 查看响应中的 trace_id，然后使用 Trace API 查看详细错误：

```bash
curl http://localhost:8080/subhuti/api/v1/trace/{trace_id}
```

### Q: 如何启用流式输出？

**A**: 在请求中设置 `stream: true`，或使用 `/chat/stream` 端点。

### Q: 心灵宫殿和记忆系统有什么区别？

**A**:
- **记忆系统**：底层存储，简单的增删改查
- **心灵宫殿**：上层封装，包含分区、重要性、遗忘、联想激活、人格影响等高级功能

---

## 📚 更多资源

- [Quickstart 快速上手](QUICKSTART.md) - 5 分钟体验
- [架构详解](ARCHITECTURE.md) - 深入理解框架
- [用户指南](USER_GUIDE.md) - 完整功能说明
- [调试工具指南](DEBUG_TOOLS_GUIDE.md) - 开发调试

---

*本文档对应 Subhuti v1.0 API*
