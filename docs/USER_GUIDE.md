# Subhuti AI Agent 框架使用指南

> 轻量级 Rust AI Agent 框架 | 心灵层动态角色养成 | 专家插件生态

## 目录

1. [框架概述](#框架概述)
2. [核心架构](#核心架构)
3. [心灵层系统](#心灵层系统)
4. [专家插件系统](#专家插件系统)
5. [Skill 系统](#skill-系统)
6. [Flow 流程层](#flow-流程层)
7. [记忆系统](#记忆系统)
8. [可观测性](#可观测性)
9. [API 使用指南](#api-使用指南)
10. [开发专家插件](#开发专家插件)
11. [配置说明](#配置说明)

---

## 框架概述

### 设计理念

**薄封装 + 强扩展**：主框架保持精简，只保留核心能力（LLM、数据库、消息管理），领域能力通过插件扩展。

```
┌─────────────────────────────────────────────────────┐
│                    Subhuti 框架                      │
├─────────────────────────────────────────────────────┤
│  ┌─────────┐  ┌─────────┐  ┌─────────┐             │
│  │ 心灵层  │  │ 专家层  │  │ Skill层 │  ← 扩展层  │
│  └─────────┘  └─────────┘  └─────────┘             │
├─────────────────────────────────────────────────────┤
│  ┌─────────┐  ┌─────────┐  ┌─────────┐             │
│  │  Flow   │  │ Memory  │  │ Runtime │  ← 核心层  │
│  └─────────┘  └─────────┘  └─────────┘             │
└─────────────────────────────────────────────────────┘
```

### 技术栈

| 组件 | 技术 |
|------|------|
| 语言 | Rust (异步) |
| HTTP | Axum |
| 数据库 | PostgreSQL + pgvector |
| 向量模型 | bge-m3:latest |
| LLM | Doubao (豆包) / Ollama |
| 观测 | 自带 Trace 系统 |

---

## 核心架构

### 四层架构图

```
┌──────────────────────────────────────────────────────────────────┐
│                        用户请求                                   │
│                     POST /chat {message}                          │
└────────────────────────────┬───────────────────────────────────┘
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│                     HTTP 网关层 (Axum)                            │
│   - 请求解析                                                      │
│   - 路由分发                                                      │
│   - 响应封装                                                      │
└────────────────────────────┬───────────────────────────────────┘
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│                      Subhuti 主入口                               │
│   - 专家激活检测                                                   │
│   - Skill 匹配                                                     │
│   - Flow 执行                                                      │
└────────────────────────────┬───────────────────────────────────┘
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│  ┌────────────┐  ┌────────────┐  ┌────────────┐  ┌────────────┐  │
│  │ 心灵层    │  │ 专家插件   │  │ Skill 层   │  │ Flow 层    │  │
│  │ Soul      │  │ Expert     │  │ Skill      │  │ Flow       │  │
│  └────────────┘  └────────────┘  └────────────┘  └────────────┘  │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐  ┌────────────┐  │
│  │ 记忆系统   │  │ LLM 运行时  │  │ Trace 追踪 │  │ 数据库     │  │
│  │ Memory     │  │ Runtime    │  │ Observe    │  │ Database   │  │
│  └────────────┘  └────────────┘  └────────────┘  └────────────┘  │
└────────────────────────────┬───────────────────────────────────┘
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│                      LLM Provider                                 │
│   Doubao API  ←─────────────────────→  Ollama (本地)               │
└──────────────────────────────────────────────────────────────────┘
```

### 模块职责

| 模块 | 文件位置 | 职责 |
|------|----------|------|
| **Soul** | `soul/mod.rs` | 心灵层：性格养成、用户反馈、大五人格 |
| **Expert** | `expert/mod.rs` | 专家插件：领域专家管理、生命周期、规划能力 |
| **Skill** | `skill/mod.rs` | 技能系统：Skill 注册、匹配、执行 |
| **Flow** | `flow/*.rs` | 流程模板：Simple / ReAct / Plan-Act |
| **Memory** | `memory/mod.rs` | 记忆系统：短期/长期/知识库/向量检索 |
| **Runtime** | `runtime/` | 运行时：LLM 客户端、Tool 调用 |
| **Observe** | `observe/trace.rs` | 可观测性：Trace 追踪、Span 记录 |
| **Database** | `db/mod.rs` | 数据库：PostgreSQL + pgvector |

---

## 心灵层系统

### 设计理念

心灵层是 Subhuti 的**独特创新**，位于 Skill 层之上，通过与用户的持续交互，动态"养成"角色的性格。

```
用户对话 + 反馈 ──→ 心灵层分析 ──→ 性格演化 ──→ Persona 更新
                                    ↓
                            大五人格参数变化
                            语气风格调整
                            擅长领域权重
```

### 核心数据结构

```rust
// 性格五维模型
pub struct BigFive {
    pub openness: f32,          // 开放性 0-1
    pub conscientiousness: f32,  // 尽责性 0-1
    pub extraversion: f32,      // 外向性 0-1
    pub agreeableness: f32,     // 宜人性 0-1
    pub neuroticism: f32,        // 情绪稳定性 0-1
}

// 完整 Persona
pub struct PersonaProfile {
    pub name: String,
    pub big_five: BigFive,
    pub tone: String,           // "友好" / "严谨" / "幽默"
    pub emotional_tendency: f32, // 情感倾向 0-1
    pub skill_proficiency: HashMap<String, f32>,  // 技能熟练度
    pub expertise_areas: HashMap<String, f32>,     // 擅长领域
    pub traits: Vec<String>,   // 性格特征标签
}
```

### 演化机制

```
┌────────────────────────────────────────────────────────────┐
│                    心灵层演化算法                            │
├────────────────────────────────────────────────────────────┤
│                                                            │
│   触发条件：每 20 次互动触发一次演化                       │
│                                                            │
│   统计分析轨道（实时）：                                    │
│   ├─ 用户反馈（👍/👎）直接影响性格参数                      │
│   ├─ 技能使用频率 → 技能熟练度（S型曲线）                   │
│   └─ 互动延迟 → 响应风格调整                                │
│                                                            │
│   LLM 自反思轨道（定期）：                                  │
│   ├─ 定期让 LLM 分析近期对话                               │
│   ├─ 生成性格变化建议                                       │
│   └─ 结合统计分析得出演化方向                               │
│                                                            │
│   演化算法：                                                │
│   new_value = old_value + learning_rate * delta            │
│                                                            │
└────────────────────────────────────────────────────────────┘
```

### 数据库持久化

心灵层数据存储在 PostgreSQL：

| 表名 | 存储内容 |
|------|----------|
| `persona_profiles` | 用户的当前 Persona |
| `persona_history` | Persona 版本历史 |
| `user_feedbacks` | 用户反馈记录（点赞/踩/评论）|

---

## 专家插件系统

### 设计理念

专家插件是 Subhuti 的**扩展单位**，每个插件代表一个"领域专家"，包含：

```
┌─────────────────────────────────────────────────────────────┐
│                      ExpertPlugin                           │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │
│  │ ExpertInfo   │  │ ExpertPersona │  │ Vec<Skill>   │    │
│  │ 专家信息     │  │ 性格定义     │  │ 技能列表     │    │
│  └──────────────┘  └──────────────┘  └──────────────┘    │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │
│  │ Manifest     │  │ Permissions   │  │ Hooks        │    │
│  │ 插件清单     │  │ 权限声明     │  │ 生命周期钩子 │    │
│  └──────────────┘  └──────────────┘  └──────────────┘    │
│  ┌──────────────────────────────────────────────────┐    │
│  │ ExpertPlanning (可选)                              │    │
│  │ 自主规划能力：任务分析 → 计划制定 → 执行 → 反思    │    │
│  └──────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
```

### 生命周期

```
┌───────────┐    enable()     ┌───────────┐   activate()   ┌────────────┐
│ Installed │ ────────────→  │  Enabled  │ ────────────→ │ Activated  │
│  (已安装)  │                │  (已启用)  │               │  (已激活)   │
└───────────┘                └───────────┘                └────────────┘
     ↑                           │                             │
     │         disable()         │       deactivate()          │
     └───────────────────────────┘ ←─────────────────────────────┘
                                   │
                                   ▼
                             ┌───────────┐
                             │ Disabled  │
                             │  (已停用)  │
                             └───────────┘
```

### 权限系统

```rust
pub struct PluginPermissions {
    pub file_read: bool,           // 文件读取
    pub file_write: bool,          // 文件写入
    pub network: bool,             // 网络请求
    pub database: bool,            // 数据库访问
    pub code_execution: bool,      // 代码执行
    pub external_api: bool,        // 外部 API
    pub modify_soul: bool,         // 修改心灵层
    pub access_other_plugins: bool, // 访问其他插件
}
```

### 钩子系统

插件可以在核心流程的各个扩展点插入逻辑：

| 钩子点 | 时机 |
|--------|------|
| `BeforeRequest` | 请求到达时 |
| `BeforeSkillMatch` | Skill 匹配前 |
| `BeforeSkillExecute` | Skill 执行前 |
| `AfterSkillExecute` | Skill 执行后 |
| `BeforeLlmCall` | LLM 调用前 |
| `AfterLlmCall` | LLM 调用后 |
| `BeforeMemorySearch` | 记忆检索前 |
| `AfterMemorySearch` | 记忆检索后 |
| `BeforeResponse` | 响应前 |
| `AfterResponse` | 响应后 |
| `OnExpertSwitch` | 专家切换时 |

---

## Skill 系统

### Skill 定义

```rust
pub trait Skill: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn keywords(&self) -> Vec<String>;
    fn execute(&self, ctx: &SkillContext) -> Result<Value, String>;
    fn flow_template(&self) -> Option<FlowTemplate>;  // 推荐流程
}
```

### Skill 匹配算法

```
输入: "北京天气怎么样"
         ↓
    提取关键词: ["北京", "天气"]
         ↓
    遍历所有 Skill:
    ┌─────────────────────────────┐
    │ weather     keywords: 7    │ ←─ 匹配!
    │ calculator  keywords: 9    │
    │ default_chat keywords: 0  │
    └─────────────────────────────┘
         ↓
    匹配度 = 命中关键词数 / Skill关键词总数
         ↓
    选择匹配度最高的 Skill
```

### 内置 Skills

| Skill | 关键词 | 说明 |
|-------|--------|------|
| `weather` | 7 | 天气查询 |
| `calculator` | 9 | 数学计算 |
| `search_long_memory` | 0 | 长期记忆检索 |
| `default_chat` | 0 | 默认聊天（兜底）|

---

## Flow 流程层

### 流程模板类型

```
┌─────────────────────────────────────────────────────────────┐
│                      Flow 类型对比                           │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Simple (简单):                                             │
│  ┌─────────┐    ┌─────────┐    ┌─────────┐               │
│  │ Input   │ →  │   LLM   │ →  │ Output  │               │
│  └─────────┘    └─────────┘    └─────────┘               │
│                                                             │
│  ReAct (思考):                                              │
│  ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐│
│  │ Input   │ →  │  Think  │ →  │  Action │ →  │ Observe ││
│  └─────────┘    └─────────┘    └─────────┘    └────▲────┘│
│                                                       │     │
│                                             (循环直到完成)│
│                                                             │
│  Plan-Act (计划-执行):                                      │
│  ┌─────────┐    ┌─────────┐    ┌─────────┐               │
│  │ Input   │ →  │  Plan   │ →  │   Act   │ → ...         │
│  └─────────┘    └─────────┘    └─────────┘               │
│                          ↓                                 │
│                    多步骤计划                               │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Flow 选择策略

```
┌─────────────────────────────────────────────────────────────┐
│  Skill 推荐 Flow                                             │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  if skill.flow_template.is_some() {                         │
│      use skill.recommended_flow()                           │
│  } else {                                                    │
│      // 根据任务复杂度自动选择                                │
│      if is_simple_task(input) {                             │
│          use Simple                                         │
│      } else if requires_reasoning(input) {                   │
│          use ReAct                                          │
│      } else {                                               │
│          use PlanAct                                         │
│      }                                                      │
│  }                                                          │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## 记忆系统

### 三层记忆架构

```
┌─────────────────────────────────────────────────────────────┐
│                        记忆系统                              │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────────────────────────────────────────┐  │
│  │              Knowledge Memory (知识库)                 │  │
│  │  - 结构化知识条目                                     │  │
│  │  - 向量嵌入存储                                      │  │
│  │  - 持久化到 PostgreSQL                               │  │
│  └─────────────────────────────────────────────────────┘  │
│                           ↑                                  │
│  ┌─────────────────────────────────────────────────────┐  │
│  │            Long-term Memory (长期记忆)                │  │
│  │  - 会话历史                                          │  │
│  │  - 重要事件                                          │  │
│  │  - 向量检索                                          │  │
│  └─────────────────────────────────────────────────────┘  │
│                           ↑                                  │
│  ┌─────────────────────────────────────────────────────┐  │
│  │           Short-term Memory (短期记忆)                │  │
│  │  - 当前会话上下文                                    │  │
│  │  - 内存存储                                          │  │
│  └─────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 向量搜索流程

```
查询: "我之前聊过什么关于编程的内容"

         ↓
┌─────────────────────────────────────┐
│  1. 生成查询向量 (bge-m3)           │
│     text → [0.1, 0.23, ...] 1024维  │
└─────────────────┬───────────────────┘
                  ↓
┌─────────────────────────────────────┐
│  2. PostgreSQL pgvector 相似度搜索   │
│     ORDER BY embedding <=> query    │
│     LIMIT 5                         │
└─────────────────┬───────────────────┘
                  ↓
┌─────────────────────────────────────┐
│  3. 返回相似记忆                     │
│     - 内容                           │
│     - 相似度分数                     │
│     - 时间戳                         │
└─────────────────────────────────────┘
```

---

## 可观测性

### Trace 追踪架构

```
┌─────────────────────────────────────────────────────────────┐
│                    Trace 追踪系统                            │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Trace ID: abc123                                           │
│  ├─ Span: Request (root)                                     │
│  │   ├─ Span: SkillMatch                                    │
│  │   │   └─ skill: default_chat, confidence: 1.0            │
│  │   ├─ Span: MemorySearch                                  │
│  │   │   └─ results: 3, total_time: 50ms                   │
│  │   ├─ Span: LlmCall                                      │
│  │   │   └─ tokens: 51+138=189, model: doubao             │
│  │   └─ Span: Response                                    │
│  │       └─ response: "你好呀..."                           │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Span 类型

| Kind | 说明 |
|------|------|
| `Request` | 整个请求 |
| `SkillMatch` | Skill 匹配过程 |
| `MemorySearch` | 记忆检索 |
| `LlmCall` | LLM 调用 |
| `ToolCall` | Tool 调用 |
| `SkillExecute` | Skill 执行 |
| `Response` | 最终响应 |

---

## API 使用指南

### 基础调用

```bash
# 健康检查
curl http://localhost:8080/subhuti/api/v1/health

# 聊天（AI 自动匹配 Skill）
curl -X POST http://localhost:8080/subhuti/api/v1/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "你好", "user_id": "test"}'

# 指定 Skill
curl -X POST http://localhost:8080/subhuti/api/v1/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "123 + 456 = ?", "skill": "calculator"}'
```

### 专家管理

```bash
# 获取专家列表
curl http://localhost:8080/subhuti/api/v1/experts

# 激活专家
curl -X POST http://localhost:8080/subhuti/api/v1/experts/psychology/activate

# 停用专家
curl -X POST http://localhost:8080/subhuti/api/v1/experts/deactivate

# 自动匹配专家
curl -X POST http://localhost:8080/subhuti/api/v1/experts/match \
  -H "Content-Type: application/json" \
  -d '{"input": "我最近压力很大"}'
```

### 心灵层

```bash
# 获取当前 Persona
curl http://localhost:8080/subhuti/api/v1/persona

# 发送反馈
curl -X POST http://localhost:8080/subhuti/api/v1/persona/feedback \
  -H "Content-Type: application/json" \
  -d '{"feedback_type": "like", "skill_name": "default_chat"}'

# 触发演化
curl -X POST http://localhost:8080/subhuti/api/v1/persona/evolve
```

### Trace 追踪

```bash
# 获取最近 Trace
curl http://localhost:8080/subhuti/api/v1/traces?limit=10

# 获取 Trace 详情
curl http://localhost:8080/subhuti/api/v1/traces/{trace_id}
```

---

## 开发专家插件

### 1. 创建插件项目

```bash
cargo new --lib crates/subhuti-expert-myplugin
```

### 2. 添加依赖

```toml
# Cargo.toml
[dependencies]
subhuti = { path = "../subhuti" }
```

### 3. 实现 ExpertPlugin Trait

```rust
use subhuti::expert::{ExpertPlugin, ExpertInfo, ExpertPersona, KnowledgeEntry};
use subhuti::skill::Skill;

pub struct MyExpert;

impl MyExpert {
    pub fn new() -> Self {
        Self
    }
}

impl ExpertPlugin for MyExpert {
    fn info(&self) -> ExpertInfo {
        ExpertInfo {
            id: "my_expert".into(),
            name: "我的专家".into(),
            description: "这是一个示例专家".into(),
            version: "1.0.0".into(),
            author: "Your Name".into(),
            category: "general".into(),
            keywords: vec![
                "示例".into(),
                "测试".into(),
            ],
        }
    }

    fn persona(&self) -> ExpertPersona {
        ExpertPersona {
            name: "我的助手".into(),
            big_five: BigFive { /* ... */ },
            tone: "友好".into(),
            emotional_tendency: 0.7,
            expertise_areas: hashmap! {
                "示例" => 0.9,
            },
            traits: vec!["耐心".into(), "细心".into()],
        }
    }

    fn skills(&self) -> Vec<Box<dyn Skill>> {
        vec![Box::new(MySkill::new())]
    }

    fn knowledge(&self) -> Vec<KnowledgeEntry> {
        vec![
            KnowledgeEntry {
                content: "这是我的专业知识".into(),
                tags: vec!["专业".into()],
                source: "builtin".into(),
            }
        ]
    }
}
```

### 4. 注册到主框架

```rust
// 在创建 Subhuti 时注册
let subhuti = Subhuti::builder()
    .llm(config)
    .skill(skill)
    .build()?
    .register_expert(MyExpert::new());
```

---

## 配置说明

### 环境变量

```bash
# LLM 配置
OPENAI_API_KEY=your_key          # OpenAI API Key
DOUBOA_API_KEY=your_key           # 豆包 API Key
DOUBOA_API_BASE=https://...       # 豆包 API 地址

# Ollama (可选，本地模型)
OLLAMA_API_URL=http://localhost:11434

# Embedding
EMBEDDING_API_URL=http://localhost:11434/api/embeddings
EMBEDDING_MODEL=bge-m3:latest

# 数据库
DATABASE_URL=postgres://postgres:123456@localhost:5432/postgres
```

### 配置结构

```rust
pub struct LLMConfig {
    pub provider: LLMProvider,     // Doubao / Ollama / OpenAI
    pub api_key: String,
    pub api_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: usize,
}

pub struct SubhutiConfig {
    pub llm: LLMConfig,
    pub db: Option<DbConfig>,       // 数据库配置
    pub embedding: EmbeddingConfig, // 向量配置
}
```

---

## 项目结构

```
subhuti-app/
├── crates/
│   ├── subhuti/                    # 主框架
│   │   └── src/
│   │       ├── lib.rs             # 主入口
│   │       ├── soul/              # 心灵层
│   │       ├── expert/            # 专家插件
│   │       ├── skill/             # 技能系统
│   │       ├── flow/              # 流程模板
│   │       ├── memory/            # 记忆系统
│   │       ├── runtime/           # 运行时
│   │       │   └── llm/           # LLM 客户端 + 重试
│   │       ├── observe/           # 可观测性
│   │       └── db/                # 数据库
│   │
│   └── subhuti-expert-psychology/ # 示例：心理咨询专家
│       └── src/lib.rs
│
├── src/
│   └── bin/
│       └── http_server/           # HTTP 网关
│           └── main.rs
│
├── static/
│   └── index.html                 # 测试页面
│
└── logs/                          # 日志目录
```

---

## 下一步

### 框架 roadmap

- [x] 心灵层系统
- [x] 专家插件生态
- [x] Trace 追踪
- [x] LLM 重试机制
- [ ] 结构化输出 (Function Calling)
- [ ] Tool 生态扩充
- [ ] 记忆自动清理
- [ ] 相似问题缓存

### 专家插件 roadmap

- [ ] 文件读写专家
- [ ] 代码助手专家
- [ ] 生活助手专家
- [ ] 写作专家

---

## 附录：API 完整列表

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/subhuti/api/v1/health` | 健康检查 |
| GET | `/subhuti/api/v1/skills` | 技能列表 |
| POST | `/subhuti/api/v1/chat` | 聊天（AI 匹配）|
| POST | `/subhuti/api/v1/chat/stream` | 流式聊天 |
| POST | `/subhuti/api/v1/skills/{name}` | 执行指定技能 |
| POST | `/subhuti/api/v1/skills/{name}/stream` | 流式执行技能 |
| GET | `/subhuti/api/v1/traces` | Trace 列表 |
| GET | `/subhuti/api/v1/traces/{id}` | Trace 详情 |
| GET | `/subhuti/api/v1/experts` | 专家列表 |
| GET | `/subhuti/api/v1/experts/plugins` | 插件详情 |
| GET | `/subhuti/api/v1/experts/active` | 当前专家 |
| POST | `/subhuti/api/v1/experts/{id}/enable` | 启用插件 |
| POST | `/subhuti/api/v1/experts/{id}/disable` | 停用插件 |
| POST | `/subhuti/api/v1/experts/{id}/activate` | 激活专家 |
| POST | `/subhuti/api/v1/experts/deactivate` | 停用专家 |
| POST | `/subhuti/api/v1/experts/match` | 匹配专家 |
| GET | `/subhuti/api/v1/persona` | 人格快照 |
| POST | `/subhuti/api/v1/persona/feedback` | 发送反馈 |
| POST | `/subhuti/api/v1/persona/evolve` | 触发演化 |
| GET | `/subhuti/api/v1/logs` | 日志查询 |
| GET | `/` | 测试页面 |

---

*文档版本: v0.1.0 | 更新日期: 2026-06-28*
