# Subhuti 文档中心

> 轻量级 Rust AI Agent 框架 | 心灵层动态角色养成 | 专家插件生态

---

## 📚 文档导航

### 🚀 快速开始

| 文档 | 说明 | 阅读时间 |
|------|------|----------|
| [Quickstart 快速上手](QUICKSTART.md) | 5 分钟从零开始体验 | ⏱️ 5 分钟 |
| [API 使用教程](API_TUTORIAL.md) | 手把手教你用所有 API | ⏱️ 20 分钟 |

### 🏗️ 架构设计

| 文档 | 说明 | 深度 |
|------|------|------|
| [架构与流程详解](ARCHITECTURE.md) | 深入理解框架设计和运行机制 | ⭐⭐⭐⭐⭐ |
| [心灵宫殿详解](#) | 记忆+人格的统一系统（待补充） | ⭐⭐⭐⭐ |
| [专家插件开发指南](#) | 开发你的第一个专家插件（待补充） | ⭐⭐⭐ |

### 📖 完整参考

| 文档 | 说明 |
|------|------|
| [用户指南](USER_GUIDE.md) | 完整功能说明 |
| [调试工具指南](DEBUG_TOOLS_GUIDE.md) | 开发调试利器 |
| [调试工具实践总结](DEBUG_TOOLS_SUMMARY.md) | 调试经验总结 |

### 📊 测试报告

| 文档 | 说明 |
|------|------|
| [集成测试报告](INTEGRATION_TEST_REPORT.md) | 完整集成测试结果 |

### 🧪 测试体系

| 测试类型 | 数量 | 运行命令 |
|----------|------|----------|
| 单元测试 | 49 个 | `cargo test -p subhuti` |
| 集成测试 | 10 个 | `cargo test -p subhuti --test integration_test -- --nocapture` |
| 调试工具测试 | 9 个 | `cargo test -p subhuti --test test_debug_tools -- --nocapture` |
| 性能基准测试 | 10 个 | `cargo test -p subhuti --test performance_test -- --nocapture` |
| LLM 同步测试 | 1 个 | `cargo run --bin sync_test` |

### 🛣️ 路线图

| 文档 | 说明 |
|------|------|
| [未来路线图](ROADMAP.md) | 框架发展规划和优先级 |

---

## 🎯 框架特色

### 🏛️ 心灵宫殿
记忆与心灵的统一体，6 大分区、4 级重要性、遗忘机制、联想激活

### 🧠 动态人格
大五人格模型，双轨驱动演化，越用越懂你

### 🔌 专家插件
插件化领域能力注入，角色+技能+知识库一体化

### 🛠️ 调试友好
完整的诊断工具、健康检查、性能分析

---

## 📂 项目结构

```
subhuti-app/
├── crates/
│   └── subhuti/              # 核心框架
│       ├── src/
│       │   ├── soul/         # 心灵层 + 心灵宫殿
│       │   ├── skill/        # Skill 系统
│       │   ├── expert/       # 专家插件
│       │   ├── flow/         # Flow 流程层
│       │   ├── memory/       # 记忆系统
│       │   ├── debug.rs      # 调试工具
│       │   └── lib.rs        # 主入口
│       └── tests/            # 集成测试 + 性能测试
│
├── src/
│   └── bin/http_server/      # HTTP 服务器
│
├── docs/                     # 文档（就是这里！）
│   ├── QUICKSTART.md         # 快速上手
│   ├── ARCHITECTURE.md       # 架构详解
│   ├── API_TUTORIAL.md       # API 教程
│   ├── USER_GUIDE.md         # 用户指南
│   └── ...
│
└── index.html                # 测试页面
```

---

## 🔧 核心 API 速查

### 聊天接口
```bash
# 发送消息
POST /subhuti/api/v1/chat

# 流式输出
POST /subhuti/api/v1/chat/stream
```

### 心灵宫殿
```bash
# 统计信息
GET /subhuti/api/v1/palace/stats

# 搜索记忆
POST /subhuti/api/v1/palace/search

# 遗忘清理
POST /subhuti/api/v1/palace/forget
```

### 专家插件
```bash
# 列出插件
GET /subhuti/api/v1/experts/list

# 激活专家
POST /subhuti/api/v1/experts/activate

# 停用专家
POST /subhuti/api/v1/experts/deactivate
```

### 系统监控
```bash
# 健康检查
GET /subhuti/api/v1/health

# 详细健康状态
GET /subhuti/api/v1/health/detailed

# Trace 追踪
GET /subhuti/api/v1/trace/{trace_id}
```

---

## 💡 从这里开始

### 新手 👉 [Quickstart](QUICKSTART.md)
5 分钟快速体验框架核心能力

### 开发者 👉 [API 教程](API_TUTORIAL.md)
手把手学会所有 API 的使用

### 架构师 👉 [架构详解](ARCHITECTURE.md)
深入理解框架设计理念和运行机制

### 插件开发者 👉 用户指南
学习如何开发专家插件和自定义 Skill

---

## 🆘 获取帮助

- 查看 [调试工具指南](DEBUG_TOOLS_GUIDE.md) - 开发调试必备
- 查看 [集成测试报告](INTEGRATION_TEST_REPORT.md) - 了解系统状态
- 访问测试页面 `http://localhost:8080/` - 交互式体验

---

## 📝 更新日志

### v1.0 (2026-06-28)
- ✅ 四层架构（心灵层 + 专家层 + Skill 层 + Flow 层）
- ✅ 心灵宫殿（记忆分区 + 重要性 + 遗忘 + 联想激活）
- ✅ 动态人格系统（大五人格 + 双轨演化）
- ✅ 专家插件系统（生命周期 + 钩子 + 知识库）
- ✅ 完整调试工具（诊断 + 性能 + 健康检查）
- ✅ 完整测试体系（49 单元测试 + 10 集成测试 + 9 调试工具测试 + 10 性能基准测试）
- ✅ HTTP API + 测试页面

---

**享受 Subhuti 的开发之旅吧！** 🎉
