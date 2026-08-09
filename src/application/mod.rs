//! # 应用层
//!
//! 编排领域专家与 subhuti 框架，组装应用实例。
//!
//! ## 职责
//!
//! - **组合根**（`CompositionRoot`）：创建 Subhuti 实例、注册领域专家、设置规则、装配出站端口
//! - **应用服务**（`AppService`）：纯应用服务，持有领域出站端口、实现 3 个入站窄端口
//!
//! ## 入站端口（Driving Port，入站适配层调用）
//!
//! - `ChatPort`：聊天/编排调度
//! - `ExpertQueryPort`：专家查询
//! - `SkillPort`：技能操作
//!
//! ## 观察者端口
//!
//! - `TraceObserverPort`、`SessionObserverPort`（入站适配层中间件使用，定义在 [`observer`] 模块）
//!
//! ## 与其他层的关系
//!
//! - 领域 DTO 在 `domain::dto`（ExpertInfo, SkillInfo, OrchestrateRequest/Response, SkillResponse）
//! - 领域出站端口在 `domain::ports`（ExpertRepositoryPort, OrchestrationEnginePort, SkillExecutionPort）
//! - 图编排：在出站适配层（`adapter/outbound/graphs/`）配置

pub mod app_service;
pub mod composition_root;
pub mod observer;
pub mod ports;
pub mod trace_decorator;

pub use app_service::AppService;
pub use composition_root::CompositionRoot;
pub use observer::{
    SessionObserverPort, SessionRecordParams, SpanData, TraceHandle, TraceObserverPort, TraceStatus,
};
pub use ports::{ChatPort, ExpertQueryPort, SkillPort, StreamEvent};
pub use trace_decorator::TraceAppService;
