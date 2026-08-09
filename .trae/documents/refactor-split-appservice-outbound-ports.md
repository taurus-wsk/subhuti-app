# 重构方案：拆分 AppService + 出站端口归位

## Context

当前 `AppService` 同时承担"组合根"（组装依赖 7 步）和"应用服务"（impl OrchestratePort）两个职责，是 God Method。`FrameworkInitPort` 通过 `get_expert_repository()` / `get_orchestration_engine()` / `get_skill_executor()` 三个方法把运行时出站端口"掏出来"，让初始化端口承担了工厂职责，概念混乱。`ConfigRepositoryPort` 是死代码（仅有定义，无实现无调用）。

本次重构目标：拆分 AppService → CompositionRoot + AppService，精简 FrameworkInitPort，删除死代码，重新归类端口分区。**不改变业务逻辑**，出站端口返回应用层 DTO 的现状留到后续步骤处理。

## 设计方案

### 重构后的依赖方向

```
server.rs / tests
    │
    ▼
CompositionRoot（组合根，知道所有具体类型）
    │ ├── 创建 DomainRepository（PG/内存降级）
    │ ├── 创建 SubhutiFrameworkInitializer → 调用 FrameworkInitPort 方法
    │ ├── 调用 initializer.build_adapters() 获取 3 个出站端口
    │ └── AppService::new(adapters) → 返回纯应用服务
    │
    ▼
AppService（纯应用服务，只依赖端口接口）
    │ 持有: ExpertRepositoryPort + OrchestrationEnginePort + SkillExecutionPort
    │ 实现: OrchestratePort（9 个方法，逻辑零修改）
```

### 关键设计决策

**用 `build_adapters()` 替代 `get_xxx` + `subhuti()` getter**：

SubhutiFrameworkInitializer 新增 `build_adapters()` 方法，返回 `AppAdapters` 结构体（包含 3 个出站端口）。CompositionRoot 不需要直接接触 `Arc<Subhuti>`，也不需要 import 3 个适配器类型，只需调一个方法拿结果。`AppAdapters` 定义在 `subhuti_framework_initializer.rs` 中（基础设施层产物）。

```rust
// infrastructure/adapters/subhuti_framework_initializer.rs
pub struct AppAdapters {
    pub expert_repository: Arc<dyn ExpertRepositoryPort>,
    pub orchestration_engine: Arc<dyn OrchestrationEnginePort>,
    pub skill_executor: Arc<dyn SkillExecutionPort>,
}

impl SubhutiFrameworkInitializer {
    pub fn build_adapters(&self) -> AppAdapters {
        AppAdapters {
            expert_repository: Arc::new(SubhutiExpertRepository::new(self.subhuti.clone())),
            orchestration_engine: Arc::new(SubhutiOrchestrationEngine::new(self.subhuti.clone())),
            skill_executor: Arc::new(SubhutiSkillExecutor::new(self.subhuti.clone())),
        }
    }
}
```

## 文件修改清单（按修改顺序）

### 步骤 1：`src/application/ports.rs` — 删除死代码 + 精简端口 + 重新分区

- **删除** `ConfigRepositoryPort`（死代码，无实现无调用）
- **从 `FrameworkInitPort` 删除 3 个方法**：`get_expert_repository` / `get_orchestration_engine` / `get_skill_executor`
- **重新分区**：DTO（顶部）→ 入站 Port → 出站 Port → 初始化 Port → 观察者 Port
- **更新模块文档**：4 类端口（入站/出站/初始化/观察者），移除 ConfigRepositoryPort 描述

### 步骤 2：`src/infrastructure/adapters/subhuti_framework_initializer.rs` — 精简 + 新增 build_adapters

- **删除** `impl FrameworkInitPort` 中的 3 个 `get_xxx` 方法实现
- **删除** `new()` 的未使用参数 `_domain_repository: Arc<dyn DomainRepository>`
- **删除** 不再需要的 import（`ExpertRepositoryPort, OrchestrationEnginePort, SkillExecutionPort` 从 application::ports import 中移除，因为 build_adapters 内部用全路径引用）
- **新增** `AppAdapters` 结构体定义
- **新增** `pub fn build_adapters(&self) -> AppAdapters` 方法
- **清理** `use crate::domain::traits::{DomainExpert, DomainRepository}` → 只保留 `DomainExpert`（`DomainRepository` 不再被 new 参数使用，但 register_expert 仍需要）

### 步骤 3：`src/application/app_service.rs` — 精简为纯应用服务

- **删除** `build()` 方法（7 步初始化逻辑，搬到 CompositionRoot）
- **删除** `orchestrate_port()` 方法（死代码，全代码库零调用）
- **删除** 基础设施层 import：`SubhutiFrameworkInitializer`, `PostgresRepository`, `InMemoryRepository`, `AppConfig`, `DomainRepository`, `FrameworkInitPort`, `experts`
- **新增** `pub fn new(expert_repository, orchestration_engine, skill_executor) -> Self`
- **保留** 3 个出站端口字段 + `impl OrchestratePort` 的 9 个方法（逻辑零修改）
- **更新** 模块文档：从"组合根"改为"纯应用服务"

### 步骤 4：`src/application/composition_root.rs` — 新增组合根

- 新建文件，搬移原 `AppService::build` 的步骤 1-6（仓库创建、框架初始化、注册专家、设置规则、注册图）
- 步骤 7 改为：`let adapters = framework_initializer.build_adapters();`
- 调用 `AppService::new(adapters.expert_repository, adapters.orchestration_engine, adapters.skill_executor)`
- 返回 `AppService`

### 步骤 5：`src/application/mod.rs` — 注册新模块

- 新增 `pub mod composition_root;`
- 新增 `pub use composition_root::CompositionRoot;`
- 保留 `pub use app_service::AppService;`（CompositionRoot::build 返回类型需要 pub）
- 更新模块文档

### 步骤 6：`src/presentation/http/server.rs` — 更新调用方

- `use crate::application::AppService` → `use crate::application::CompositionRoot`
- `AppService::build(&app_config)` → `CompositionRoot::build(&app_config)`

### 步骤 7：`tests/e2e_smoke_test.rs` — 更新测试调用方

- `use subhuti_app::application::{AppService, OrchestratePort}` → `use subhuti_app::application::{CompositionRoot, OrchestratePort}`
- `AppService::build(&cfg)` → `CompositionRoot::build(&cfg)`

### 步骤 8：文档注释更新

更新以下文件中提及 `AppService::build()` 或"AppService 作为组合根"的注释：
- `src/lib.rs` — 架构图
- `src/domain/mod.rs` — "应用层（AppService）作为组合根"
- `src/infrastructure/graphs/mod.rs` — "在 AppService::build() 中调用"
- `src/infrastructure/rules/mod.rs` — "通过 AppService::build() 注入"

## 验证步骤

1. `cargo check` — 编译通过
2. `cargo check --tests` — 测试编译通过
3. `cargo test --test e2e_smoke_test` — E2E 冒烟测试通过（验证组合根装配 + Port 接线 + 框架调度全链路）
4. `cargo test --test route_adapter_test` — 路由适配器测试通过（MockOrchestratePort 路径不受影响）
5. `cargo test --test repository_integration` — 仓库集成测试通过
6. `make serve-debug` + `make orch-experts` — 启动服务验证专家列表正常返回
7. `cargo clippy` — 无新警告

## 已知限制（本次不处理）

- `ExpertRepositoryPort.register()` 方法无调用方（注册走 FrameworkInitPort::register_expert），留到后续"出站端口返回领域实体"步骤清理
- 出站端口仍返回应用层 DTO（ExpertInfo 等），未改为返回领域实体——这是后续步骤
- `OrchestratePort` 仍是宽端口（9 个方法），接口隔离留到后续步骤
