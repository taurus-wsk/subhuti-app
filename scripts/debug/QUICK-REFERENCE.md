# Subhuti 调试快速参考（终端版）

## 🎯 核心命令

### 1. Orchestrate 调试
```bash
# 调用编排接口（debug 模式，日志更详细）
make orchestrate-debug MESSAGE="测试编排流程"

# 指定策略链
make orchestrate-debug MESSAGE="测试" CHAIN=pipeline

# 指定用户
make orchestrate-debug MESSAGE="测试" USER=test_user
```

### 2. 日志查询
```bash
# 查询日志 API
curl "http://localhost:8080/subhuti/api/v1/logs?keyword=orchestrate&limit=10"

# 直接查看日志文件
tail -f ./logs/subhuti.log.*

# 搜索特定 Session
grep "session_id" ./logs/subhuti.log.* | python3 -m json.tool
```

### 3. Trace 重建
```bash
# 从日志重建 Trace 调用链
./scripts/debug/rebuild-trace-from-log.sh <trace_id>

# 或使用 Makefile
make trace ID=<trace_id>
```

### 4. Session 查询
```bash
# 查询 Session
curl "http://localhost:8080/subhuti/api/v1/sessions/<session_id>"

# 列出所有 Session
curl "http://localhost:8080/subhuti/api/v1/sessions"
```

### 5. 完整验证脚本
```bash
# 自动调用 + 验证日志 + 验证 Session
./scripts/debug/verify-orchestrate.sh "测试消息"
```

---

## 📊 日志文件结构

```
logs/
├── subhuti.log              # 当前日志
├── subhuti.log.2026-07-02   # 历史日志（按天轮转）
└── subhuti.log.2026-07-01
```

每条日志包含：
- `timestamp`: 时间戳
- `level`: 日志级别 (INFO/DEBUG/WARN/ERROR)
- `fields.message`: 日志消息
- `span`: 当前 span 信息（trace_id, user_id, session_id 等）
- `spans`: 完整的调用链上下文

---

## 🔍 常用调试技巧

### 查看实时日志
```bash
tail -f ./logs/subhuti.log.*
```

### 过滤特定 Trace
```bash
grep "trace_id: xxx" ./logs/subhuti.log.* | python3 -m json.tool
```

### 统计请求数
```bash
grep -c "POST /subhuti/api/v1/orchestrate" ./logs/subhuti.log.*
```

### 查看错误日志
```bash
grep '"level":"ERROR"' ./logs/subhuti.log.*
```

### 提取 Session 历史
```bash
grep "session_id" ./logs/subhuti.log.* | \
  python3 -c "import sys, json; [print(json.loads(l)['fields']['message']) for l in sys.stdin]"
```

---

## 🛠️ 服务管理

```bash
# 启动服务（release 模式）
./target/release/subhuti serve

# 启动服务（debug 模式，更多日志）
RUST_LOG=debug ./target/release/subhuti serve

# 停止服务
pkill -f "subhuti serve"

# 查看服务状态
ps aux | grep "subhuti serve"
```

---

## 📝 标准调试流程

1. **启动服务**
   ```bash
   ./target/release/subhuti serve
   ```

2. **发送测试请求**
   ```bash
   make orchestrate-debug MESSAGE="测试消息"
   ```

3. **查看日志**
   ```bash
   tail -20 ./logs/subhuti.log.*
   ```

4. **重建 Trace**
   ```bash
   ./scripts/debug/rebuild-trace-from-log.sh <trace_id>
   ```

5. **验证 Session**
   ```bash
   curl "http://localhost:8080/subhuti/api/v1/sessions/<session_id>"
   ```

---

## 💡 关键提示

- ✅ **所有调试信息都在日志文件中**，无需额外的存储
- ✅ **Trace 完全从日志重建**，使用 `tracing::info_span!` 自动记录
- ✅ **日志按天轮转**，历史数据永久保存
- ✅ **使用 `RUST_LOG=debug`** 可以看到更详细的调用链
- ✅ **使用 `make orchestrate-debug`** 而不是 `make orchestrate`（前者日志更详细）

---

## 🎯 调试工具总结

| 工具 | 用途 | 命令示例 |
|------|------|---------|
| **日志查询** | 搜索日志 | `curl /logs?keyword=xxx` |
| **Trace 重建** | 重建调用链 | `./scripts/debug/rebuild-trace-from-log.sh <id>` |
| **Session 查询** | 查看会话历史 | `curl /sessions/<id>` |
| **验证脚本** | 自动验证 | `./scripts/debug/verify-orchestrate.sh <msg>` |
| **Makefile** | 统一入口 | `make orchestrate-debug MESSAGE=xxx` |

---

**记住：一切靠终端，日志就是真相！** 🔍
