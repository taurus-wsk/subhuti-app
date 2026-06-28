# Subhuti 线上调试指南

> **版本**: v1.0  
> **日期**: 2026-06-28  
> **适用场景**: Docker 容器内调试、线上问题排查

---

## 📋 目录

- [调试方法概览](#调试方法概览)
- [方法 1：日志调试](#方法-1日志调试)
- [方法 2：Trace 调试](#方法-2trace-调试)
- [方法 3：健康检查](#方法-3健康检查)
- [完整调试流程](#完整调试流程)
- [常见问题排查](#常见问题排查)

---

## 🔍 调试方法概览

Subhuti 提供三种线上调试方法：

| 方法 | 用途 | 复杂度 | 信息量 |
|------|------|--------|--------|
| **日志调试** | 查看应用运行日志 | ⭐ | 中 |
| **Trace 调试** | 查看完整请求链路 | ⭐⭐ | 高 |
| **健康检查** | 快速诊断服务状态 | ⭐ | 低 |

**推荐组合**：日志 + Trace

---

## 方法 1：日志调试

### 1.1 查看实时日志

```bash
# Docker 容器实时日志
docker logs -f subhuti-app

# 查看最近 100 条
docker logs --tail 100 subhuti-app

# 查看最近 10 分钟的日志
docker logs --since 10m subhuti-app
```text

### 1.2 通过 API 查询日志

```bash
# 查看所有日志（默认 50 条）
curl http://localhost:8080/subhuti/api/v1/logs

# 查看错误日志
curl http://localhost:8080/subhuti/api/v1/logs?level=ERROR

# 查看警告和错误
curl http://localhost:8080/subhuti/api/v1/logs?level=WARN

# 时间范围查询
curl http://localhost:8080/subhuti/api/v1/logs?start=2026-06-28T00:00:00&end=2026-06-28T23:59:59

# 分页查询
curl http://localhost:8080/subhuti/api/v1/logs?page=1&page_size=100

# 关键词搜索
curl http://localhost:8080/subhuti/api/v1/logs?keyword=error
```text

### 1.3 日志级别

- `ERROR` - 错误
- `WARN` - 警告
- `INFO` - 信息（默认）
- `DEBUG` - 调试
- `TRACE` - 跟踪

### 1.4 日志位置

```bash
# 容器内日志文件
docker exec subhuti-app ls -la /app/logs/

# 复制日志到本地
docker cp subhuti-app:/app/logs/ ./logs/

# 查看日志文件
docker exec subhuti-app tail -f /app/logs/subhuti.log
```text

---

## 方法 2：Trace 调试 ⭐⭐⭐⭐⭐

Trace 系统记录每个请求的**完整调用链**，是线上调试的利器。

### 2.1 获取 Trace 列表

```bash
# 获取所有 Trace 摘要
curl http://localhost:8080/subhuti/api/v1/traces | python3 -m json.tool

# 输出示例：
{
    "success": true,
    "data": [
        {
            "trace_id": "f1003e43-1d19-49aa-811b-d07b5bc12536",
            "input": "你好",
            "output": "你好呀😊！请问有什么我可以帮到你的吗？",
            "duration_ms": 3684,
            "expert_id": null,
            "matched_skill": "default_chat",
            "tools_used": [],
            "token_usage": {
                "prompt_tokens": 51,
                "completion_tokens": 87,
                "total_tokens": 138
            },
            "span_count": 3,
            "status": "Success"
        }
    ],
    "total": 1
}
```text

### 2.2 查看 Trace 详情

```bash
# 获取单个 Trace 的完整信息
curl http://localhost:8080/subhuti/api/v1/traces/<trace_id> | python3 -m json.tool

# 示例：
curl http://localhost:8080/subhuti/api/v1/traces/f1003e43-1d19-49aa-811b-d07b5bc12536 | python3 -m json.tool
```text

**返回内容**：
- 完整的输入输出
- 所有 Span 的详细信息
- 每个步骤的耗时
- Token 消耗
- 错误信息（如果有）

### 2.3 查看调用链树

```bash
# 获取 Span 树结构（可视化调用链）
curl http://localhost:8080/subhuti/api/v1/traces/<trace_id>/tree | python3 -m json.tool

# 示例：
curl http://localhost:8080/subhuti/api/v1/traces/f1003e43-1d19-49aa-811b-d07b5bc12536/tree | python3 -m json.tool
```text

**Span 树结构**：
```text
Trace (request_id)
  ├── Span: request (请求开始)
  ├── Span: skill_match (Skill 匹配)
  │   └── matched: default_chat
  ├── Span: llm_call (LLM 调用)
  │   ├── prompt_tokens: 51
  │   └── completion_tokens: 87
  └── Span: response (响应生成)
```text

### 2.4 Trace 数据结构

每个 Trace 包含：

```json
{
  "id": "trace_uuid",
  "user_id": "user123",
  "session_id": "session456",
  "input": "用户输入",
  "output": "AI 输出",
  "started_at": "2026-06-28T10:00:00Z",
  "ended_at": "2026-06-28T10:00:03Z",
  "total_duration_ms": 3684,
  "expert_id": null,
  "matched_skill": "default_chat",
  "tools_used": ["weather", "search"],
  "token_usage": {
    "prompt_tokens": 51,
    "completion_tokens": 87,
    "total_tokens": 138
  },
  "spans": {
    "span_id_1": {
      "id": "span_id_1",
      "kind": "skill_match",
      "name": "skill_match",
      "start_time_ms": 0,
      "duration_ms": 150,
      "status": "Success",
      "input": {...},
      "output": {...},
      "error": null
    }
  }
}
```text

### 2.5 Span 类型

| Span 类型 | 说明 |
|----------|------|
| `request` | 请求开始 |
| `skill_match` | Skill 匹配 |
| `skill_execute` | Skill 执行 |
| `memory_search` | 记忆检索 |
| `tool_call` | 工具调用 |
| `llm_call` | LLM 调用 |
| `hook_execute` | 钩子执行 |
| `expert_switch` | 专家切换 |
| `planning_execute` | 规划执行 |
| `response` | 响应生成 |

### 2.6 Trace 持久化

Trace 会自动持久化到磁盘，重启后不丢失：

```bash
# 查看 Trace 文件
docker exec subhuti-app ls -la /app/traces/

# 复制 Trace 到本地
docker cp subhuti-app:/app/traces/ ./traces/
```text

---

## 方法 3：健康检查

### 3.1 基础健康检查

```bash
# 快速检查服务状态
curl http://localhost:8080/subhuti/api/v1/health

# 输出：
{
    "status": "ok",
    "timestamp": "2026-06-28 16:12:05"
}
```text

### 3.2 详细健康检查

```bash
# 详细状态（包含各组件状态）
curl http://localhost:8080/subhuti/api/v1/health/detailed | python3 -m json.tool

# 输出：
{
    "status": "ok",
    "timestamp": "2026-06-28 16:12:05",
    "components": [
        {
            "name": "database",
            "healthy": true,
            "details": "Connected to localhost:5432"
        },
        {
            "name": "llm",
            "healthy": true,
            "details": "Ollama available"
        }
    ]
}
```text

---

## 🎯 完整调试流程

### 场景 1：服务异常

```bash
# 1. 检查服务状态
curl http://localhost:8080/subhuti/api/v1/health

# 2. 查看错误日志
curl http://localhost:8080/subhuti/api/v1/logs?level=ERROR&limit=50

# 3. 查看实时日志
docker logs -f subhuti-app

# 4. 查看详细健康检查
curl http://localhost:8080/subhuti/api/v1/health/detailed
```text

### 场景 2：请求响应慢

```bash
# 1. 获取 Trace 列表（按时间排序）
curl http://localhost:8080/subhuti/api/v1/traces | python3 -c "
import sys, json
traces = json.load(sys.stdin)['data']
for t in sorted(traces, key=lambda x: x['duration_ms'], reverse=True)[:5]:
    print(f\"{t['trace_id']}: {t['duration_ms']}ms - {t['input'][:50]}\")
"

# 2. 查看最慢的请求详情
curl http://localhost:8080/subhuti/api/v1/traces/<slow_trace_id> | python3 -m json.tool

# 3. 查看调用链，找出瓶颈
curl http://localhost:8080/subhuti/api/v1/traces/<slow_trace_id>/tree | python3 -m json.tool
```text

### 场景 3：AI 回答错误

```bash
# 1. 获取最近的 Trace
curl http://localhost:8080/subhuti/api/v1/traces | python3 -m json.tool

# 2. 找到对应的 Trace ID
# 根据 input 找到你想查看的请求

# 3. 查看完整调用链
curl http://localhost:8080/subhuti/api/v1/traces/<trace_id> | python3 -m json.tool

# 4. 分析：
# - matched_skill: 匹配了哪个 Skill
# - tools_used: 使用了哪些工具
# - token_usage: Token 消耗
# - spans: 每个步骤的输入输出
```text

### 场景 4：工具调用失败

```bash
# 1. 查看错误日志
curl http://localhost:8080/subhuti/api/v1/logs?level=ERROR&keyword=tool

# 2. 获取包含工具调用的 Trace
curl http://localhost:8080/subhuti/api/v1/traces | python3 -c "
import sys, json
traces = json.load(sys.stdin)['data']
for t in traces:
    if t['tools_used']:
        print(f\"{t['trace_id']}: tools={t['tools_used']}\")
"

# 3. 查看失败的 Trace
curl http://localhost:8080/subhuti/api/v1/traces/<trace_id> | python3 -c "
import sys, json
trace = json.load(sys.stdin)['data']
for span_id, span in trace['spans'].items():
    if span.get('error'):
        print(f\"Span {span_id} ({span['kind']}): {span['error']}\")
"
```text

---

## 🔧 常见问题排查

### Q1: 服务无法启动

```bash
# 1. 查看日志
docker logs subhuti-app

# 2. 检查端口占用
docker exec subhuti-app netstat -tlnp | grep 8080

# 3. 检查数据库连接
curl http://localhost:8080/subhuti/api/v1/health/detailed
```text

### Q2: LLM 调用失败

```bash
# 1. 查看错误日志
curl http://localhost:8080/subhuti/api/v1/logs?level=ERROR&keyword=llm

# 2. 查看包含 LLM 调用的 Trace
curl http://localhost:8080/subhuti/api/v1/traces | python3 -c "
import sys, json
traces = json.load(sys.stdin)['data']
for t in traces:
    if t.get('token_usage'):
        print(f\"{t['trace_id']}: tokens={t['token_usage']['total_tokens']}\")
"

# 3. 查看失败的 LLM 调用
curl http://localhost:8080/subhuti/api/v1/traces/<trace_id> | python3 -c "
import sys, json
trace = json.load(sys.stdin)['data']
for span_id, span in trace['spans'].items():
    if span['kind'] == 'llm_call' and span.get('error'):
        print(f\"LLM Error: {span['error']}\")
"
```text

### Q3: Skill 匹配错误

```bash
# 1. 查看 Trace 中的 Skill 匹配
curl http://localhost:8080/subhuti/api/v1/traces | python3 -c "
import sys, json
traces = json.load(sys.stdin)['data']
for t in traces:
    print(f\"{t['trace_id']}: skill={t['matched_skill']}\")
"

# 2. 查看匹配详情
curl http://localhost:8080/subhuti/api/v1/traces/<trace_id> | python3 -c "
import sys, json
trace = json.load(sys.stdin)['data']
for span_id, span in trace['spans'].items():
    if span['kind'] == 'skill_match':
        print(f\"Matched: {span['output']}\")
"
```text

### Q4: 数据库连接失败

```bash
# 1. 查看详细健康检查
curl http://localhost:8080/subhuti/api/v1/health/detailed | python3 -c "
import sys, json
health = json.load(sys.stdin)
for comp in health['components']:
    print(f\"{comp['name']}: {comp['healthy']} - {comp['details']}\")
"

# 2. 查看数据库相关日志
curl http://localhost:8080/subhuti/api/v1/logs?keyword=database
```text

---

## 📊 调试技巧

### 技巧 1：使用 jq 美化输出

```bash
# 安装 jq
brew install jq  # macOS
apt-get install jq  # Linux

# 使用 jq
curl http://localhost:8080/subhuti/api/v1/traces | jq .
```text

### 技巧 2：实时监控 Trace

```bash
# 每 5 秒刷新一次 Trace 列表
watch -n 5 'curl -s http://localhost:8080/subhuti/api/v1/traces | jq ".total"'
```text

### 技巧 3：导出 Trace 分析

```bash
# 导出所有 Trace
curl http://localhost:8080/subhuti/api/v1/traces > traces.json

# 分析耗时分布
cat traces.json | python3 -c "
import sys, json
traces = json.load(sys.stdin)['data']
durations = [t['duration_ms'] for t in traces]
print(f\"平均耗时: {sum(durations)/len(durations):.0f}ms\")
print(f\"最慢: {max(durations)}ms\")
print(f\"最快: {min(durations)}ms\")
"
```text

### 技巧 4：查找特定请求

```bash
# 根据输入内容查找 Trace
curl http://localhost:8080/subhuti/api/v1/traces | python3 -c "
import sys, json
traces = json.load(sys.stdin)['data']
keyword = '你好'
for t in traces:
    if keyword in t['input']:
        print(f\"{t['trace_id']}: {t['input'][:50]}\")
"
```text

---

## 🎓 最佳实践

1. **日常监控**
   - 使用 `docker logs -f` 实时查看
   - 定期检查 `/health` 端点

2. **问题排查**
   - 先看日志（快速定位）
   - 再看 Trace（详细分析）
   - 最后看调用链树（找出瓶颈）

3. **性能优化**
   - 定期分析 Trace 耗时
   - 关注 Token 消耗
   - 优化慢的 Span

4. **日志管理**
   - 生产环境使用 INFO 级别
   - 调试时临时切换到 DEBUG
   - 定期清理旧日志

---

## 📚 相关文档

- [日志系统架构](../../crates/subhuti/src/observe/trace.rs)
- [Trace 系统代码](../../crates/subhuti/src/observe/trace.rs)
- [标准流程手册](../releases/STANDARD_WORKFLOW.md)

---

**最后更新**: 2026-06-28  
**维护者**: Subhuti Team
