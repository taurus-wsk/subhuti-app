# Subhuti

> 轻量级 Rust AI Agent 框架 | 心灵层动态角色养成 | 专家插件生态

[![Build](https://github.com/subhuti-ai/subhuti-app/actions/workflows/build.yml/badge.svg)](https://github.com/subhuti-ai/subhuti-app/actions/workflows/build.yml)

## 🚀 快速开始

```bash
# 编译并安装 CLI
cargo build --release --bin subhuti
make cli-install

# 环境诊断
subhuti doctor

# 启动 HTTP 服务
subhuti serve

# 测试 API
subhuti api chat --message "你好"
```

## 📋 功能特性

- **🏛️ 心灵宫殿** - 记忆与心灵的统一体，6 大分区、4 级重要性、遗忘机制、联想激活
- **🧠 动态人格** - 大五人格模型，双轨驱动演化，越用越懂你
- **🔌 专家插件** - 插件化领域能力注入，角色+技能+知识库一体化
- **🛠️ 调试友好** - 完整的诊断工具、实时日志、性能分析、数据库查询
- **⚡ 统一入口** - 单二进制 + CLI 子命令架构，核心逻辑完全复用

## 📦 安装

### 依赖要求

- Rust 1.74+
- PostgreSQL (可选，用于持久化存储)
- Ollama (可选，用于本地 LLM)

### 编译安装

```bash
# 进入项目目录
cd subhuti-app

# 编译 release 版本
make build

# 安装到系统路径
make cli-install
# 或手动复制
cp target/release/subhuti /usr/local/bin/subhuti
```

### Docker 部署

```bash
# 构建镜像
make docker-build

# 启动容器
make docker
```

## 🖥️ CLI 命令

### 命令总览

```
subhuti <COMMAND>

Commands:
  serve          启动 HTTP 服务
  doctor         环境诊断工具
  log-stream     实时日志流查看器
  db             数据库操作
  api            API 测试客户端
  flame          性能火焰图
```

---

### 1. 启动 HTTP 服务

```bash
# 基本用法
subhuti serve

# 使用 Makefile
make serve

# 开发模式（后台运行）
./scripts/build/dev.sh start
```

**选项说明**：
- 服务地址：`http://localhost:8080`
- 测试页面：`http://localhost:8080/subhuti/test/index.html`
- 健康检查：`http://localhost:8080/subhuti/api/v1/health`

---

### 2. 环境诊断

```bash
# 基本用法
subhuti doctor

# JSON 格式输出
subhuti doctor --json
```

**诊断内容**：
- 基础工具链（Rust、Cargo、Rustup）
- 数据库（Docker、PostgreSQL/pgvector）
- 向量模型（Ollama、bge-m3）
- LLM 服务（API Key 配置）
- 项目配置（Cargo.toml、源代码）

**输出示例**：
```
╔══════════════════════════════════════════════════════════════╗
║           Subhuti Doctor - 环境诊断工具
╚══════════════════════════════════════════════════════════════╝

━━━ 基础工具链 ━━━
  ✅  Rust 编译器             rustc 1.74.0 (79e9716c9 2023-11-13)
  ✅  Cargo 包管理            cargo 1.74.0 (ecb9851af 2023-10-18)
  ✅  Rustup 工具链管理        rustup 1.26.0 (5af9b9484 2023-04-05)

━━━ 数据库 ━━━
  ✅  Docker                  Docker version 24.0.6, build ed223bc
  ✅  PostgreSQL (pgvector)   Docker 容器运行中

━━━ 向量模型 ━━━
  ✅  Ollama                  ollama version 0.1.34
  ✅  bge-m3 向量模型          已安装 bge-m3:latest

━━━ LLM 服务 ━━━
  ✅  LLM 提供商              已配置豆包 (Doubao) API Key

━━━ 项目配置 ━━━
  ✅  项目配置                 Cargo.toml 存在
  ✅  源代码与文档             源代码和文档完整

══════════════════════════════════════════════════════════════

📊 诊断结果汇总
───────────────────────────────────────────────────────────────
  ✅ 通过:   10 项
  ❌ 失败:    0 项
  ⚠️  警告:    0 项
  🟡 可选:    0 项
───────────────────────────────────────────────────────────────
  🎯 环境就绪度: 100.0%

✅ 核心环境检查通过！

🚀 快速开始:
   subhuti serve              # 启动 HTTP 服务
   subhuti doctor             # 环境诊断
   subhuti api chat --message '你好' # 测试 API
   subhuti log-stream         # 实时日志监控

══════════════════════════════════════════════════════════════
```

---

### 3. 实时日志流

```bash
# 基本用法
subhuti log-stream

# 过滤 trace_id
subhuti log-stream --trace-id abc123

# 过滤级别
subhuti log-stream --level DEBUG

# 过滤用户
subhuti log-stream --user-id test_user

# 关键词搜索
subhuti log-stream --keyword "error"

# 指定日志目录
subhuti log-stream --log-dir ./logs

# 显示最近 N 行
subhuti log-stream --tail 100

# 组合过滤
subhuti log-stream --trace-id abc123 --level ERROR --keyword "orchestrate"
```

**快捷键**：按 `Ctrl+C` 退出

**输出示例**：
```
📡 实时日志流查看器 - 按 Ctrl+C 退出
───────────────────────────────────────────────────────────────
🔍 过滤 level: DEBUG

[2026-07-02T15:30:00.123456+08:00] [DEBUG] subhuti::orchestrator - dispatch: domain_tags=["psychology"]
[2026-07-02T15:30:00.124567+08:00] [INFO]  subhuti::orchestrator - expert_run: expert_id=psychology, step=1
[2026-07-02T15:30:00.567890+08:00] [INFO]  subhuti::server - Chat response: session=abc123, duration=443ms, tokens=128
```

---

### 4. 数据库操作

#### 4.1 列出所有表

```bash
subhuti db list-tables
```

#### 4.2 查看表结构

```bash
subhuti db schema --table traces
```

**输出示例**：
```
📋 表 traces 的结构
───────────────────────────────────────────────────────────────
| 字段名         | 类型         | 可空 |
|------------------------------------------------------------|
| id             | uuid         | ✗    |
| user_id        | varchar      | ✓    |
| session_id     | varchar      | ✓    |
| input          | text         | ✓    |
| output         | text         | ✓    |
| status         | varchar      | ✓    |
| duration_ms    | bigint       | ✓    |
| created_at     | timestamp    | ✓    |
```

#### 4.3 执行 SQL 查询

```bash
# 直接执行 SQL
subhuti db query --sql "SELECT * FROM traces LIMIT 5"

# 从文件读取 SQL
subhuti db query --file ./scripts/query.sql
```

#### 4.4 数据库统计

```bash
subhuti db stats
```

**输出示例**：
```
📊 数据库统计信息
───────────────────────────────────────────────────────────────
| 表名         | 行数   |
|────────────────────────|
| traces       | 1250   |
| sessions     | 48     |
| personas     | 1      |
```

---

### 5. API 测试客户端

#### 5.1 健康检查

```bash
subhuti api health
```

**输出示例**：
```
🌐 API 测试客户端 - http://localhost:8080/subhuti/api/v1
───────────────────────────────────────────────────────────────
Status: 200 OK
Response:
{
  "status": "ok",
  "timestamp": "2026-07-02 15:30:00"
}
```

#### 5.2 聊天测试

```bash
# 基本用法
subhuti api chat --message "你好，介绍一下你自己"

# 指定用户 ID
subhuti api chat --message "你好" --user-id test_user

# 指定会话 ID
subhuti api chat --message "继续" --session-id abc123

# 指定技能
subhuti api chat --message "计算 100 + 200" --skill calculator
```

**输出示例**：
```
🌐 API 测试客户端 - http://localhost:8080/subhuti/api/v1
───────────────────────────────────────────────────────────────
Status: 200 OK
Response:
{
  "response": "你好！我是 Subhuti，一个基于 Rust 构建的 AI Agent 框架...",
  "session_id": "abc123-def456",
  "trace_id": "trace-789-xyz",
  "skill_used": null,
  "duration_ms": 1245,
  "total_tokens": 156
}
```

#### 5.3 技能列表

```bash
subhuti api skills
```

#### 5.4 专家列表

```bash
subhuti api experts
```

#### 5.5 人格信息

```bash
subhuti api persona
```

#### 5.6 查询 Trace

```bash
subhuti api trace --id abc123
```

#### 5.7 会话列表

```bash
subhuti api sessions
```

#### 5.8 编排调用

```bash
# 基本用法
subhuti api orchestrate --message "分析这个心理问题"

# 指定用户
subhuti api orchestrate --message "分析" --user-id test_user

# 指定策略链
subhuti api orchestrate --message "分析" --chain SimpleDispatch
```

---

### 6. 性能火焰图

```bash
# 生成火焰图（默认 http 格式）
subhuti flame

# 生成后自动打开
subhuti flame --open

# 指定输出格式
subhuti flame --output-format html
```

**输出示例**：
```
🔥 性能火焰图生成器
───────────────────────────────────────────────────────────────
📝 输出文件: ./flamegraphs/flame_20260702_153000.html
🚀 服务已启动，开始收集性能数据... (按 Ctrl+C 停止)
^C✓ 数据收集完成
🌐 打开火焰图...
```

---

## 📂 项目结构

```
subhuti-app/
├── crates/
│   ├── subhuti/              # 核心框架
│   │   ├── src/
│   │   │   ├── soul/         # 心灵层 + 心灵宫殿
│   │   │   ├── skill/        # Skill 系统
│   │   │   ├── expert/       # 专家插件
│   │   │   ├── flow/         # Flow 流程层
│   │   │   ├── memory/       # 记忆系统
│   │   │   ├── orchestrator/ # 编排器
│   │   │   ├── observe/      # 观测器（Trace/Session）
│   │   │   └── runtime/      # 运行时系统
│   │   └── tests/            # 集成测试 + 性能测试
│   └── subhuti-expert-psychology/  # 心理学专家插件
│
├── src/bin/
│   ├── main.rs               # 统一入口（CLI + HTTP 服务）
│   ├── server.rs             # HTTP 服务器逻辑
│   ├── config.rs             # 配置模块
│   └── middleware.rs         # 中间件模块
│
├── config/
│   ├── Subhuti.toml          # 主配置文件
│   └── mock_responses.json   # Mock LLM 响应
│
├── docs/                     # 文档
├── scripts/                  # 脚本
├── static/                   # 静态资源（测试页面）
├── Makefile                  # 构建脚本
└── Cargo.toml                # 项目配置
```

## 🔧 配置说明

### 主配置文件

配置文件位于 `config/Subhuti.toml`：

```toml
[llm]
model = "doubao-pro-32k"
api_url = "https://api.doubao.com/v1/chat/completions"
provider = "doubao"
temperature = 0.7
max_tokens = 4096

[database]
host = "localhost"
port = 5432
database = "postgres"
username = "postgres"
password = "123456"
max_connections = 10

[http]
addr = "0.0.0.0:8080"

[test_mode]
enabled = true
mock_responses_path = "config/mock_responses.json"
mock_delay_ms = 3000
```

### 环境变量

| 变量 | 说明 |
|------|------|
| `DOUBAO_API_KEY` | 豆包 API Key |
| `OPENAI_API_KEY` | OpenAI API Key |
| `OLLAMA_BASE_URL` | Ollama 服务地址 |
| `RUST_LOG` | 日志级别（默认 `info`） |

### .env 文件

创建 `.env` 文件存放敏感配置：

```bash
DOUBAO_API_KEY=your_api_key_here
```

## 🧪 测试体系

```bash
# 运行所有测试
make test

# 单元测试
cargo test -p subhuti

# 集成测试
cargo test -p subhuti --test integration_test -- --nocapture

# 性能基准测试
cargo test -p subhuti --test performance_test -- --nocapture
```

## 🌐 API 端点

### 聊天接口

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/subhuti/api/v1/chat` | 发送消息 |
| POST | `/subhuti/api/v1/chat/stream` | 流式输出 |

### 技能接口

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/subhuti/api/v1/skills` | 技能列表 |
| POST | `/subhuti/api/v1/skills/:name` | 执行技能 |

### 专家接口

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/subhuti/api/v1/experts` | 专家列表 |
| POST | `/subhuti/api/v1/experts/:id/activate` | 激活专家 |
| POST | `/subhuti/api/v1/experts/deactivate` | 停用专家 |

### 记忆宫殿

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/subhuti/api/v1/palace/stats` | 统计信息 |
| POST | `/subhuti/api/v1/palace/search` | 搜索记忆 |
| POST | `/subhuti/api/v1/palace/forget` | 遗忘清理 |

### 编排接口

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/subhuti/api/v1/orchestrate` | 编排调用 |
| POST | `/subhuti/api/v1/orchestrate/analyze` | 任务分析 |

### 系统监控

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/subhuti/api/v1/health` | 健康检查 |
| GET | `/subhuti/api/v1/health/detailed` | 详细健康状态 |
| GET | `/subhuti/api/v1/traces` | Trace 列表 |
| GET | `/subhuti/api/v1/traces/:id` | Trace 详情 |

## 📝 日志查询

```bash
# 按 trace_id 查询
make log-trace ID=abc123

# 按 user_id 查询
make log-user ID=test_user

# 查看最近 N 条日志
make log-recent N=50

# 查看错误日志
make log-errors
```

## 🛠️ 开发工作流

```bash
# 代码格式化
make fmt

# Clippy 检查
make clippy

# 一键检查（fmt + clippy + test）
make check

# 编译 release
make build

# 启动开发服务器
make serve

# 查看服务状态
make serve-status

# 查看服务日志
make serve-logs

# 停止服务
make serve-stop
```

## 📚 文档

| 文档 | 说明 |
|------|------|
| [docs/QUICKSTART.md](docs/QUICKSTART.md) | 快速上手 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 架构详解 |
| [docs/API_TUTORIAL.md](docs/API_TUTORIAL.md) | API 使用教程 |
| [docs/DEBUG_TOOLS_GUIDE.md](docs/DEBUG_TOOLS_GUIDE.md) | 调试工具指南 |
| [docs/USER_GUIDE.md](docs/USER_GUIDE.md) | 用户指南 |

## 🆘 故障排除

### subhuti 命令未找到

```bash
# 安装 CLI
make cli-install
# 或
cp target/release/subhuti /usr/local/bin/subhuti
```

### 服务启动失败

```bash
# 检查端口占用
lsof -ti:8080

# 查看日志
make serve-logs
```

### 数据库连接失败

```bash
# 确认 PostgreSQL 容器运行
docker ps | grep pgvector

# 重新启动数据库
make docker
```

### LLM 调用失败

```bash
# 检查 API Key 配置
subhuti doctor

# 查看环境变量
echo $DOUBAO_API_KEY
```

## 📜 许可证

MIT License

## 🤝 贡献

欢迎提交 Issue 和 Pull Request！

---

**享受 Subhuti 的开发之旅吧！** 🎉