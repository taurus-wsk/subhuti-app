# 复用框架 EventBus + 适配器桥接：细粒度 Trace 落地计划

## Context（为什么做这个改动）

**痛点**：当前 trace 只记录应用层入口/出口（input/output/duration），领域引擎内部和出站适配器完全没有埋点：

* `trace_decorator.rs:97` token\_usage 硬编码 `{"total_tokens": 0}`

* `subhuti_orchestration_engine.rs:49` 引擎适配器只取最终 result，丢弃中间过程

* `observer_adapters.rs:89` `get_span_tree` 返回 `None`，无 span 树

**关键发现**：subhuti 框架内部**已有完整事件总线**（`EventBus` + 23 种 `AgentEventData` + `EventHandler` 订阅机制），且 Orchestrator 已在 5 个关键点 emit 事件（`UserMessage`/`ChainSelected`/`AgentStarted`/`AgentCompleted`/`AgentFailed`）。`Subhuti.event_bus()` 已暴露，`SubhutiFrameworkInitializer` 已调 `init_builtin_handlers`。

**真正问题**：项目出站适配器**没有订阅框架 EventBus**，导致框架内部产生的事件全部丢失。

**方案**：不新建事件总线，复用框架已有的。在出站适配器层加 EventBridge 桥接框架事件 → 项目 TraceObserver；改框架 `emit_event` 让事件带 trace\_id（方案 A）；trace\_id 穿透端口签名到 `ctx.metadata`。

**预期成果**：`/traces/:id/span_tree` 返回完整嵌套链路（根请求 → 策略 → 专家调用 → token 消耗），token 不再硬编码 0。

## 架构设计

```
TraceAppService (生成 trace_id, 设入 request.trace_id)
  → AppService (透传 trace_id)
    → OrchestrationEnginePort.orchestrate(message, user_id, chain, trace_id)  [端口加参数]
      → SubhutiOrchestrationEngine
        - ctx.set_metadata("trace_id", trace_id)
        - subhuti.dispatch_with_context(ctx)
          [框架 emit_event(ctx, data) 从 ctx.metadata 读 trace_id → emit_with_trace]  ← 方案A

框架 EventBus ──broadcast──→ TraceEventBridge (持久 EventHandler)
                              handle(event) 按 event.metadata.trace_id 归集
                                → AgentEventData 转换为 SpanData
                                → TraceObserverPort.record_span(trace_id, span)

SubhutiTraceObserverAdapter
  traces: Mutex<Vec<TraceHandle>>            (摘要，已有)
  spans:  Mutex<HashMap<trace_id, Vec<SpanData>>>  (新增，细粒度)
  get_span_tree(id) → 组装 span 树 JSON
```

**时序解耦**（关键设计）：`store_trace` 存摘要，bridge 持续异步写 `spans` HashMap，`get_span_tree` 查询时读 HashMap。请求结束 → handler 异步处理完事件 → 查询时 span 已完整。**不需要同步等待事件处理**。

## 改动清单（按层 + 执行顺序）

### 步骤 1：框架层 emit_event 改造（方案 A，orchestrator + graph 两条路径）

用户确认：orchestrator + graph 都改，领域层保持纯净，LLM 事件本次不做。

#### 1a. Orchestrator 路径（emit_event 从 ctx.metadata 读 trace_id）
**文件**：`crates/subhuti-core/src/orchestrator/mod.rs`

改 `emit_event` 签名，从 ctx.metadata 读 trace_id：
```rust
// 改前（L623）
async fn emit_event(&self, data: AgentEventData) {
    if let Some(ref bus) = self.event_bus { bus.emit(data).await; }
}
// 改后
async fn emit_event(&self, ctx: &AgentContext, data: AgentEventData) {
    if let Some(ref bus) = self.event_bus {
        let trace_id = ctx.metadata.get("trace_id").cloned();
        let session_id = ctx.metadata.get("session_id").cloned();
        match trace_id {
            Some(tid) => bus.emit_with_trace(data, tid, session_id).await,
            None => bus.emit(data).await,
        }
    }
}
```
**9 个调用点**加 ctx 参数（都在有 `ctx: &mut AgentContext` 的方法内）：L402, L467, L521, L540, L549, L561, L595, L669, L678。改法统一：`self.emit_event(data).await` → `self.emit_event(ctx, data).await`。

覆盖事件：`UserMessage` / `ChainSelected` / `AgentStarted` / `AgentCompleted` / `AgentFailed`。

#### 1b. Graph 路径（emit 从 GraphState 读 trace_id）
**文件**：`crates/subhuti-core/src/graph/engine.rs` + `crates/subhuti-core/src/graph/actor/node_actor.rs`

- `dispatch_via_graph`（orchestrator/mod.rs:600）构造 GraphState 时注入 trace_id：
  ```rust
  let mut graph_state = GraphState::new();
  graph_state.set("input", &*ctx.input);
  graph_state.set("trace_id", ctx.metadata.get("trace_id").cloned().unwrap_or_default());  // 新增
  ```
- `graph/engine.rs:803` `emit_event` 改签名加 `state: &GraphState`，从 `state.get("trace_id")` 读，用 `emit_with_trace`。调用点 L271,318,372,387,588,742 加 state 参数（都在持有 state 的 run 方法内）。
- `graph/actor/node_actor.rs:247` `emit` 同样改造，从 state 读 trace_id。调用点 L171,192 加 state 参数（在 `handle_execute(state, reply)` / `execute_with_retry(state)` 内）。

覆盖事件：`FlowStarted` / `FlowStepExecuted` / `FlowCompleted` / `GraphStarted` / `NodeCompleted` / `NodeFailed` / `GraphCompleted`。

> **边界**：LLM/工具层事件（`LLMCalling`/`LLMResponded`/`ToolCalling`）本次**不做**——框架内无 emit 调用点（只有类型定义），需改 LLM client，留作后续。领域专家内部步骤（blender 子流程）**不做**——领域层 `exec_ctx` 不含 trace_id，保持领域层纯净。

### 步骤 2：领域层 DTO + 端口签名

**文件**：`src/domain/dto.rs`、`src/domain/ports.rs`

* `OrchestrateRequest` 加字段：`pub trace_id: Option<String>`

* `OrchestrationEnginePort::orchestrate` 加参数：`fn orchestrate(&self, message: &str, user_id: &str, chain: &str, trace_id: &str) -> ...`

* `SkillExecutionPort::execute_skill` 加参数：`fn execute_skill(&self, skill_id: &str, args: &str, trace_id: &str) -> ...`

### 步骤 3：应用层 SpanData + TraceObserverPort 升级

**文件**：`src/application/observer.rs`

新增 SpanData DTO（trace 属应用运维概念，放 observer.rs 不放 domain）：

```rust
#[derive(Debug, Clone)]
pub struct SpanData {
    pub span_type: String,      // "agent_started" / "agent_completed" / ...
    pub name: String,           // 人类可读（专家名/技能名）
    pub input: Option<String>,
    pub output: Option<String>,
    pub duration_ms: Option<u64>,
    pub tokens: Option<u64>,    // LLM 事件用
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub success: Option<bool>,
}
```

`TraceObserverPort` 加方法：

```rust
fn record_span(&self, trace_id: &str, span: SpanData);
```

### 步骤 4：应用层 AppService 透传 + TraceAppService 注入 trace\_id

**文件**：`src/application/app_service.rs`、`src/application/trace_decorator.rs`

* `AppService.orchestrate`：从 `request.trace_id` 取，传给 `engine.orchestrate(message, user_id, chain, trace_id)`

* `AppService.execute_skill`：签名加 `trace_id: &str`，传给 `skill_executor.execute_skill(skill_id, args, trace_id)`

* `TraceAppService.orchestrate`：生成 trace\_id 后，`request.trace_id = Some(trace_id.clone())`，再调 inner

* `TraceAppService.execute_skill`：生成 trace\_id，调 `inner.execute_skill(skill_id, args, &trace_id)`

> `SkillPort`（入站）`execute_skill` 加 `trace_id: &str` 参数。HTTP handler 传空字符串，装饰器生成真实值传给 inner。

### 步骤 5：出站适配器设 ctx.metadata

**文件**：`src/adapter/outbound/subhuti_orchestration_engine.rs`、`src/adapter/outbound/subhuti_skill_executor.rs`

* `SubhutiOrchestrationEngine.orchestrate`：接收 trace\_id，`ctx.set_metadata("trace_id", trace_id)` + `ctx.set_metadata("session_id", session_id)`（session\_id 可选，从 user\_id 或生成）

* `SubhutiSkillExecutor.execute_skill`：同上，设 ctx.metadata

### 步骤 6：TraceEventBridge（核心桥接器）

**新建文件**：`src/adapter/outbound/event_bridge.rs`

实现 subhuti `EventHandler` trait，把框架 `AgentEventData` 转成项目 `SpanData`：

```rust
pub struct TraceEventBridge {
    observer: Arc<dyn TraceObserverPort>,
}

#[async_trait]
impl subhuti::event::EventHandler for TraceEventBridge {
    async fn handle(&self, event: &subhuti::event::Event) {
        if let Some(trace_id) = &event.metadata.trace_id {
            if let Some(span) = convert_event_to_span(&event.data) {
                self.observer.record_span(trace_id, span);
            }
        }
    }
    fn filter(&self) -> subhuti::event::EventFilter {
        EventFilter::Types(vec![
            "user_message", "chain_selected",
            "agent_started", "agent_completed", "agent_failed",
            "llm_responded",  // token 消耗
        ])
    }
    fn name(&self) -> &str { "app_trace_bridge" }
}
```

`convert_event_to_span`：match `AgentEventData` 变体 → 构造 `SpanData`（专家名、输出、duration、tokens）。

### 步骤 7：SubhutiTraceObserverAdapter span 存储

**文件**：`src/adapter/outbound/observer_adapters.rs`

* 加字段：`spans: Mutex<HashMap<String, Vec<SpanData>>>`

* 实现 `record_span`：按 trace\_id push 到 Vec

* 实现 `get_span_tree`：读 HashMap，组装嵌套 JSON（按时间排序，专家调用作为子 span）

### 步骤 8：组合根注册 bridge

**文件**：`src/application/composition_root.rs`、`src/adapter/outbound/subhuti_framework_initializer.rs`

调整组装顺序：

1. 创建 observer（`SubhutiTraceObserverAdapter`）提前到 build\_adapters 之前
2. `SubhutiFrameworkInitializer` 加方法 `pub async fn register_event_bridge(&self, observer: Arc<dyn TraceObserverPort>)`：构造 `TraceEventBridge`，`subhuti.event_bus().subscribe(Arc::new(bridge)).await`
3. CompositionRoot 在 `init()` 后、`build_adapters()` 前调用 `register_event_bridge`
4. observer 作为具体类型 `Arc<SubhutiTraceObserverAdapter>` 创建，clone 给 bridge（record\_span）和 Composition（trace\_observer 给查询路由 + TraceAppService）

### 步骤 9：入站适配器 + 测试适配

**文件**：`src/adapter/inbound/http/adapters.rs`、`tests/inbound_adapter_test.rs`

* HTTP handler 调 `skill_port.execute_skill(&skill_name, &req.message, "").await`（空 trace\_id，装饰器生成）

* `chat_port.orchestrate` 不变（OrchestrateRequest.trace\_id 由装饰器设置，handler 不传）

* 测试文件适配新签名

## 关键设计决策

| 决策           | 选择                                   | 理由                                            |
| ------------ | ------------------------------------ | --------------------------------------------- |
| 事件总线         | 复用框架 EventBus                        | 框架已有 23 种事件 + 5 个 emit 点，零重复造轮子               |
| trace\_id 关联 | 方案 A（改 emit\_event 从 ctx.metadata 读） | 用户确认；9 个调用点都有 ctx；一次改彻底                       |
| trace\_id 穿透 | 改端口签名加 trace\_id 参数                  | 用户确认；显式、可追踪、编译器强制                             |
| span 存储时序    | store\_trace 与 span 写入解耦             | 避免同步等待异步 handler；get\_span\_tree 查询时 span 已完整 |
| bridge 生命周期  | 持久注册 EventHandler                    | 一次注册，按 trace\_id 归集不同请求事件                     |
| SpanData 放置  | application/observer.rs              | trace 属应用运维概念，不污染 domain                      |

## 风险与边界

1. **覆盖范围（已确认）**：orchestrator 路径（5 种事件）+ graph 路径（7 种 Flow/Graph 事件）本次覆盖。LLM/工具层事件（`LLMCalling`/`LLMResponded`/`ToolCalling`）**不做**——框架内无 emit 调用点，需改 LLM client，留作后续。领域专家内部步骤**不做**——领域层保持纯净。
2. **并发安全**：`SubhutiTraceObserverAdapter` 用 `std::sync::Mutex`（Port 方法同步签名），临界区极短（push/iter），OK。
3. **EventHandler async 注册**：`subscribe` 是 async，在 `init()`（async）里注册，OK。
4. **emit_event 改签名**：orchestrator 9 个调用点 + graph engine 6 个 + node_actor 2 个，全是 private 方法，改动安全隔离。
5. **端口签名变更影响面**：OrchestrationEnginePort + SkillExecutionPort + SkillPort（入站）+ AppService + 2 适配器 + HTTP handler + 测试，约 8 文件。
6. **graph 路径 trace_id 传递**：依赖 `dispatch_via_graph` 把 trace_id 注入 GraphState，graph engine/node_actor 从 state 读。若 graph 节点 spawn 子任务，state 需 clone 传递（已有 clone 模式）。

## 验证方式

1. **编译**：`cargo build`（零 warning）+ `cargo build --release`
2. **单测回归**：`cargo test`（9 个 inbound\_adapter\_test 全通过）
3. **端到端验证**（mock 模式）：

   * 启动服务 `make serve`（或 `cargo run -- serve`）

   * 发 chat 请求：`POST /subhuti/api/v1/chat` `{"message":"画个红色立方体"}`

   * 查 traces 列表：`GET /subhuti/api/v1/traces` — 确认有记录

   * 查 span 树：`GET /subhuti/api/v1/traces/:id/span_tree` — **确认返回嵌套 span**（user\_message → chain\_selected → agent\_started → agent\_completed），不再是 `null`

   * 确认 token 不再硬编码 0（若 LLMResponded 事件可见）
4. **日志验证**：启动时看到 `EventBus: handler subscribed` 日志含 `app_trace_bridge`

