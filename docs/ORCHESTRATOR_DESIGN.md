# Orchestrator 编排器设计文档

## 1. 概述

Orchestrator（编排器）是 Subhuti 多专家协作系统的核心调度层，负责：
- 专家注册与管理
- 任务理解与分析
- 调度策略决策
- 责任链串行执行
- 全局规则约束

### 设计理念

- **规则引擎驱动**：所有约束、策略、监控都通过规则引擎统一管理
- **全局约束分散**：防护规则不集中在一层，而是分散到各阶段作为全局约束
- **零 AI 调度**：任务理解和策略决策完全基于关键词匹配，不调用 LLM，减少 AI 消耗
- **责任链模式**：专家按优先级排序后串行执行，前一个输出作为后一个输入

---

## 2. 核心架构

### 2.1 整体架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                        HTTP API Layer                            │
│                   /subhuti/api/v1/orchestrate                    │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                       Subhuti (全局状态)                        │
│  ┌─────────────┐  ┌─────────────┐  ┌───────────────────────┐   │
│  │   Memory    │  │   Runtime   │  │    Orchestrator       │   │
│  │  (记忆层)   │  │  (运行时)   │  │  (多专家调度器)       │   │
│  └─────────────┘  └─────────────┘  └───────────────────────┘   │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Orchestrator 内部结构                         │
│                                                                 │
│  ┌──────────────────┐  ┌──────────────────────────┐            │
│  │  Agent Registry  │  │      Rule Engine         │            │
│  │  (专家注册中心)   │  │    (三层规则引擎)         │            │
│  │  HashMap<id,     │  │                          │            │
│  │   ExpertAgent>   │  │  1. TaskAnalysisRule     │            │
│  └──────────────────┘  │  2. DispatchRule         │            │
│                        │  3. ExecutionRule        │            │
│  ┌──────────────────┐  └──────────────────────────┘            │
│  │  Context Store   │                                          │
│  │  (上下文存储)    │  ┌──────────────────────────┐            │
│  │  HashMap<ctx_id, │  │    责任链执行器          │            │
│  │   ContextData>   │  │    (Vec<Step> 串行)      │            │
│  └──────────────────┘  └──────────────────────────┘            │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 核心数据结构

| 结构体 | 职责 | 关键字段 |
|--------|------|----------|
| `Orchestrator` | 顶层调度器 | agent_registry, rule_engine, context_store |
| `AgentMeta` | 专家元信息 | id, name, tags, priority |
| `TaskProfile` | 任务画像 | domain_tags, task_type, subject, predicate, object |
| `Step` | 链路步骤 | agent_id, input_ctx_id |
| `ContextData` | 上下文数据 | content, metadata, created_at |
| `RuleConfig` | 全局规则配置 | 见第 4 节 |

---

## 3. 调度流程

### 3.1 完整调度流程图

```
用户任务输入
     │
     ▼
┌─────────────────────────────────────────────────────┐
│  Layer 1: TaskAnalysisRule（任务理解层）             │
│                                                     │
│  1. 长度校验（max_task_length）← 全局约束           │
│  2. 黑名单校验（blacklist_keywords）← 全局约束      │
│  3. 领域标签提取（关键词 → domain_tags）             │
│  4. 任务类型识别（关键词 → task_type）               │
│  5. 主谓宾提取（subject / predicate / object）      │
│                                                     │
│  输出: TaskProfile（结构化任务画像）                 │
└───────────────────────┬─────────────────────────────┘
                        │ TaskProfile
                        ▼
┌─────────────────────────────────────────────────────┐
│  Layer 2: DispatchRule（调度策略层）                 │
│                                                     │
│  1. 专家匹配（关键词 tags 匹配）                     │
│  2. 策略决策（SimpleDispatch / Pipeline）           │
│     - 匹配专家数 >= pipeline_threshold → Pipeline    │
│     - 领域标签数 >= 2 → Pipeline                      │
│     - 否则 → SimpleDispatch                          │
│  3. 专家过滤与限制 ← 全局约束                        │
│     - denied_agents（黑名单过滤）                    │
│     - allowed_agents（白名单过滤）                   │
│     - max_expert_count（数量截断）                   │
│  4. 优先级排序（priority 高 → 低）                   │
│                                                     │
│  输出: Vec<Step>（线性执行链路）                     │
└───────────────────────┬─────────────────────────────┘
                        │ Vec<Step>
                        ▼
┌─────────────────────────────────────────────────────┐
│  Layer 3: ExecutionRule（执行监控层）                │
│                                                     │
│  循环执行每个 Step:                                  │
│    ┌─ 步骤数检查（max_steps）← 全局约束              │
│    ├─ 总超时检查（max_execution_time_ms）← 全局约束  │
│    ├─ 获取专家实例                                   │
│    ├─ 确定输入上下文（传递上一步输出 or 原始输入）    │
│    ├─ 调用专家 run() 方法                            │
│    ├─ 单步超时检查（per_step_timeout_ms）← 全局约束  │
│    └─ 失败处理（continue_on_failure）← 全局约束      │
│                                                     │
│  结果聚合（result_strategy）← 全局约束               │
│    - TakeLast: 取最后一步结果                        │
│    - TakeFirst: 取第一步结果                         │
│    - MergeAll: 合并所有步骤结果                      │
│                                                     │
│  输出: ExecutionResult（最终结果）                   │
└─────────────────────────────────────────────────────┘
```

### 3.2 调度时序

```
用户      Orchestrator    RuleEngine      ExpertA      ExpertB
 │           │                │             │            │
 │  请求     │                │             │            │
 │──────────▶│                │             │            │
 │           │  analyze_task  │             │            │
 │           │───────────────▶│             │            │
 │           │  TaskProfile   │             │            │
 │           │◀───────────────│             │            │
 │           │                │             │            │
 │           │ decide_strategy│             │            │
 │           │───────────────▶│             │            │
 │           │  Strategy +    │             │            │
 │           │  Vec<Step>     │             │            │
 │           │◀───────────────│             │            │
 │           │                │             │            │
 │           │    run()       │             │            │
 │           │─────────────────────────────▶│            │
 │           │    ctx_id      │             │            │
 │           │◀─────────────────────────────│            │
 │           │                │             │            │
 │           │    run()       │             │            │
 │           │──────────────────────────────────────────▶│
 │           │    ctx_id      │             │            │
 │           │◀──────────────────────────────────────────│
 │           │                │             │            │
 │           │ merge_results  │             │            │
 │           │───────────────▶│             │            │
 │           │  final_output  │             │            │
 │           │◀───────────────│             │            │
 │  响应     │                │             │            │
 │◀──────────│                │             │            │
```

---

## 4. 规则引擎与全局约束

### 4.1 三层规则架构

| 层级 | 规则类型 | 核心职责 | 内置全局约束 |
|------|----------|----------|--------------|
| Layer 1 | TaskAnalysisRule | 任务理解、结构化提取 | max_task_length、blacklist_keywords |
| Layer 2 | DispatchRule | 策略决策、专家过滤 | max_expert_count、allowed_agents、denied_agents、pipeline_threshold |
| Layer 3 | ExecutionRule | 执行监控、结果聚合 | max_steps、max_execution_time_ms、per_step_timeout_ms、continue_on_failure、pass_full_context、result_strategy |

### 4.2 RuleConfig 完整配置

```rust
pub struct RuleConfig {
    // ── Layer 1 约束 ──
    pub max_task_length: usize,          // 任务最大长度，默认 10000
    pub blacklist_keywords: Vec<String>, // 禁用关键词，默认 ["暴力", "攻击", "色情"]

    // ── Layer 2 约束 ──
    pub max_expert_count: usize,         // 最大专家数量，默认 10
    pub allowed_agents: Vec<AgentId>,    // 专家白名单（空 = 不限制）
    pub denied_agents: Vec<AgentId>,     // 专家黑名单
    pub pipeline_threshold: usize,       // 触发 Pipeline 的匹配阈值，默认 2

    // ── Layer 3 约束 ──
    pub max_steps: usize,                // 最大执行步骤数，默认 10
    pub max_execution_time_ms: u64,      // 总执行超时(ms)，默认 300000
    pub per_step_timeout_ms: u64,        // 单步执行超时(ms)，默认 60000
    pub pass_full_context: bool,         // 是否传递完整上下文，默认 false
    pub continue_on_failure: bool,       // 失败是否继续，默认 false
    pub result_strategy: ResultStrategy, // 结果聚合策略，默认 TakeLast
}
```

### 4.3 策略决策逻辑

```
决策条件：
  if 匹配专家数 >= pipeline_threshold  OR 领域标签数 >= 2:
      → DispatchStrategy::Pipeline（串行流水线）
  else:
      → DispatchStrategy::SimpleDispatch（单专家直连）
```

### 4.4 结果聚合策略

| 策略 | 行为 | 适用场景 |
|------|------|----------|
| `TakeLast` | 取最后一个专家的输出 | 递进式任务，后一步覆盖前一步 |
| `TakeFirst` | 取第一个专家的输出 | 咨询式任务，第一步即最终答案 |
| `MergeAll` | 合并所有专家输出，带步骤标记 | 需要汇总多方意见的场景 |

---

## 5. 专家系统

### 5.1 ExpertAgent Trait

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

### 5.2 专家注册流程

```rust
// 1. 创建专家实例
let expert = DefaultExpert::new(
    "coding".into(),
    "编程专家".into(),
    vec!["编程".into(), "代码".into(), "rust".into()],  // 匹配关键词
    10,  // 优先级
    "Rust 编程专家".into(),
    "擅长 Rust 系统编程...".into(),
    "帮助用户解决编程问题".into(),
    runtime.clone(),
);

// 2. 注册到编排器
orchestrator.register_agent(Arc::new(expert));
```

### 5.3 内置专家列表

| 专家 ID | 名称 | 关键词 | 优先级 | 说明 |
|---------|------|--------|--------|------|
| `coding` | 编程专家 | 编程、代码、rust、开发、bug | 10 | 编程技术咨询 |
| `psychology` | 心理咨询师 | 心情、心理、情绪、咨询 | 8 | 心理健康支持 |
| `weather` | 天气专家 | 天气、温度、下雨、气象 | 5 | 天气查询 |

---

## 6. 上下文存储（享元模式）

### 6.1 设计思路

为了避免在专家间传递大量数据，采用**享元模式**：
- 所有上下文数据存储在 `ContextStore`（HashMap）中
- 专家间只传递 `ctx_id`（字符串），而非完整数据
- 每个专家通过 `ctx_id` 读写上下文

### 6.2 ContextData 结构

```rust
pub struct ContextData {
    pub content: String,           // 上下文内容
    pub metadata: HashMap<String, String>,  // 元数据
    pub created_at: u64,           // 创建时间戳
}
```

### 6.3 上下文传递策略

由 `pass_full_context` 配置控制：

- `false`（默认）：**链式传递**，每个专家的输出作为下一个专家的输入
- `true`：**全量传递**，每个专家都接收原始任务输入

---

## 7. 日志与可观测性

### 7.1 日志分级

| 层级 | 日志级别 | 内容 |
|------|----------|------|
| Layer 1 | info | 分析开始、5 个步骤完成、最终画像 |
| Layer 2 | info | 策略决策、专家过滤过程与结果 |
| Layer 3 | debug | 每次超时检查、步骤数检查 |
| 失败场景 | error / warn | 校验失败、超时、专家被过滤 |

### 7.2 日志命名规范

统一格式：`【模块名·子模块】描述信息`

示例：
```
【任务理解·Layer 1】开始分析任务
【任务理解·3/5】领域标签提取完成
【调度策略·Layer 2】决策结果: Pipeline
【调度策略·专家过滤】专家 'coding' 通过过滤
【执行监控·总超时检查】已执行 1500ms / 限制 300000ms
【执行监控·结果聚合】TakeLast策略: 取最后一个结果
```

### 7.3 快速排查指南

| 问题现象 | 搜索关键词 | 排查方向 |
|----------|-----------|----------|
| 请求被拒绝 | `长度校验失败` / `黑名单校验失败` | 检查输入长度或内容 |
| 没有匹配专家 | `无匹配专家` | 检查专家 tags 和输入关键词 |
| 专家未执行 | `被拒绝` / `denied_agents` / `allowed_agents` | 检查白/黑名单配置 |
| 执行超时 | `总执行超时` / `单步超时` | 检查超时配置或专家性能 |
| 结果不对 | `结果聚合` / `TakeLast` / `MergeAll` | 检查聚合策略 |

---

## 8. 扩展点

### 8.1 自定义任务理解规则

实现 `TaskAnalysisRule` trait：

```rust
pub trait TaskAnalysisRule: Send + Sync + Debug {
    fn analyze(&self, input: &str, config: &RuleConfig) -> Result<TaskProfile>;
}

// 注册
rule_engine.set_analysis_rule(Box::new(MyAnalysisRule));
```

### 8.2 自定义调度策略

实现 `DispatchRule` trait：

```rust
pub trait DispatchRule: Send + Sync + Debug {
    fn decide_strategy(&self, profile: &TaskProfile, matched_count: usize, config: &RuleConfig) -> DispatchStrategy;
    fn filter_and_limit(&self, agent_ids: Vec<AgentId>, profile: &TaskProfile, config: &RuleConfig) -> Vec<AgentId>;
}

// 注册
rule_engine.set_dispatch_rule(Box::new(MyDispatchRule));
```

### 8.3 自定义执行监控

实现 `ExecutionRule` trait：

```rust
#[async_trait]
pub trait ExecutionRule: Send + Sync + Debug {
    async fn check_timeout(&self, elapsed: Duration, config: &RuleConfig) -> Result<()>;
    async fn check_step_timeout(&self, elapsed: Duration, config: &RuleConfig) -> Result<()>;
    fn should_continue(&self, step_result: Result<CtxId>, config: &RuleConfig) -> bool;
    fn merge_results(&self, results: Vec<&ContextData>, config: &RuleConfig) -> String;
    fn check_max_steps(&self, current_step: usize, config: &RuleConfig) -> Result<()>;
}

// 注册
rule_engine.set_execution_rule(Box::new(MyExecutionRule));
```

### 8.4 添加新专家

1. 实现 `ExpertAgent` trait
2. 调用 `orchestrator.register_agent(Arc::new(my_expert))`

---

## 9. 设计模式总结

| 模式 | 应用位置 | 作用 |
|------|----------|------|
| 策略模式 | RuleEngine 的三层规则 | 任务分析、调度、执行策略可动态替换 |
| 责任链模式 | Step 串行执行 | 专家按顺序执行，输出传递给下一步 |
| 享元模式 | ContextStore | 只传 ctx_id，减少数据拷贝 |
| 模板方法模式 | ExpertAgent trait | 统一专家执行接口，内部逻辑各异 |
| 注册中心模式 | agent_registry + meta_registry | 全局唯一的专家注册与查询 |

---

## 10. 相关文件

- 核心实现：[crates/subhuti/src/orchestrator/mod.rs](../crates/subhuti/src/orchestrator/mod.rs)
- 全局状态：[crates/subhuti/src/lib.rs](../crates/subhuti/src/lib.rs)
- API 入口：[src/bin/http_server/main.rs](../src/bin/http_server/main.rs)
- 测试页面：[static/index.html](../static/index.html)
