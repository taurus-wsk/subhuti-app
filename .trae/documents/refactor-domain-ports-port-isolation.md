# 重构方案：出站端口归位领域层 + OrchestratePort 接口隔离

## Context

上一轮重构拆分了 AppService（CompositionRoot + AppService），精简了 FrameworkInitPort。但出站端口（ExpertRepositoryPort 等）仍在应用层，返回应用层 DTO（ExpertInfo 等），不是领域实体——违反 DDD 的"出站端口属于领域层"原则。同时 OrchestratePort 有 9 个方法，每个 handler 只用其中 1-2 个却被迫依赖全部——违反接口隔离原则（ISP）。

本次重构目标：**DTO + 出站端口移到领域层**，**OrchestratePort 拆分为 3 个窄端口**（ChatPort/ExpertQueryPort/SkillPort），实现最纯粹的 DDD 分层。

## 重构后的依赖方向

```
表现层 → 应用层 → 领域层（纯，无外部依赖）
              ↑
基础设施 ─────┘（实现领域出站端口 + 应用层入站/初始化/观察者端口）
```

- **领域层**：DTO + 出站端口 + 领域 trait（完全独立，不依赖应用层/基础设施层）
- **应用层**：3 个窄入站端口 + 初始化端口 + 观察者端口 + AppService
- **基础设施层**：实现领域出站端口（SubhutiExpertRepository 等）
- **表现层**：AppState 持有 3 个窄入站端口 + 2 个观察者端口

## 关键设计决策

1. **不做跨层 re-export** — 导入路径即文档，`crate::domain::dto::ExpertInfo` 明确归属
2. **保留 SkillInfo 和 DomainSkill 双类型** — DomainSkill 是专家声明拥有的技能（无 expert_id），SkillInfo 是展开后的快照（带 expert_id/expert_name），语义不同
3. **观察者端口和 DTO 留应用层** — TraceHandle/SessionRecordParams 是观察者概念，非领域概念
4. **入站端口留应用层** — ChatPort/ExpertQueryPort/SkillPort 描述用例，是应用层职责

## 文件修改清单（按 Phase 排序）

### Phase 1：领域层新增（不影响现有代码）

| 步骤 | 文件 | 操作 |
|------|------|------|
| 1 | `src/domain/dto.rs` | **新建** — 迁入 5 个 DTO（OrchestrateRequest/OrchestrateResponse/ExpertInfo/SkillInfo/SkillResponse），保留 derive |
| 2 | `src/domain/ports.rs` | **新建** — 迁入 3 个出站端口（ExpertRepositoryPort/OrchestrationEnginePort/SkillExecutionPort），引用 domain::dto 和 domain::traits |
| 3 | `src/domain/mod.rs` | **修改** — 增加 `pub mod dto; pub mod ports;` |

### Phase 2：应用层重构（核心改动）

| 步骤 | 文件 | 操作 |
|------|------|------|
| 4 | `src/application/ports.rs` | **重写** — 删除迁出的 5 DTO + 3 出站端口；删除 OrchestratePort；新增 ChatPort(2 方法)/ExpertQueryPort(4 方法)/SkillPort(3 方法)；保留观察者 DTO + FrameworkInitPort + TraceObserverPort + SessionObserverPort |
| 5 | `src/application/mod.rs` | **修改** — re-export 改为 ChatPort/ExpertQueryPort/SkillPort；删除对 OrchestratePort/ExpertInfo 的 re-export |
| 6 | `src/application/app_service.rs` | **修改** — import 从 domain::dto/domain::ports 引入；impl 块拆为 3 个（ChatPort/ExpertQueryPort/SkillPort），方法逻辑零修改 |

### Phase 3：基础设施层（更新 import 路径）

| 步骤 | 文件 | 操作 |
|------|------|------|
| 7 | `src/infrastructure/adapters/mod.rs` | **修改** — framework_to_app_expert 的 ExpertInfo/SkillInfo 改从 domain::dto 引入 |
| 8 | `src/infrastructure/adapters/subhuti_expert_repository.rs` | **修改** — ExpertInfo/ExpertRepositoryPort 改从 domain::{dto,ports} 引入 |
| 9 | `src/infrastructure/adapters/subhuti_orchestration_engine.rs` | **修改** — ExpertInfo/OrchestrateResponse/OrchestrationEnginePort 改从 domain::{dto,ports} 引入 |
| 10 | `src/infrastructure/adapters/subhuti_skill_executor.rs` | **修改** — SkillInfo/SkillResponse/SkillExecutionPort 改从 domain::{dto,ports} 引入 |
| 11 | `src/infrastructure/adapters/subhuti_framework_initializer.rs` | **修改** — AppAdapters 字段类型改从 domain::ports 引入；FrameworkInitPort 仍从 application 引入 |

### Phase 4：表现层

| 步骤 | 文件 | 操作 |
|------|------|------|
| 12 | `src/presentation/http/routes/mod.rs` | **修改** — AppState 从 3 字段改为 5 字段（chat_port/expert_query_port/skill_port/trace_observer/session_observer） |
| 13 | `src/presentation/http/adapters.rs` | **修改** — 9 处 `state.orchestrate_port.X` 改为对应窄端口；HttpAdapterFactory 改为 5 参数；import 调整（OrchestrateRequest 从 domain::dto 引入） |
| 14 | `src/presentation/http/routes/experts.rs` | **修改** — 3 处 `state.orchestrate_port.X` 改为 `state.expert_query_port.X` |
| 15 | `src/presentation/http/server.rs` | **修改** — factory 接线改为 5 参数（3 端口 + 2 观察者） |

### Phase 5：测试

| 步骤 | 文件 | 操作 |
|------|------|------|
| 16 | `tests/route_adapter_test.rs` | **修改** — MockOrchestratePort 拆为 MockApp 实现 3 trait；create_test_app_state 注入 5 字段；import 从 domain::dto 引入 DTO |
| 17 | `tests/e2e_smoke_test.rs` | **修改** — setup 注入 3 端口；import 调整 |

## Handler 调用方式变化

| handler | 原调用 | 新调用 |
|---------|--------|--------|
| chat_handler | `state.orchestrate_port.orchestrate()` | `state.chat_port.orchestrate()` |
| chat_stream_handler | `state.orchestrate_port.orchestrate_stream()` | `state.chat_port.orchestrate_stream()` |
| orchestrate_handler | `state.orchestrate_port.orchestrate()` | `state.chat_port.orchestrate()` |
| orchestrate_analyze_handler | `state.orchestrate_port.analyze_task()` | `state.expert_query_port.analyze_task()` |
| orchestrate_match_handler | `state.orchestrate_port.match_expert()` | `state.expert_query_port.match_expert()` |
| orchestrate_experts_handler | `state.orchestrate_port.list_experts()` | `state.expert_query_port.list_experts()` |
| skill_execute_handler | `state.orchestrate_port.execute_skill()` | `state.skill_port.execute_skill()` |
| skill_list_handler | `state.orchestrate_port.skill_list()` | `state.skill_port.skill_list()` |
| skill_stream_handler | `state.orchestrate_port.execute_skill_stream()` | `state.skill_port.execute_skill_stream()` |
| experts_list_handler | `state.orchestrate_port.list_experts()` | `state.expert_query_port.list_experts()` |
| experts_active_handler | `state.orchestrate_port.active_expert()` | `state.expert_query_port.active_expert()` |
| experts_match_handler | `state.orchestrate_port.match_expert()` | `state.expert_query_port.match_expert()` |

## server.rs 组装方式变化

```rust
// 重构后
let app_service = Arc::new(CompositionRoot::build(&app_config).await?);

let chat_port: Arc<dyn ChatPort> = app_service.clone();
let expert_query_port: Arc<dyn ExpertQueryPort> = app_service.clone();
let skill_port: Arc<dyn SkillPort> = app_service;

let trace_observer: Arc<dyn TraceObserverPort> = Arc::new(SubhutiTraceObserverAdapter::new());
let session_observer: Arc<dyn SessionObserverPort> = Arc::new(SubhutiSessionObserverAdapter::new());

let factory = HttpAdapterFactory::new(
    chat_port, expert_query_port, skill_port,
    trace_observer, session_observer,
);
let app_state = factory.create_app_state();
```

## 验证步骤

1. `cargo check` — 编译通过
2. `cargo check --tests` — 测试编译通过
3. `cargo test` — 全部测试通过（35+ 个测试）
4. `make serve-debug` + `make orch-experts` — 启动服务验证专家列表正常
5. `curl POST /orchestrate` — 验证调度正常
6. `curl GET /skills` — 验证技能列表正常
7. `curl GET /experts` — 验证专家查询正常

## 编译风险注意事项

1. **循环依赖**：domain/ports.rs 只引用 domain::dto 和 domain::traits，不引用 application/infrastructure，无循环
2. **Phase 2-3 必须一起完成**：应用层和基础设施层互相依赖，单独完成一个 Phase 无法编译通过
3. **OrchestrateResponse 全限定路径**：测试中 `subhuti_app::application::OrchestrateResponse` 路径失效，需改为 `subhuti_app::domain::dto::OrchestrateResponse`
4. **mpsc 留应用层**：orchestrate_stream/execute_skill_stream 的 `mpsc::Receiver` 留在应用层端口定义中，不移到 domain（domain 不依赖 tokio）
5. **serde_json::Value 在领域层**：analyze_task 返回 serde_json::Value，domain 依赖 serde_json（可接受，无状态序列化库）
