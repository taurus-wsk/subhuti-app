# 观测性数据丢失修复 + 端到端冒烟测试

## Context

当前项目（六边形架构 Rust 应用）存在两个核心验证盲区：

1. **观测性数据全丢**：`/traces` 和 `/sessions` 路由无论怎么调都返回空。根因在框架层——`crates/subhuti/src/lib.rs:70-97` 的 `TraceObserver`/`SessionObserver` 全是空 stub（`store_trace` 是 `{}`、`list_summaries` 返回 `Vec::new()`）。应用层的 `SubhutiTraceObserverAdapter`（`src/infrastructure/adapters/observer_adapters.rs:46-53`）把数据转发给空实现，还在转换时丢了所有字段（`Trace::new("","","")`）。`TraceHandle` 的 `complete_success(_output, ...)` 把 output 参数丢弃，`message` 字段（`ports.rs:305`）触发 dead_code warning。

2. **主链路零集成测试**：HTTP→AppService→Port 全链路没有任何集成测试。`tests/route_adapter_test.rs` 用 MockOrchestratePort 只测路由层，不经过 AppService 组合根。框架升级 / Port 签名改动 / 组合根装配错误都会"编译过、运行炸"。

**框架层 stub 不动**（那是另一个工程，且流式问题已确认同源）。观测性在**应用层自包含修复**：让 observer adapter 持有自己的内存存储，不再转发给空框架。端到端测试用真 AppService（test_mode 走 MockLlmClient + InMemoryRepository，无外部依赖）。

**协同点**：一个端到端测试同时验证两条链路——发一条 orchestrate 请求触发 `TraceSessionLayer` 记录，再查 `/traces` + `/sessions` 断言落库。

## 实现步骤（先工作二，后工作一）

### 工作二：修复观测性数据丢失（应用层自包含）

#### W2-1 扩展 TraceHandle（`src/application/ports.rs`）
- 在 `TraceHandle`（ports.rs:301-307）上方新增枚举：
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum TraceStatus { InProgress, Success, Failed }
  ```
- struct 追加字段：`output: Option<String>`、`error: Option<String>`、`duration_ms: Option<u64>`、`status: TraceStatus`
- `new()`（ports.rs:311）初始化新字段为 None/InProgress
- `complete_success`（ports.rs:322）去掉参数 `_` 前缀，真存 `output`/`duration_ms`/`status=Success`
- `complete_failed`（ports.rs:333）同理存 `error`/`duration_ms`/`status=Failed`
- 新增只读访问器：`user_id()`/`session_id()`/`message()`/`output()`/`error()`/`duration_ms()`/`status()` —— 消除 `message` dead_code warning
- `TraceStatus` derive `Copy`（访问器返回值类型，避免借用纠缠）；TraceHandle 不需要 Clone（store_trace 是 move 语义）

#### W2-2 SubhutiTraceObserverAdapter 自包含存储（`src/infrastructure/adapters/observer_adapters.rs`）
- 移除 `use subhuti::observe::{TraceObserver, SessionObserver, session}`，改 `use std::sync::Mutex`
- struct（observer_adapters.rs:20）改为持 `traces: Mutex<Vec<TraceHandle>>`，`new()` 初始化空 Vec
- `create_trace`（:34）不再从框架取 trace_id，改应用层 `uuid::Uuid::new_v4().to_string()`（Cargo.toml:62 已有 uuid 依赖）
- `store_trace`（:46）直接 `guard.push(trace)`，不再调框架空实现
- `list_summaries`（:55）从 Vec 读转 JSON，**字段名必须含 `total_duration_ms`**（traces.rs:164 HTML 模板硬编码读这个 key，用 `duration_ms` 会让 HTML 显示 "-"）
- `get_trace`（:59）按 `trace_id` 查，JSON 字段集覆盖 traces.rs:152-199 读取的所有 key：`id`/`user_id`/`session_id`/`input`/`total_duration_ms`/`chain_name`/`expert_chain`/`status`/`output`
- `get_span_tree`（:75）保持返回 `None`（框架本就空，无必要）

#### W2-3 SubhutiSessionObserverAdapter 自包含存储（同文件）
- struct（:83）改为持 `sessions: Mutex<Vec<SessionRecordParams>>`，`SessionRecordParams` 已 derive Clone（ports.rs:369）
- `record_request`（:96）直接 push，不再转框架类型
- `list_sessions`（:116）/`get_session`（:120）从 Vec 读转 JSON，字段名自由（sessions.rs 无 HTML 约束）

#### W2-4 验证 server.rs 兼容
- `server.rs:79-80` 的 `SubhutiTraceObserverAdapter::new()` / `SubhutiSessionObserverAdapter::new()` 构造签名不变，**无需改动**

### 工作一：端到端冒烟测试

#### W1-1 新建 `tests/e2e_smoke_test.rs`
- 构造 test_mode AppConfig：`default_config()` + `test_mode.enabled=true` + `test_mode.mock_delay_ms=0`（默认 3000，config.rs:201，保险设 0）
- 组装真 AppService：`AppService::build(&cfg)` → `orchestrate_port: Arc<dyn OrchestratePort>` → 真实 `SubhutiTraceObserverAdapter`/`SubhutiSessionObserverAdapter`（工作二已自包含）→ `HttpAdapterFactory::new` → `create_app_state()` → `RouteRegistry.build_service`
- 返回 `(RouterService, Arc<SubhutiTraceObserverAdapter>, Arc<SubhutiSessionObserverAdapter>)` 供双保险断言
- 复用 `route_adapter_test.rs` 的 `set_test_app_state`/`clear_test_app_state`（routes/mod.rs）+ `oneshot` 模式（更快、无线程竞争）
- **坑**：`set_test_app_state` 是线程局部（routes/mod.rs:51），oneshot 在当前线程 poll OK，**不要 `tokio::spawn` 跨线程**

#### W1-2 四条链路用例
1. `test_e2e_list_experts`（GET `/orchestrate/experts`）—— 最稳定，不依赖 LLM/调度，走 `repo.get_all()` 返回注册的 BlenderExpert，断言 200 + `total>=1`
2. `test_e2e_skill_list`（GET `/skills`）—— 走 `skill_executor.skill_list()` 聚合
3. `test_e2e_orchestrate`（POST `/orchestrate`）—— **不硬断言 200**，断言 `json["success"]` 字段存在。orchestrate 可能 500（RuleEngine 未匹配专家），但 `BusinessOutcome` 仍落库（adapters.rs:255-262 + 289-291），middleware 仍会 store_trace
4. `test_e2e_skill_execute`（POST `/skills/blender`）—— 同上，断言 success 字段存在

#### W1-3 观测性协同测试（一测覆盖两条链路）
`test_e2e_orchestrate_records_trace_and_session`：
- POST `/orchestrate` `{"message":"hello","user_id":"u1","session_id":"s1"}`
- GET `/traces` → 断言 `total==1` + `data[0].user_id=="u1"` + `data[0].input=="hello"`（证明 message 字段没丢，工作二核心修复点）
- GET `/sessions` → 断言 `total==1`
- **关键**：orchestrate 500 时仍落库（middleware.rs:316-320 走 complete_failed 分支仍调 store_trace），所以观测性测试不依赖 orchestrate 成功，能稳定通过

## 关键文件

- `src/application/ports.rs` —— TraceHandle 扩展 + TraceStatus 枚举 + 访问器
- `src/infrastructure/adapters/observer_adapters.rs` —— 两个 adapter 改 Mutex<Vec> 自包含存储
- `tests/e2e_smoke_test.rs` —— 新建，端到端冒烟测试
- 只读参考：`src/presentation/http/middleware.rs:301-344`（调用顺序）、`tests/route_adapter_test.rs`（复用辅助函数模式）、`src/presentation/http/server.rs:73-118`（生产组装方式）

## 坑清单

1. **字段名 `total_duration_ms`**：traces.rs:164 HTML 硬编码，不能用 `duration_ms`
2. **status 字符串含 "Success"**：traces.rs:186 `status_text.contains("Success")` 决定 CSS class。`format!("{:?}", TraceStatus::Success)` = `"Success"` ✓
3. **Mutex 用 `std::sync::Mutex` 非 `tokio::sync::Mutex`**：Port 方法是同步签名，不能 await；临界区极短（push/iter）
4. **poisoned mutex 用 `.expect("... mutex poisoned")`** 便于排错
5. **`mock_delay_ms=3000`** 默认值（config.rs:201）测试配置必须设 0
6. **不要跨线程 spawn**：`set_test_app_state` 线程局部，oneshot 在当前线程 OK
7. **orchestrate 500 仍落库**：协同测试稳定性的关键，不要因 orchestrate 失败就跳过观测性断言

## 验证

```bash
# 工作二完成后
cargo build          # 编译通过，message dead_code warning 消失

# 工作一完成后
cargo test --test e2e_smoke_test   # 全绿
cargo test                        # 全部测试（含原 route_adapter_test 24 个 + repository_integration 3 个）不回归

# 手动验证
cargo run -- serve --mock
curl -X POST localhost:8080/subhuti/api/v1/orchestrate -d '{"message":"hello"}'
curl localhost:8080/subhuti/api/v1/traces   # total>=1，字段非空
curl localhost:8080/subhuti/api/v1/sessions # total>=1
```
