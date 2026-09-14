# Subhuti 架构与数据流总览

> 版本：2026-09-13（含 rule_engine 精简、框架级 SessionContext 重构后的当前状态）
> 目标：讲清楚 `POST /subhuti/api/v1/orchestrate` 从请求到响应的完整数据流，以及专家内部「黑盒」是如何运作的。
>
> 说明：本文档基于真实代码逐文件核实（行号指向当前 working-tree），不是凭记忆写的。

---

## 〇、整体架构：六边形分层

```
┌──────────────────────────────────────────────────────────────────────┐
│                       入站适配 (inbound)                              │
│   HTTP (axum, adapters.rs)   │   MCP (mcp.rs, stdio JSON-RPC)          │
│   orchestrate / experts/match / analyze ...                           │
└───────────────┬──────────────────────────┬───────────────────────────┘
                │ chat_port                  │ chat_port（MCP 直调，不经 HTTP）
                ▼                            ▼
┌──────────────────────────────────────────────────────────────────────┐
│                     应用层 (application)                              │
│  TraceAppService(装饰器: trace_id/session_id)                          │
│  OrchestrationService(编排主入口 + 框架级上下文落盘)                    │
│  SessionManager(内存缓存 + 增量 flush)  ·  StreamRegistry(SSE 路由)    │
└───────────────┬──────────────────────────────────────────────────────┘
                │ orchestration_engine (把参数翻译成 AgentContext.metadata)
                ▼
┌──────────────────────────────────────────────────────────────────────┐
│                     领域层 (domain)  ← 端口在此定义                   │
│  SubhutiLlmAdapter · DomainExecutionContext · SessionContext(实体)    │
│  ports: ChatPort / ExpertQueryPort / SessionContextPort / Toolchain…  │
│  专家实现: RustExpert / BlenderExpert …（plan_and_execute 黑盒）      │
└───────────────┬──────────────────────────────────────────────────────┘
                │ ExpertAgent 接口
                ▼
┌──────────────────────────────────────────────────────────────────────┐
│  出站适配 (outbound)           框架核心 crate (subhuti-core)           │
│  DomainExpertAdapter          Orchestrator(三级路由)                  │
│  SubhutiFrameworkInitializer   Engine · Planner · EventBus            │
│  SqliteSessionContextAdapter  rule_engine(插件位, 已精简)             │
└──────────────────────────────────────────────────────────────────────┘
                │ 基础设施 (subhuti-infra)
                ▼
        SQLite(sessions/traces) · Postgres(默认关)
```

**启动装配顺序（composition_root.rs 关键顺序，顺序错会踩坑）：**
1. 建 LLM（provider → ContextLimit → Retry → Cached，由内到外）
2. 建 EventBus + TraceObserver（sqlite）
3. 建 Subhuti 引擎（`Arc`，全局唯一）
4. 建 SessionManager（**必须在注册专家之前**，否则专家拿不到上下文）
5. 通过 `SubhutiFrameworkInitializer` 把专家一个个注册进引擎（注册即 `actor_registry` 登记）
6. 注册 ProgressEventBridge 到 EventBus（框架动作事件 → SSE 阶段流）
7. 建 OrchestrationService，注入 session_manager
8. 起 HTTP / MCP 入站

> ⚠️ 已删除：Graph 图路由（M1c）、DispatchRule/ExecutionRule 两层（从未被主链路读取）、默认关键词表。详见「陆、路由 = 唯一真相」。

---

# 甲、/orchestrate 的完整数据流（框架主管视角）

> 一条消息「该给谁、怎么做」由**框架主管**决定；专家「内部怎么干活」对框架完全黑盒（见乙部分）。

## ① HTTP 入站：Accept 协商响应模式
`src/adapter/inbound/http/adapters.rs`
- `orchestrate_handler`（L375）按 `Accept` 头分流：
  - 含 `text/event-stream` → `orchestrate_sse`（L334），流式，逐 delta + 进度事件
  - 否则 → `orchestrate_json`（L256），等整条链路结束，一次性返回结构化 JSON
- 路由注册：`inventory::submit!`（L387），路径 `/subhuti/api/v1/orchestrate`
- 入参 `OrchestrateRequest`：`message / user_id / session_id / chain / graph / expert_id / workspace_folder / system_prompt`
- **MCP 不经 HTTP**：MCP 的 `subhuti_chat` 直调 `chat_port.orchestrate_stream`，复用同一 `ChatPort`，只是传输换成 JSON-RPC + `notifications/progress`。

## ② TraceAppService 装饰器：生成 trace_id / session_id
`src/application/trace_decorator.rs`
- 若请求未带 `trace_id`/`session_id`，在此生成（UUID）并下传
- 包成 `FnTracer`，串起整条链路的 tracing span 与 trace 落盘
- 调用底层 `ChatPort`，拿到 `StreamEvent` 流（流式）或 `OrchestrationResponse`（一次性）

## ③ OrchestrationService：记用户消息 + 开通道 + 调引擎
`src/application/orchestration_service.rs`
- `orchestrate_stream`（L210）是流式主路径：
  - `mgr.record_user(session_id, message)`（L241）：把本轮用户原句写进**框架级上下文**（内存）
  - 按 `trace_id` 订阅框架 `EventBus`（`ProgressEventBridge` 把 `AgentEventData` 路由进本请求的 `p_tx`），使框架事件也能汇入同一条 `ProgressEvent` 流
  - 创建 per-request `ProgressEvent` 通道 `(p_tx, p_rx)`（L231），注入 `AgentContext.progress`；专家经 `emit_step/emit_chunk/emit_ask` 与框架 `ProgressEventBridge`（EventBus→p_tx）汇入同一条流
  - 先发 `StreamEvent::Start` + 一个 `phase=analyze` 的 Step（L252，文案「分析任务中…」）—— 这是「框架在分析」的视觉效果，不是真做语义分析
  - 调 `engine.orchestrate(...)` 拿到结果
  - 成功时 `record_final_answer` + `flush`（L168，落盘）
- 一次性路径 `orchestrate`（L78 起）逻辑相同，只是把 `StreamEvent` 累积成最终 JSON（含 `duration_ms`、`trace_id`、`expert_chain`）

## ④ SubhutiOrchestrationEngine：翻译参数
`src/adapter/outbound/subhuti_orchestration_engine.rs`
- 实现领域层 `ChatPort`
- 把 `OrchestrateRequest` 的字段塞进 `AgentContext.metadata`：`expert_id / graph_name / trace_id / session_id / workspace_folder / system_prompt`
- 调 `subhuti.dispatch(ctx, state)`（进入框架核心）

## ⑤ 框架 Orchestrator：三级路由（主管只干「选人 + 排班」）
`crates/subhuti-core/src/orchestrator/mod.rs`
`dispatch`（L470）按三级决策：

1. **显式指定专家**（`metadata.expert_id`，L493）→ 直接 `dispatch_via_actor`，跳过全部路由
2. **graph_name**（L511）→ 当「专家 ID / 标签别名」用：命中专家 ID 或 tags 即直调，未命中回退主管编排
3. **主管编排** `dispatch_without_graph`（L637）→ 按标签打分分三路：
   - `relevant_actors` 命中 **1 个** → 单域快速路径，直调（不花 LLM 规划，保速度）
   - 命中 **≥2 个** → `dispatch_with_plan`（L684），**全架构唯一一次框架级 LLM**：把候选专家渲染成「技能清单」喂 Planner 拆成串行步骤，每步调一个专家，上一步输出拼进下一步 input
   - 命中 **0 个** → **不接管**：直接返回「未匹配到相关领域专家 + 本服务领域边界」的显式结果（`success=false`）。产品定位是「领域深度 + 多软件工作流」，不做泛化兜底，泛化/领域外问题不会被任意专家硬答

> 红线：标签打分里**禁止硬编码「优先 code/rust」**——否则 Blender / 写作类问题会被固定塞给编程专家。
> 设计本质：**框架主管不深度理解语义，只回答「这条消息该给谁」**，多数请求 0 次 LLM，多域请求 1 次。

`dispatch_via_actor`（L781）在真正执行前发 `AgentMatched` 事件（→ SSE `route` 阶段：「🧭 匹配专家」），然后 `actor.perform(ctx, state)`。

## ⑥ DomainExpertAdapter::run：装好专家执行环境
`src/adapter/outbound/domain_expert_adapter.rs`（`run`，L184）
- 从 `ExpertState` 取框架 LLM，包成 `SubhutiLlmAdapter`（L195，携带 trace_id/session_id/专家身份/**框架 SessionContext**）
- 把框架 `ctx.session` 历史转成 `DomainMessage` 注入 `DomainContext.history`（L217）
- 若本请求带 trace_id 且接了 EventBus，把 RustToolchain 单例用 `TracedToolchainAdapter`（L280）包一层，使工具调用透传成 SSE `tool` 阶段（**不污染全局单例**，否则并发请求 trace_id 互踩）
- 组装 `DomainExecutionContext`（L293）：一次性塞入 llm / engine_llm / repository / toolchain / file_system / command / progress_tx / event_bus / **session_context**
- 调 `domain_expert.run(exec_ctx)`（L339）→ 进入专家黑盒（见乙）

## ⑦ 专家执行（黑盒）→ ⑧ 结果回收
- 专家 `plan_and_execute` 产出最终文本（详见乙部分）
- `dispatch_via_actor` 收到 output，发 `AgentCompleted` 事件，把 `output` 写回 `ctx.session`（`mod.rs` L533）
- 回到 `OrchestrationService`：`record_final_answer` 去重（若专家已回流同内容）+ `flush` 增量落 SQLite

## ⑨ SSE 事件桥接：框架动作 → 阶段流
`src/adapter/outbound/event_bridge.rs`（`to_step`，L443）
- `AgentMatched` → `route`（🧭 匹配专家）
- `LLMCalling`（来自 SubhutiLlmAdapter / TracedEngineLlm）→ `think`（🤔 模型推理中）
- `ToolCalling`/`ToolResponded` → `tool`（🔧 调用工具）
- `MemoryRetrieved` → `retrieve`（📚 记忆召回）
- 专家内部用 `progress_tx` 推的 `step` JSON（带 phase + 真实专家名）直接转成 Step 事件（OrchestrationService `build_step`，L267）—— 不再硬编码 "rust-expert"

## ⑩ Done：总耗时回传
- 编排结束发 `StreamEvent::Done { output, meta }`，`meta` 含 `duration_ms`
- 一次性 JSON 在 `data.output` / `data.duration_ms` 返回；Postman 能看到 `duration_ms`

### 数据流：请求 → 响应（骨架）

```
HTTP/mcp → TraceAppService(trace/session) → OrchestrationService(record_user,开通道)
  → SubhutiOrchestrationEngine(塞 metadata) → Orchestrator.dispatch(三级路由)
  → dispatch_via_actor(AgentMatched→route) → DomainExpertAdapter.run(装环境)
  → 专家 plan_and_execute【黑盒: 见乙】→ output 回写 ctx.session
  → OrchestrationService(record_final_answer+flush) → Done(duration_ms)
        ↑ SSE 阶段流：analyze→run→route→[plan→think→edit→verify→tool…]
```

---

# 乙、专家级流程（黑盒内部）

> 对框架而言，专家是 `actor.perform()` 返回一段字符串；**内部怎么规划、怎么调 LLM、怎么写文件、怎么验证，框架一概不知**。下面拆开这个黑盒——它也分两层：框架层默认 `plan_and_execute`，和专家自定义实现（以 RustExpert 为例）。

## 入口：DomainExecutionContext 已备齐一切
专家拿到 `exec_ctx` 时，下列依赖都已注入（六边形出站适配的产物）：
- `llm`：SubhutiLlmAdapter（**所有专家 LLM 调用的唯一出口**）
- `engine_llm`：规划器用 LLM（包成 `TracedEngineLlm`，规划期也发 `think`）
- `toolchain` / `file_system` / `command`：工具能力
- `progress_tx` / `event_bus`：阶段流
- `session_context`：框架级上下文（历史读 + 记忆回流）

## 关键一环：SubhutiLlmAdapter 三件事合一
`domain_expert_adapter.rs`（`SubhutiLlmAdapter`，L442）
同一处完成三件事（之所以放这里，是因为它是专家 LLM 调用的唯一汇聚点）：
1. **历史注入**：把 `session_context` 里的历史前插到 messages（system 之后、本轮 user 之前），上限 `inject_limit`（默认 6 条，role+content 去重挡膨胀）
2. **记忆回流**：专家每次 LLM 产出后，把问答按 `ContextMessage{ role, content, source, expert_id }` 写回 `session_context` → 后续专家/查询接口都能读到（框架级，非专家私有）
3. **真流式透传**：逐 delta 推 `progress_tx`（首字即出，无假打字机）

## 默认 plan_and_execute（领域层通用骨架）
`src/domain/traits.rs`（`plan_and_execute`，L199）
1. **规划**：`generate_plan` 用 `engine_llm` 把请求拆成技能步骤（`PlanOrAsk::Plan`）
2. **主动提问**（Ask 分支，L248）：规划返回提问 → 阻塞等用户答复（`/ask-resolve` 投递），拼回输入再规划；`MAX_ASK_ROUNDS=2` 防无限循环，超限强制「直接给计划别问」
3. **空计划短路**（L306）：0 步骤（如问候）→ 退化为 LLM 直接对话（`phase=answer`，真流式）
4. **自适应执行链**（L345）：L1 常态执行 → L2 反馈重试 → L3 领域层降级兜底（`DomainLlmFallback`，与普通技能同上下文，绝不递归）
5. **待办清单**：先以 `- [ ]` 未完成态推给前端，执行中逐个打勾（`todo_state`）

## RustExpert 定制 plan_and_execute（实测阶段序列来源）
`src/domain/experts/rust_expert.rs`（约 L540 起）
这是一个「先勘察项目、再规划、再写代码、再编译验证」的专家，阶段手动 push（每个阶段对应一个 SSE `phase`）：

| 阶段 | phase | 做什么 | 关键代码 |
|---|---|---|---|
| 1 | `analyze` | 勘察项目：是否存在 Cargo.toml、搜 `.rs` 文件 | L569 |
| 2 | `plan` | LLM 生成 markdown 任务清单，推送 `plan` 事件 | L586 / L617 |
| 3 | `edit` | 生成代码并写入文件（含 `cargo init` 等） | L650 |
| 4 | `verify` | `cargo build` / clippy / 编译验证（`generate_with_verify`，L1332） | L1373 |
| 5 | `tool` | 工具调用前后由 `TracedToolchainAdapter` 发 `ToolCalling/ToolResponded` | adapter L280 |
| 6 | `fix` | 编译不过则改代码重试（L3 兜底） | 内部 |

BlenderExpert 类似，但阶段是 `analyze→retrieve→think→done`（启动记忆召回走 `retrieve`）。

## 阶段 → SSE phase 映射（实测）

| 来源 | phase | 徽标 |
|---|---|---|
| OrchestrationService | `analyze` | 🔍 分析 |
| EventBridge(AgentMatched) | `route` | 🧭 匹配专家 |
| 专家 plan / LLMCalling | `plan` / `think` | 📋 / 🤔 |
| 专家 edit / verify | `edit` / `verify` | ✏️ / 🔍 |
| TracedToolchain | `tool`（×2，cargo check 前/后） | 🔧 |
| library_retrieve | `retrieve` | 📚 |

**实测 phase 序列：**
- Rust：`analyze → run → route → plan → think → edit → verify → tool×2 → done`
- Blender：`analyze → run → route → retrieve → think → done`

> 注意：`run` 是「进入框架 dispatch」，`route` 是路由结论，二者都来自框架；`plan/think/edit/verify/tool` 全部是专家内部事件。把整个序列当成「专家流程」是误解——前两段是主管，后段才是专家。

---

# 丙、框架级会话上下文（SessionContext，已重构）

层级（从内存到跨进程）：
```
SessionManager(内存缓存, 同 session_id 共享)
   └─ SessionContextPort(领域端口, 定义 load/append/clear)
        └─ SqliteSessionContextAdapter → SQLite(sessions.sqlite)
```
- 实体 `SessionContext`（`domain/session_context.rs`）：`ContextMessage{ role, content, source, expert_id }` + `to_llm_messages`
- 专家记忆经 `DomainExecutionContext.session_context` 回流，供**任意消费方**（下一轮、其他专家、`/experts/match`）使用——不再是适配器层私活
- 跨进程验证：进程 A 写入「秘密代号：子龙」→ 杀掉 → 全新进程 C 启动 → 追问「秘密代号是什么」→ 从 SQLite 读出答「子龙」✓
- 落盘行带专家归属：`source='Blender 动画专家'`、`expert_id='blender'`

---

# 丁、SSE 协议（6 类事件）

| 事件 | 字段 | 说明 |
|---|---|---|
| `start` | — | 流开始 |
| `step` | `message, source, phase, todo_state` | 阶段进度（框架/专家共用） |
| `data` | `content, done` | 真流式回答增量 |
| `ask` | `ask_id, question, options` | 主动提问（前端单选卡片） |
| `done` | `content, duration_ms, trace_id…` | 结束 + 总耗时 |
| `error` | `error` | 错误 |

---

# 戊、rule_engine 精简后：路由 = 唯一真相

`crates/subhuti-core/src/orchestrator/rule_engine.rs`（已 ~960 行精简到 ~150 行）
- 删：`DispatchRule`/`ExecutionRule`/`RuleEngine`/`DispatchPlan`/`TaskProfile` 旧关键词表等纯死代码
- 留：`TaskProfile`（查询端点响应结构）+ `TaskAnalysisRule` 插件位（**可替换**，默认不装）
- `Orchestrator::match_experts(input)` = dispatch 同款标签打分（pub 出来）
- `Orchestrator::analyze_task(input)`：`domain_tags` 直接从「专家 tags × 输入匹配」派生 → **与 dispatch 零漂移**
- HTTP `/experts/match`、`/orchestrate/analyze` 与 MCP `subhuti_match_expert` 看到的，就是 dispatch 实际会做的

验证（新二进制 8615）：
- 单域「教我用Blender建模」→ `match=['blender']`
- 多域「用 Rust 给 Blender 写导出插件」→ `['blender','rust-expert']`（按命中分排序）
- `analyze` 的 `domain_tags=['blender','rust']`（旧关键词表会映射成 coding，现直接给专家真实 tags）

---

# 己、设计决策备忘（为什么这样）

1. **注入选在 LLM 出口**：`SubhutiLlmAdapter` 是专家 LLM 调用唯一汇聚点，历史注入 + 记忆回流 + 真流式三件事只能放这，否则要改 N 处。
2. **历史去重**：`with_history` 按 role+content 去重，挡住落盘重复（编排层 record_user 与专家 push 各写一次用户原句）。
3. **MCP/HTTP 同源**：都直调 `chat_port`，不重复实现编排逻辑。
4. **无锁原则**：`Subhuti` 以 `Arc` 共享，编排主链路细粒度 `RwLock`；进度通道已移除全局 `PROGRESS_TX_REGISTRY`，改为 per-request 的 `AgentContext.progress` 注入（专家 + 框架事件桥汇入同一条 `ProgressEvent` 流），无跨请求可变状态，避免 `&mut self` 阻塞与误投。
5. **SQLite 同步桥**：独立 OS 线程 + current_thread runtime + `std::sync::mpsc`，禁 runtime 内 block_on。
6. **⚠️ SQLite PRAGMA 坑**：`PRAGMA table_info(?1)` 绑定参数会语法报错 → 迁移必须字面量拼表名 + 表名白名单。
7. **PG 探活默认关**：`SUBHUTI_PG_ENABLED=true` 才连，冷启动省 ~5s。
8. **路由红线**：标签打分禁止硬编码专家优先级；`match_expert`/`analyze_task` 复用 dispatch 打分（唯一真相）。
9. **token 成本采集（2026-09-13 新增）**：`LLM` trait 加 `chat_counted() -> (String, Option<u64>)`，
   领域事件加 `LlmResponded`，经 `SubhutiLlmAdapter` / `TracedEngineLlm` 发射 → 框架 `LLMResponded`
   → `SpanData.tokens` → `total_tokens()`。
   - **为何用 `chat_counted` 而不是改 `chat` 签名**：`chat()` 只返回 `String`，加返回值会波及所有调用方；
     新方法给默认实现（退化为 `chat()` 返回 `None`），不实现也不改变行为，向后兼容。
   - ⚠️ **装饰器必须转发**：`ContextLimitLLM` / `RetryLLM` / `CachedLLM` 三个包装层都要显式实现
     `chat_counted`，漏一个就会被 trait 默认实现截断、用量静默归零。
   - 命中 LLM 缓存时用量返回 `None`（没有真实调用发生，不该计成开销）。
   - 实测：一次真实请求 `+936 tokens`；43 项 MCP 探针跑完累计 21 条 `llm_response` span / 10150 tokens。
   - 残留：`trace_summaries` 未物化 `token_usage` 列（读时按 trace_id 从 `trace_spans` 求和）；流式路径未解析 usage。

---

## 复现命令

```bash
# 起服务（持久化到 /Users/hezenghui/sqlite）
cd /Users/hezenghui/RustroverProjects/subhuti-app
SUBHUTI_DATA_DIR=/Users/hezenghui/sqlite ./target/debug/subhuti serve

# 一次性编排
curl -s --noproxy '*' -X POST http://127.0.0.1:8615/subhuti/api/v1/orchestrate \
  -H 'Content-Type: application/json' \
  -d '{"message":"用 Rust 给 Blender 写个导出插件","session_id":"demo","user_id":"u"}'

# 流式（看 phase 序列）
curl -s --noproxy '*' -N -H 'Accept: text/event-stream' \
  -X POST http://127.0.0.1:8615/subhuti/api/v1/orchestrate \
  -H 'Content-Type: application/json' \
  -d '{"message":"教我用Blender做阵列建模","session_id":"demo2","user_id":"u"}'

# 路由预览（= 实际 dispatch）
curl -s --noproxy '*' -X POST http://127.0.0.1:8615/subhuti/api/v1/experts/match \
  -H 'Content-Type: application/json' -d '{"input":"用 Rust 给 Blender 写导出插件"}'
```
