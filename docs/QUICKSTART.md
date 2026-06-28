# Quickstart: 5 分钟快速上手

> 从零开始，快速体验 Subhuti AI Agent 框架的核心能力

---

## ⏱️ 预计时间：5 分钟

| 步骤 | 内容 | 时间 |
|------|------|------|
| 1 | 环境准备 | 1 分钟 |
| 2 | 启动服务 | 1 分钟 |
| 3 | 发送第一条消息 | 1 分钟 |
| 4 | 体验心灵宫殿 | 1 分钟 |
| 5 | 查看系统状态 | 1 分钟 |

---

## 🚀 第一步：环境准备（1 分钟）

### 前置要求

确保你的系统已安装：

```bash
# Rust 工具链（必需）
rustc --version   # 建议 1.75+
cargo --version

# Docker（可选，用于 PostgreSQL + pgvector）
docker --version
```

### 克隆项目

```bash
git clone <your-repo-url> subhuti-app
cd subhuti-app
```

---

## 🏃 第二步：启动服务（1 分钟）

### 方式一：快速启动（无需数据库）

```bash
# 编译并启动 HTTP 服务器
cargo run --bin http_server
```

等待输出类似：
```
🚀 HTTP server running on http://0.0.0.0:8080
```

### 方式二：完整启动（含 PostgreSQL + pgvector）

```bash
# 1. 启动 PostgreSQL（已配置好 pgvector）
docker start pgvector

# 2. 配置数据库连接
export DATABASE_URL=postgres://postgres:123456@localhost:5432/postgres

# 3. 启动服务
cargo run --bin http_server
```

---

## 💬 第三步：发送第一条消息（1 分钟）

服务启动后，打开浏览器访问测试页面：

```
http://localhost:8080/
```

或者使用 curl 发送消息：

```bash
curl -X POST http://localhost:8080/subhuti/api/v1/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "你好，我是小明，今天天气真好！", "user_id": "test_user_001"}'
```

### 预期响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "response": "你好小明！今天天气确实不错，适合出去走走。有什么我可以帮助你的吗？",
    "trace_id": "trace_abc123",
    "matched_skill": "default_chat",
    "expert_id": null
  }
}
```

### 发生了什么？

当你发送消息时，Subhuti 做了这些事情：

```
用户消息 → Skill 匹配 → 记忆检索 → LLM 生成 → 记忆存储 → 心灵更新 → 返回响应
```

---

## 🏛️ 第四步：体验心灵宫殿（1 分钟）

### 查看心灵宫殿统计

```bash
curl http://localhost:8080/subhuti/api/v1/palace/stats | python3 -m json.tool
```

### 预期输出

```json
{
  "total_count": 1,
  "zone_counts": {
    "DailyChat": 1,
    "ProfessionalKnowledge": 0,
    "Emotional": 0,
    "TaskProgress": 0,
    "CreativeIdeas": 0,
    "Default": 0
  },
  "importance_distribution": {
    "Trivial": 0,
    "Normal": 1,
    "Important": 0,
    "Core": 0
  }
}
```

### 搜索记忆

```bash
curl -X POST http://localhost:8080/subhuti/api/v1/palace/search \
  -H "Content-Type: application/json" \
  -d '{"query": "天气", "limit": 5}'
```

### 执行遗忘清理

```bash
curl -X POST http://localhost:8080/subhuti/api/v1/palace/forget
```

---

## 🩺 第五步：查看系统状态（1 分钟）

### 健康检查

```bash
# 简单健康检查
curl http://localhost:8080/subhuti/api/v1/health

# 详细健康检查
curl http://localhost:8080/subhuti/api/v1/health/detailed | python3 -m json.tool
```

### 预期输出

```
{
  "healthy": true,
  "components": [
    {
      "name": "MemoryPalace",
      "healthy": true,
      "optional": false,
      "details": { ... }
    },
    {
      "name": "Database",
      "healthy": false,
      "optional": true,
      "details": { "reason": "Not configured (optional component)" }
    },
    ...
  ]
}
```

### 查看测试页面

在浏览器中打开：

```
http://localhost:8080/
```

你会看到一个完整的测试界面，可以测试：
- 聊天对话
- 心灵宫殿
- 专家插件
- 系统健康检查
- 记忆管理

---

## 🎯 下一步：深入探索

恭喜！你已经完成了快速上手。接下来可以探索：

### 1. 体验专家插件

```bash
# 查看可用插件
curl http://localhost:8080/subhuti/api/v1/experts/list

# 激活心理咨询专家
curl -X POST http://localhost:8080/subhuti/api/v1/experts/activate \
  -H "Content-Type: application/json" \
  -d '{"expert_id": "psychological_counselor"}'
```

### 2. 开发第一个 Skill

查看 [Skill 开发指南](SKILL_GUIDE.md)

### 3. 开发专家插件

查看 [专家插件开发指南](EXPERT_PLUGIN_GUIDE.md)

### 4. 了解心灵宫殿

查看 [心灵宫殿详解](SOUL_PALACE.md)

---

## ❓ 常见问题

### Q: 启动报错 "Failed to connect to LLM"

**A**: 需要配置 LLM API Key。设置环境变量：

```bash
export DOUBAO_API_KEY=your_api_key_here
```

或者使用 Ollama 本地模型：

```bash
export LLM_PROVIDER=ollama
export OLLAMA_BASE_URL=http://localhost:11434
```

### Q: 可以不用数据库吗？

**A**: 可以！框架默认使用内存存储，适合快速体验。生产环境建议配置 PostgreSQL + pgvector。

### Q: 支持哪些 LLM？

**A**: 目前支持：
- 豆包（Doubao）- 默认
- Ollama - 本地模型
- OpenAI 兼容接口

---

## 📚 更多资源

- [完整用户指南](USER_GUIDE.md) - 详细功能说明
- [架构详解](ARCHITECTURE.md) - 深入理解框架设计
- [调试工具指南](DEBUG_TOOLS_GUIDE.md) - 开发调试利器
- [API 文档](API_REFERENCE.md) - 完整 API 参考

---

**🎉 恭喜完成 Quickstart！** 你已经体验了 Subhuti 框架的核心能力。继续探索，发现更多可能！
