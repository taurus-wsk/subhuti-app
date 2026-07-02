# Subhuti 系统架构设计文档

## 1. 整体架构总览

### 1.1 分层架构图

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          HTTP API 入口层                                  │
│              /subhuti/api/v1/orchestrate, /chat, /skills                 │
└─────────────────────────────────────┬───────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                      Subhuti 全局状态层 (lib.rs)                         │
│                                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                 │
│  │   Memory     │  │   Runtime    │  │ Orchestrator │                 │
│  │  (记忆层)    │  │  (运行时)    │  │ (多专家调度)  │                 │
│  └──────────────┘  └──────────────┘  └──────────────┘                 │
│                                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                 │
│  │    Soul      │  │   Observe    │  │    Skill     │                 │
│  │  (心灵层)    │  │ (可观测性)   │  │  (技能层)    │                 │
│  └──────────────┘  └──────────────┘  └──────────────┘                 │
└─────────────────────────────────────┬───────────────────────────────────┘
                                      │
              ┌───────────────────────┼───────────────────────┐
              ▼                       ▼                       ▼
    ┌──────────────────┐    ┌──────────────────┐    ┌──────────────────┐
    │   Expert 插件层  │    │    Flow 流程层    │    │   Context 上下文 │
    │  (领域专家)      │    │  (智能闭环)      │    │  (状态管理)      │
    └──────────────────┘    └──────────────────┘    └──────────────────┘
```

### 1.2 模块职责总表

| 模块 | 目录 | 核心职责 | 关键 Trait/Struct |
|------|------|----------|-------------------|
| **记忆层** | `memory/` | 所有数据存储、检索、分层治理 | `Memory`, `ShortTermMemory`, `LongTermMemory`, `KnowledgeMemory` |
| **存储层** | `memory/storage.rs` | PostgreSQL + pgvector 数据库实现 | `Database`, `DbConfig` |
| **运行时** | `runtime/` | LLM 抽象、工具系统、约束护栏 | `Runtime`, `LLM`, `Tool`, `Constraints` |
| **技能层** | `skill/` | 类似 HTTP 路由的技能匹配与执行 | `Skill`, `SkillManager`, `SkillContext` |
| **流程层** | `flow/` | Agent 智能闭环，多种流程策略 | `Flow`, `SimpleFlow`, `ReactFlow`, `PlanActFlow` |
| **专家层** | `expert/` | 专家插件系统，自主规划能力 | `ExpertPlugin`, `ExpertPlanning`, `PluginManifest` |
| **编排层** | `orchestrator/` | 多专家调度、责任链、规则引擎 | `Orchestrator`, `RuleEngine`, `ExpertAgent` |
| **心灵层** | `soul/` | 人格系统、记忆宫殿、动态养成 | `SoulLayer`, `MemoryPalace`, `BigFive` |
| **可观测性** | `observe/` | Trace 追踪、Span 树、统计指标 | `TraceObserver`, `Trace`, `Span` |
| **上下文** | `context.rs` | 全局状态 + 请求级上下文分离 | `RunContext`, `TokenStats` |

---

## 2. 记忆层 (Memory Layer)

### 2.1 三层记忆架构

```
┌─────────────────────────────────────────────────────────┐
│                    Memory (统一入口)                       │
└─────────────────────────────┬───────────────────────────┘
                              │
          ┌───────────────────┼───────────────────┐
          ▼                   ▼                   ▼
┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐
│ ShortTermMemory  │ │ LongTermMemory   │ │ KnowledgeMemory  │
│  短期工作记忆     │ │  长期归档记忆     │ │  知识库语义记忆   │
│  (Session 内)    │ │  (历史沉淀)      │ │  (向量检索)      │
│  容量: 10 条     │ │  归档阈值: 20    │ │  维度: 384       │
└──────────────────┘ └──────────────────┘ └──────────────────┘
          │                   │                   │
          └───────────────────┼───────────────────┘
                              ▼
                    ┌──────────────────┐
                    │  storage::Database│
                    │  PostgreSQL +     │
                    │  pgvector         │
                    └──────────────────┘
```

### 2.2 记忆流转机制

1. **写入**：新对话 → 短期记忆（HashMap，内存）
2. **归档**：短期记忆超过 `archive_threshold`(20) → 异步写入数据库 → 长期记忆
3. **检索**：
   - 短期记忆：直接遍历，最新 N 条
   - 长期记忆：关键词 + 语义搜索（向量相似度）
   - 知识库：纯向量检索

### 2.3 存储层 (storage.rs)

数据库作为 memory 模块的**内部基础设施**，不独立成层：

| 表名 | 用途 |
|------|------|
| `memories` | 所有记忆（短期/长期/知识库分层存储） |
| `personas` | 用户人格画像数据 |
| `feedbacks` | 用户反馈记录（点赞/点踩） |
| `sessions` | 会话记录 |

---

## 3. 运行时层 (Runtime Layer)

### 3.1 运行时结构图

```
┌─────────────────────────────────────────────────────────┐
│                        Runtime                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │
│  │  LLM Client  │  │  Tool Registry│ │ Constraints  │   │
│  │  (模型客户端) │ │  (工具注册中心)│ │  (约束护栏)  │   │
│  └──────────────┘  └──────────────┘  └──────────────┘   │
│                                                         │
│  ┌──────────────┐                                       │
│  │   Session    │                                       │
│  │  (会话状态)  │                                       │
│  └──────────────┘                                       │
└─────────────────────────────────────────────────────────┘
```

### 3.2 LLM 抽象层

```rust
pub trait LLM: Send + Sync {
    async fn chat(&self, messages: Vec<Message>) -> Result<LLMResponse>;
    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolInfo>,
    ) -> Result<LLMResponse>;
}
```

**实现**：
- `MockLLM`：测试模式，3 秒延迟，返回 mock 响应
- `LLMClient`：真实客户端（支持 Doubao、Ollama 等）
- **重试机制**：指数退避，最多 3 次（1s → 2s → 4s）

### 3.3 工具系统

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value) -> ToolResult;
}
```

**内置工具**：天气查询、计算器、代码执行、文件操作、网页搜索、提醒等

### 3.4 约束护栏 (Constraints)

- `max_turns: 10`：最大工具调用轮次
- `max_context_tokens: 8192`：最大上下文长度
- `timeout_seconds: 60`：请求超时

---

## 4. 技能层 (Skill Layer)

### 4.1 Skill 定位

Skill 是**单领域的原子能力单元**，类似 HTTP 路由：
- 输入：用户消息
- 匹配：关键词 + 置信度
- 执行：调用 LLM + Tools 完成任务

### 4.2 Skill Trait

```rust
#[async_trait]
pub trait Skill: Send + Sync {
    fn name(&self) -> &str;
    fn matches(&self, input: &str) -> f32;  // 匹配度 0.0-1.0
    fn flow_template(&self) -> Option<FlowTemplate>;  // 可选预设流程
    async fn execute(&self, ctx: SkillContext) -> Result<String>;
}
```

### 4.3 内置 Skill 列表

| Skill | 匹配关键词 | 用途 |
|-------|-----------|------|
| `WeatherSkill` | 天气、温度、下雨 | 天气查询 |
| `CalculatorSkill` | 计算、等于、加减乘除 | 数学计算 |
| `CodeExecutionSkill` | 运行代码、执行 | 代码执行 |
| `FileOperationSkill` | 读文件、写文件 | 文件操作 |
| `WebSearchSkill` | 搜索、查一下 | 网页搜索 |
| `ReminderSkill` | 提醒、定时 | 提醒设置 |
| `SearchLongMemorySkill` | 回忆、之前说过 | 长期记忆检索 |
| `DefaultChatSkill` | （默认兜底） | 普通对话 |

### 4.4 SkillContext 执行上下文

```rust
pub struct SkillContext {
    pub input: String,
    pub session: Session,
    pub memory: Arc<Memory>,
    pub runtime: Arc<Runtime>,
    pub tokens: Arc<RwLock<TokenStats>>,
    // 工具：call_tool(), call_llm(), search_memory()
}
```

---

## 5. 流程层 (Flow Layer)

### 5.1 Flow 定位

Flow 是 **Agent 智能闭环的执行策略**，Skill 可以选择使用预设 Flow 模板，也可以完全自定义。

### 5.2 内置 Flow

| Flow | 适用场景 | 执行逻辑 |
|------|----------|----------|
| `SimpleFlow` | 简单对话，无工具调用 | 直接调用 LLM 返回 |
| `ReactFlow` | 需要工具调用的任务 | ReAct 循环：Thought → Action → Observation |
| `PlanActFlow` | 复杂多步骤任务 | 先规划 Plan → 再分步 Act |

### 5.3 Flow Trait

```rust
#[async_trait]
pub trait Flow: Send + Sync {
    async fn execute(&self, ctx: &mut FlowContext) -> Result<String>;
}
```

### 5.4 FlowStep 步骤类型

Flow 可以由声明式步骤组成：

| 步骤类型 | 说明 | 是否需要 AI |
|----------|------|-------------|
| `Tool` | 调用工具 | 否 |
| `Knowledge` | 查询知识库 | 否 |
| `LLM` | 调用 LLM 生成 | 是 |
| `Condition` | 条件判断 | 否（代码逻辑） |
| `Memory` | 记忆读写 | 否 |

---

## 6. 专家层 (Expert Layer)

### 6.1 Expert 定位

Expert（专家）是**完整的领域智能体**，拥有：
- 独立的角色设定（role、backstory、goal）
- 专属的技能集合（skills）
- 自主的规划能力（ExpertPlanning）
- 独立的记忆（memory）

> **Skill vs Expert 的区别**：
> - Skill 是**原子工具能力**，无角色、无记忆
> - Expert 是**完整领域专家**，有人格、有技能、有记忆、能规划

### 6.2 ExpertPlugin Trait（插件接口）

```rust
pub trait ExpertPlugin: Send + Sync {
    // ── 清单与元数据 ──
    fn manifest(&self) -> PluginManifest;  // ID、版本、权限、依赖

    // ── 角色设定 ──
    fn persona(&self) -> ExpertPersona;    // role、backstory、goal

    // ── 能力声明 ──
    fn skills(&self) -> Vec<Arc<dyn Skill>>;   // 技能列表
    fn knowledge(&self) -> Vec<KnowledgeEntry>; // 知识库条目

    // ── 生命周期 ──
    fn on_activate(&self, ctx: &ExpertContext) -> Result<()>;
    fn on_deactivate(&self, ctx: &ExpertContext) -> Result<()>;

    // ── 匹配 ──
    fn matches(&self, input: &str) -> f32;
}
```

### 6.3 PluginManifest（清单系统）

```rust
pub struct PluginManifest {
    pub id: String,              // 唯一 ID
    pub name: String,            // 显示名称
    pub version: String,         // 语义化版本
    pub keywords: Vec<String>,   // 匹配关键词
    pub dependencies: Vec<String>,  // 依赖的其他插件
    pub permissions: PluginPermissions,  // 权限声明
    pub hooks: Vec<HookPoint>,   // 钩子点
    pub category: PluginCategory, // 分类
}
```

### 6.4 生命周期状态机

```
installed → enabled → activated
    ↑          │         │
    └──────────┴─────────┘
       (disabled)
```

| 状态 | 触发时机 | 钩子 |
|------|----------|------|
| installed | 插件被发现 | — |
| enabled | 用户启用插件 | — |
| activated | 任务匹配到该专家 | `on_activate()` |
| deactivated | 任务结束 | `on_deactivate()` |
| uninstalled | 用户卸载插件 | — |

### 6.5 ExpertPlanning（自主规划能力）

专家可以实现自主规划，成为"领域专家 + 规划师"：

```rust
pub trait ExpertPlanning {
    // 1. 任务分析
    fn analyze_task(&self, input: &str, ctx: &PlanningContext) -> TaskAnalysis;
    
    // 2. 制定计划
    fn create_plan(&self, analysis: &TaskAnalysis) -> ExecutionPlan;
    
    // 3. 执行步骤
    fn execute_step(
        &self,
        plan: &mut ExecutionPlan,
        step: &mut PlanStep,
    ) -> Result<Value, String>;
    
    // 4. 反思调整
    fn reflect_on(&self, plan: &ExecutionPlan, result: &Value) -> Reflection;
    
    // 5. 调整计划
    fn adjust_plan(&self, plan: &mut ExecutionPlan, reflection: &Reflection) -> bool;
}
```

**规划流程**：
```
analyze_task → create_plan → execute_step → reflect_on → adjust_plan → (循环)
```

### 6.6 专家内部执行流程

```
专家被调用
    │
    ▼
┌─────────────────────┐
│  on_activate()      │  激活钩子：加载技能、初始化记忆
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│  任务理解           │  关键词匹配 + 规则（不调用 LLM）
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│  有规划能力?        │
└─────┬─────────┬─────┘
      │ 是      │ 否
      ▼         ▼
┌──────────┐ ┌──────────────┐
│ 规划执行  │ │ 直接 Skill   │
│ Plan-Act │ │ 匹配执行     │
└─────┬────┘ └──────┬───────┘
      │              │
      └──────┬───────┘
             ▼
┌─────────────────────┐
│  on_deactivate()    │  停用钩子：清理资源
└─────────────────────┘
```

---

## 7. 编排层 (Orchestrator Layer)

### 7.1 Orchestrator 定位

Orchestrator 是**多专家协作的顶层调度器**，负责：
- 专家注册与管理（注册中心模式）
- 任务理解与分析（规则引擎 Layer 1）
- 调度策略决策（规则引擎 Layer 2）
- 责任链串行执行（规则引擎 Layer 3）

### 7.2 内部核心组件

```
┌─────────────────────────────────────────────────────────┐
│                       Orchestrator                        │
│                                                         │
│  ┌─────────────────────┐  ┌─────────────────────┐      │
│  │   Agent Registry    │  │     Rule Engine     │      │
│  │  HashMap<id, Arc<   │  │  ┌───────────────┐  │      │
│  │   ExpertAgent>>     │  │  │ TaskAnalysis  │  │      │
│  │  (专家注册中心)      │  │  │   Rule        │  │      │
│  └─────────────────────┘  │  ├───────────────┤  │      │
│                            │  │ Dispatch Rule │  │      │
│  ┌─────────────────────┐  │  ├───────────────┤  │      │
│  │   Context Store     │  │  │ ExecutionRule │  │      │
│  │  HashMap<ctx_id,    │  │  └───────────────┘  │      │
│  │   ContextData>      │  └─────────────────────┘      │
│  │  (享元模式)         │                                │
│  └─────────────────────┘                                │
└─────────────────────────────────────────────────────────┘
```

### 7.3 ExpertAgent Trait（专家统一接口）

```rust
#[async_trait]
pub trait ExpertAgent: Send + Sync + Debug {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn tags(&self) -> Vec<String>;     // 关键词标签，用于匹配
    fn priority(&self) -> u32 { 0 }   // 优先级，高的先执行

    async fn run(&self, ctx_id: &str, store: &mut ContextStore) -> Result<CtxId>;
}
```

> **ExpertPlugin vs ExpertAgent 的关系**：
> - `ExpertPlugin`：插件级接口，面向**插件开发者**，有完整生命周期、权限、依赖
> - `ExpertAgent`：调度级接口，面向**编排器**，是专家的最小执行抽象
> - 专家插件被注册时，会被适配为 `ExpertAgent` 注册到编排器

### 7.4 三层规则引擎

| 层级 | 规则类型 | 核心职责 | 全局约束 |
|------|----------|----------|----------|
| Layer 1 | `TaskAnalysisRule` | 任务理解（关键词→结构化画像） | `max_task_length`, `blacklist_keywords` |
| Layer 2 | `DispatchRule` | 策略决策 + 专家过滤 | `max_expert_count`, `allowed_agents`, `denied_agents`, `pipeline_threshold` |
| Layer 3 | `ExecutionRule` | 执行监控 + 结果聚合 | `max_steps`, `max_execution_time_ms`, `per_step_timeout_ms`, `continue_on_failure`, `pass_full_context`, `result_strategy` |

### 7.5 完整调度流程

```
用户任务
    │
    ▼
┌─────────────────────────────────────────────┐
│  Layer 1: TaskAnalysisRule                   │
│  1. 长度校验                                 │
│  2. 黑名单校验                               │
│  3. 领域标签提取 (DOMAIN_KEYWORDS)           │
│  4. 任务类型识别 (TASK_TYPE_KEYWORDS)        │
│  5. 主谓宾提取 (SPO)                         │
│  输出: TaskProfile                           │
└─────────────────────┬───────────────────────┘
                      │ TaskProfile
                      ▼
┌─────────────────────────────────────────────┐
│  专家匹配 (关键词 tags)                      │
│  优先级排序 (priority 高→低)                 │
└─────────────────────┬───────────────────────┘
                      │ matched_agents
                      ▼
┌─────────────────────────────────────────────┐
│  Layer 2: DispatchRule                       │
│  1. 策略决策 (SimpleDispatch / Pipeline)     │
│  2. 专家过滤 (白/黑名单)                     │
│  3. 数量截断 (max_expert_count)              │
│  输出: Vec<Step> 线性链路                    │
└─────────────────────┬───────────────────────┘
                      │ Vec<Step>
                      ▼
┌─────────────────────────────────────────────┐
│  Layer 3: ExecutionRule                      │
│  循环每个 Step:                               │
│    ├─ 步骤数检查 (max_steps)                 │
│    ├─ 总超时检查 (max_execution_time_ms)     │
│    ├─ 获取专家实例                            │
│    ├─ 确定输入上下文                          │
│    ├─ 调用 expert.run()                      │
│    └─ 失败处理 (continue_on_failure)         │
│                                              │
│  结果聚合 (result_strategy):                  │
│    TakeLast / TakeFirst / MergeAll           │
│                                              │
│  输出: ExecutionResult                        │
└─────────────────────────────────────────────┘
```

---

## 8. 心灵层 (Soul Layer)

### 8.1 Soul 定位

Soul 是 **AI 的"人格系统"**，让 AI 不再是冰冷的问答机器，而是有性格、有记忆、会成长的"数字生命"。

### 8.2 心灵宫殿结构

```
┌─────────────────────────────────────────────────────────┐
│                      Soul Layer                           │
│                                                         │
│  ┌─────────────────────┐                                │
│  │   MemoryPalace      │  ← 记忆宫殿（所有记忆的容器）  │
│  │  · 短期记忆厅       │                                │
│  │  · 长期记忆长廊     │                                │
│  │  · 知识图书馆       │                                │
│  │  · 6个主题房间      │                                │
│  └─────────────────────┘                                │
│                                                         │
│  ┌─────────────────────┐                                │
│  │   PersonaProfile    │  ← 人格画像（动态养成）        │
│  │  · BigFive 大五人格  │                                │
│  │  · 语气风格          │                                │
│  │  · 情感倾向          │                                │
│  │  · 技能熟练度        │                                │
│  └─────────────────────┘                                │
│                                                         │
│  ┌─────────────────────┐                                │
│  │  EvolutionEngine    │  ← 演化引擎（记忆→人格）       │
│  │  · 统计轨道(实时)   │                                │
│  │  · LLM反思(周期)    │                                │
│  └─────────────────────┘                                │
└─────────────────────────────────────────────────────────┘
```

### 8.3 大五人格模型 (BigFive)

| 维度 | 说明 | 默认值 |
|------|------|--------|
| Openness 开放性 | 愿意尝试新事物、创造力 | 0.6 |
| Conscientiousness 尽责性 | 严谨、精确、结构化 | 0.5 |
| Extraversion 外向性 | 活泼、话多、热情 | 0.5 |
| Agreeableness 宜人性 | 友善、共情、乐于助人 | 0.7 |
| Neuroticism 情绪稳定性 | 谨慎、保守、防御性 | 0.4 |

### 8.4 人格演化机制

**双轨驱动**：
1. **统计分析轨道**（实时、轻量）：每次互动更新词频、情感倾向
2. **LLM 自反思轨道**（周期性、深度）：每 N 次互动触发一次深度反思

**用户反馈直接影响**：点赞 → 对应特质 +0.01，点踩 → -0.01

---

## 9. 可观测性层 (Observe Layer)

### 9.1 Trace 追踪系统

记录完整的 Agent 思考过程：

```
Trace (一次请求)
  └─ Span (步骤)
      ├─ SkillMatch
      ├─ MemorySearch
      ├─ ToolCall
      ├─ LlmCall
      └─ ...
```

### 9.2 核心概念

| 概念 | 说明 |
|------|------|
| `TraceId` | 追踪 ID，贯穿整个请求生命周期 |
| `Span` | 单个操作的时间跨度，有父子关系 |
| `SpanKind` | 操作类型：SkillMatch / MemorySearch / ToolCall / LlmCall / ExpertRun |
| `TraceStore` | Trace 存储（内存 HashMap） |
| `TraceObserver` | 观察者，创建和管理 Trace |

### 9.3 Session 观测

- 会话记录持久化
- 用户反馈记录
- 统计指标（平均响应时间、Token 消耗等）

---

## 10. 上下文管理 (Context)

### 10.1 双层上下文设计

参考 Axum 的 State + Extensions 模式：

```
┌─────────────────────────────────────────────────────────┐
│  Subhuti (全局状态)  ←  AppState，Arc 共享，只读        │
│  · runtime: Arc<Runtime>                                │
│  · memory: Arc<Memory>                                  │
│  · orchestrator: Arc<RwLock<Orchestrator>>              │
│  · soul: Arc<SoulLayer>                                 │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│  RunContext (请求级)  ←  Extensions，每次请求新建       │
│  · session: Session                                     │
│  · tokens: Arc<RwLock<TokenStats>>                      │
│  · chain: Vec<String>  (Skill 调用链)                   │
└─────────────────────────────────────────────────────────┘
```

### 10.2 设计原则

- **全局资源只读共享**：用 Arc，避免竞争
- **请求级资源可变**：生命周期与请求绑定
- **避免"上帝对象"**：职责清晰，各管各的

---

## 11. 从用户请求到响应的完整调用链

### 11.1 单 Skill 对话流程

```
HTTP /chat
    │
    ▼
Subhuti.chat(session, input)
    │
    ├─ SkillManager.match_skill(input)  → 找到最佳匹配 Skill
    │
    ├─ 写入短期记忆
    │
    ├─ Skill.execute(SkillContext)
    │   │
    │   ├─ (可选) 使用 Flow 模板
    │   ├─ 调用 LLM / Tools
    │   └─ 返回结果
    │
    ├─ 写入长期记忆（异步归档）
    │
    └─ 返回响应 + trace_id + tokens
```

### 11.2 多专家编排流程

```
HTTP /orchestrate
    │
    ▼
Subhuti.orchestrate(input, user_id)
    │
    └─ Orchestrator.execute(input)
        │
        ├─ 【Layer 1】TaskAnalysisRule → TaskProfile
        │
        ├─ 关键词匹配专家 + 优先级排序
        │
        ├─ 【Layer 2】DispatchRule → Vec<Step>
        │
        └─ 【Layer 3】ExecutionRule
            │
            └─ 循环 Step（责任链）:
                │
                ├─ 检查约束（步骤数、超时）
                ├─ expert.run(ctx_id, context_store)
                └─ 传递上下文给下一步
                    │
                    ▼
            结果聚合 (TakeLast/MergeAll)
                │
                ▼
            返回 OrchestrationResult
```

---

## 12. 开发规范

### 12.1 日志先行原则

**所有新代码必须同步编写 `tracing` 日志**，将日志视为函数签名的必要组成部分：

| 代码元素 | 日志要求 | 示例 |
|---------|---------|------|
| 公开方法入口 | `info!` 记录参数、状态 | `info!("开始执行，参数: {}", arg)` |
| 关键决策点 | `debug!` 记录决策过程 | `debug!("匹配得分: {}, 阈值: {}", score, threshold)` |
| 边界/异常情况 | `warn!` 记录警告 | `warn!("向量维度不匹配: expected {}, got {}", expected, got)` |
| 方法出口 | `info!` 记录结果、统计 | `info!("执行完成，耗时: {}ms", duration)` |

**日志级别约定**：
- `info!`: 用户可感知的重要事件（方法出入、状态变化、统计结果）
- `debug!`: 开发调试所需的详细信息（参数、中间计算、返回值）
- `warn!`: 异常但可恢复的边界情况（非致命错误）

### 12.2 调试工具使用

开发过程中应充分利用项目的调试工具：

```bash
# 运行时查看详细日志
RUST_LOG=subhuti=debug cargo run

# 按 trace_id 查询链路
make log-trace ID=<your_trace_id>

# 查看特定模块日志
RUST_LOG=subhuti::memory=trace cargo test -- --nocapture
```

---

## 14. 设计模式总结

| 模式 | 应用位置 | 作用 |
|------|----------|------|
| 策略模式 | RuleEngine 三层规则 | 任务分析、调度、执行策略可动态替换 |
| 责任链模式 | Orchestrator Step 串行 | 专家按顺序执行，输出传下一步 |
| 享元模式 | ContextStore / Memory | 只传 ID，减少数据拷贝 |
| 模板方法模式 | ExpertAgent / Skill / Flow | 统一接口，内部实现各异 |
| 注册中心模式 | Agent Registry / Tool Registry / Skill Manager | 全局唯一注册与查询 |
| 状态机模式 | ExpertPlugin 生命周期 | installed → enabled → activated |
| 观察者模式 | TraceObserver / SessionObserver | 事件订阅与通知 |
| 适配器模式 | ExpertPlugin → ExpertAgent | 插件接口适配为调度接口 |

---

## 15. 核心文件索引

| 文件 | 说明 |
|------|------|
| [crates/subhuti/src/lib.rs](../crates/subhuti/src/lib.rs) | Subhuti 全局状态，模块导出 |
| [crates/subhuti/src/orchestrator/mod.rs](../crates/subhuti/src/orchestrator/mod.rs) | 编排器 + 规则引擎 + 责任链 |
| [crates/subhuti/src/expert/mod.rs](../crates/subhuti/src/expert/mod.rs) | 专家插件系统 (ExpertPlugin) |
| [crates/subhuti/src/expert/planning.rs](../crates/subhuti/src/expert/planning.rs) | 专家自主规划能力 (ExpertPlanning) |
| [crates/subhuti/src/skill/mod.rs](../crates/subhuti/src/skill/mod.rs) | Skill 技能层 |
| [crates/subhuti/src/flow/mod.rs](../crates/subhuti/src/flow/mod.rs) | Flow 流程层 (Simple/ReAct/PlanAct) |
| [crates/subhuti/src/memory/mod.rs](../crates/subhuti/src/memory/mod.rs) | 记忆层（三层记忆） |
| [crates/subhuti/src/memory/storage.rs](../crates/subhuti/src/memory/storage.rs) | 数据库存储实现（PostgreSQL + pgvector） |
| [crates/subhuti/src/runtime/mod.rs](../crates/subhuti/src/runtime/mod.rs) | 运行时（LLM + Tools + Constraints） |
| [crates/subhuti/src/soul/mod.rs](../crates/subhuti/src/soul/mod.rs) | 心灵层（人格 + 记忆宫殿） |
| [crates/subhuti/src/observe/mod.rs](../crates/subhuti/src/observe/mod.rs) | 可观测性（Trace + Span） |
| [crates/subhuti/src/context.rs](../crates/subhuti/src/context.rs) | 上下文管理（RunContext + TokenStats） |
