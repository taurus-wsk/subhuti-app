# Subhuti

> AI Agent 框架 · 六边形架构 · LLM 引擎编排 + 专家技能 · 藏经阁知识召回

基于 Rust 的轻量级 AI Agent 单二进制应用。核心思路：**框架引擎只管"规划与驱动"，专家只写技能，LLM 自动编排技能完成目标**。

---

## 🚀 快速开始

### 1. 准备环境

| 依赖 | 说明 |
|------|------|
| Rust 1.74+ | 必需 |
| PostgreSQL | 用于持久化（会话、专家、知识库等） |
| 智谱 API Key | LLM 提供商（默认），其他见[配置](#配置) |

### 2. 配置 LLM

```bash
# 智谱（默认提供商）
export ZHIPU_API_KEY="你的智谱 API Key"
```

也可在 `config/Subhuti.toml` 切换 `doubao` / `openai` / `ollama`，详见[配置](#配置)。

### 3. 初始化数据库

```bash
# 导入表结构（schema 见 sql/schema.sql）
psql postgres://postgres:123456@localhost:5432/postgres -f sql/schema.sql
```

> 表结构变更统一收敛在 `sql/schema.sql`，可直接用 `make db-tables` / `make db-schema` 查看当前库。

### 4. 编译

```bash
cargo build --release
```

### 5. 启动 HTTP 服务

```bash
# 直接运行二进制（默认端口 8615）
./target/release/subhuti serve

# 或使用 Makefile（前台）
make serve
```

健康检查：`http://localhost:8615/subhuti/api/v1/health`

### 6. 试跑一次编排

```bash
# 通过 CLI 调用编排（触发 LLM 规划 → 专家执行）
subhuti api orchestrate --message "你好，介绍一下你自己"
```

此时整条链已跑通：引擎用 LLM 生成技能计划 → 按序调用专家技能 → 返回结果。

---

## 📋 功能特性

- **🧠 引擎规划执行** - 引擎 `planner` 用 LLM 生成技能计划，再按序驱动执行（生成→校验→重试等闭环）
- **🔌 专家技能体系** - 专家只实现 `execute_skill` 分发 + 各 `skill_*` 业务，技能列表暴露给 LLM 自动编排
- **🏛️ 藏经阁知识召回** - 五阶段召回流水线，专家按需检索知识库（支持 `rust-knowledge-query` 技能）
- **🧬 六边形架构** - `adapter(入/出) / application / domain` 分层解耦，领域层不依赖框架
- **🌐 多 LLM 支持** - 智谱 / 豆包 / OpenAI / Ollama 可切换，本地可挂 Mock（`--mock`）
- **🔭 可观测调试** - trace 追踪、SSE 实时进度、`log-stream` 日志流、性能火焰图
- **🧩 Plugin SDK / WASM 插件** - 插件化领域能力注入（含 `subhuti-expert-psychology-wasm` 示例）

---

## 🖥️ CLI 命令

```
subhuti <COMMAND>

Commands:
  serve         启动 HTTP 服务（--mock 启用 Mock LLM，--debug 调试模式，--addr 指定端口）
  doctor        环境诊断
  api           调用 API（health/experts/trace/sessions/orchestrate）
  log-stream    实时日志流（--trace-id / --level / --keyword 过滤）
```

### 常用示例

```bash
# 启动 HTTP 服务
subhuti serve

# 用 Mock LLM 启动（不打网络，适合本地/测试）
subhuti serve --mock

# 环境诊断
subhuti doctor

# 健康检查
subhuti api health

# 专家列表
subhuti api experts

# 编排调用（健康检查 → LLM 规划执行）
subhuti api orchestrate --message "你好"

# 实时监控日志（按级别/关键字过滤）
subhuti log-stream --level DEBUG --keyword expert
```

---

## ⚙️ 配置

配置文件：`config/Subhuti.toml`（优先级：环境变量 > 此文件 > 代码默认值）

```toml
[llm]
provider = "zhipu"                 # zhipu | doubao | openai | ollama
model = "glm-4-flash"
api_url = "https://open.bigmodel.cn/api/paas/v4"
temperature = 0.7
max_tokens = 2048

[database]
host = "localhost"
port = 5432
database = "postgres"
username = "postgres"
password = "123456"

[http]
addr = "0.0.0.0:8615"

[logging]
level = "info"                     # trace | debug | info | warn | error
```

| 环境变量 | 说明 |
|------|------|
| `ZHIPU_API_KEY` | 智谱 API Key |
| `DOUBAO_API_KEY` | 豆包 API Key |
| `OPENAI_API_KEY` | OpenAI API Key |
| `DB_PASSWORD` | 数据库密码（推荐用环境变量，勿写进配置文件） |
| `RUST_LOG` | 日志级别 |

> **不使用向量模型**：知识召回走结构/关键词检索，无需 Ollama / bge-m3 等向量依赖。

---

## 🧱 如何新增一个专家技能

框架的最小闭环：**引擎 planner 编排 → `execute_skill` 分发 → `skill_*` 业务 → `exec_ctx` 工具**。

添加新技能只需 3 步：

1. 在专家 `skills()` 里注册 `DomainSkill { id, name, description, parameters }`（会暴露给 LLM 用于自动编排）
2. 在 `execute_skill` 的 `match` 里加一个分支，指向新函数
3. 实现 `skill_xxx(&self, exec_ctx, params)`，从 `exec_ctx` 取 `llm` / `file_system` / `command` / `sutra_library` 等工具完成业务

参考：Rust 专家 `src/domain/experts/rust_expert.rs`（已含 `rust-chat/coding/generate/review/fix/refactor/skill-list/knowledge-query` 8 个技能）。

---

## 📂 项目结构

```
subhuti-app/
├── crates/
│   ├── subhuti-core/            # 框架核心（引擎）：planner 规划执行、graph 编排、Actor 竞标、event 观测
│   ├── subhuti-infra/           # 基础设施：LLM 客户端、藏经阁(sutra_library)、记忆、工具
│   ├── subhuti-plugin-sdk/      # 插件开发 SDK
│   └── subhuti-expert-psychology-wasm/  # WASM 插件示例
│
├── src/
│   ├── adapter/
│   │   ├── inbound/            # 入口：CLI + HTTP(Axum) 路由
│   │   └── outbound/           # 出站：专家适配器、文件系统、命令、LLM 注入、rules
│   ├── application/            # 应用层：编排服务、组合根、trace 装饰
│   ├── domain/                 # 领域层：DomainExpert、专家实现（rust_expert 等）、ports
│   ├── infra/                  # 应用配置
│   └── bin/                    # 二进制入口：main.rs（subhuti）
│
├── sql/
│   └── schema.sql              # 数据库表结构（唯一权威来源）
├── config/
│   └── Subhuti.toml            # 主配置
├── docs/                       # 文档
└── scripts/                    # 构建/调试/发布脚本
```

---

## 🛠️ 开发工作流

```bash
make build         # 编译 release
make fmt           # 代码格式化
make clippy        # Clippy 检查
make check         # fmt + clippy + test
make test          # 运行测试
make serve         # 启动开发服务
make serve-debug   # 调试模式启动（更详细日志）
make serve-logs    # 查看服务日志
make serve-stop    # 停止服务
make docker        # Docker 构建并启动
make db-tables     # 查看数据库表
make db-schema     # 查看表结构
make db-query      # 执行 SQL
make log-stream    # 实时日志流
make flame         # 性能火焰图
make routes        # 列出已注册 HTTP 路由
```

---

## 🌐 API 端点

服务前缀：`/subhuti/api/v1`

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/health` | 健康检查 |
| POST | `/orchestrate` | 编排统一入口（`Accept: text/event-stream` 时返回 SSE 流式） |
| GET | `/experts` | 专家列表 |
| GET | `/traces` | Trace 列表 |
| GET | `/traces/:id` | Trace 详情 |
| GET | `/sessions` | 会话列表 |
| GET/POST | `/knowledge` | 知识库相关 |

> 技能列表与技能执行**不在 HTTP 面暴露**，改用 MCP 工具 `subhuti_skill_list` / `subhuti_skill_run`。

---

## 🔍 调试

可视化链路：`src/adapter/inbound/http/routes/traces.rs` 提供 Trace 可视化；配合 `log-stream`、`flame` 观察执行链路与性能。

常用调试命令见仓库根 `本人常用调试命令.md`。

---

## 📜 许可证

MIT License

---

**享受 Subhuti 的开发之旅吧！** 🎉