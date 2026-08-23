//! # Subhuti Core
//!
//! 核心框架层：纯机制、接口定义、无业务、无第三方SDK依赖。
//!
//! ## 架构原则
//!
//! - **core**: 只定义规则和运行时机制，不含任何业务细节
//! - **infra**: 具体实现，对接第三方服务（LLM API、数据库、工具等）
//! - **应用层**: 业务实现，使用 core 接口和 infra 实现

pub mod common;
pub mod component;
pub mod engine;
pub mod event;
pub mod graph;
pub mod guardrails;
pub mod memory;
pub mod observe;
pub mod orchestrator;
pub mod runtime;
pub mod sutra_library;
pub mod vertical;

pub use common::types::CtxId;
pub use component::{
    Component, ComponentContext, ComponentLifecycle, ComponentRegistry, ComponentSlot,
    ComponentState, ExecutionContext, ExpertAgentAdapter, GraphNodeComponentAdapter,
    GuardrailAdapter, Pipeline, PipelineBuilder, ValidatorAdapter,
};
pub use engine::Subhuti;
pub use event::{
    bus::EventBus,
    handler::{EventFilter, EventHandler, EventSubscription},
    recorder::EventRecorder,
    types::{AgentEventData, ArcEvent, Event, EventMetadata},
};
pub use graph::{
    ActorAddr, ActorHandle, ActorHealth, ActorLifecycle, ActorStats, Checkpoint, CheckpointStore,
    ConditionalEdge, Edge, EventDrivenActor, EventDrivenScheduler, Graph, GraphBuilder, GraphError,
    GraphNode, GraphOutput, GraphState, GraphStructure, MemoryCheckpointStore, NodeActor, NodeFn,
    NodeMessage, NodeResult, Route, StateReducer, SupervisionStrategy, Supervisor,
};
pub use memory::Memory;
pub use observe::{
    record_fn_log, FnCallData, FnTracer, LogEntry, LogLevel, SpanData, TraceHandle,
    TraceObserverPort, TraceStatus,
};
pub use orchestrator::{
    execute_plan, generate_plan, parse_plan, Actor, ActorRegistry, AgentContext, AgentRegistry,
    DefaultDispatchRule, DefaultExecutionRule, DefaultTaskAnalysisRule, DispatchPlan, DispatchRule,
    DispatchStrategy, EventBusRef, ExecutionResult, ExecutionRule, ExpertAgent,
    ExpertAgentActorAdapter, ExpertState, FrameworkExpertInfo, FromState, GraphOrchestrator, Llm,
    MemoryRef, OrchestrationResult, Orchestrator, PlanStep, ResultStrategy, RuleConfig, RuleEngine,
    SkillPlan, Step, TaskAnalysisRule, TaskProfile, TokenUsage,
};
pub use runtime::{
    LLMConfig, LLMProvider, LLMResponse, Message, Role, Session, Tool, ToolCall, ToolCallResult,
    ToolInfo, ToolResponse, ToolResult, LLM,
};
pub use sutra_library::{EmptySutraLibrary, SutraLibraryPort};
pub use vertical::{
    Asset, AssetLibrary, ProjectInfo, ProjectMemory, ProjectNote, ToolCommand, ToolCommandInfo,
    ToolIntegration, ToolRegistry, Workflow, WorkflowStore,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Orchestrator: {0}")]
    Orchestrator(String),
    #[error("Graph: {0}")]
    Graph(String),
    #[error("Event: {0}")]
    Event(String),
    #[error("Memory: {0}")]
    Memory(String),
    #[error("Runtime: {0}")]
    Runtime(String),
    #[error("Expert: {0}")]
    Expert(String),
    #[error("Tool: {0}")]
    Tool(String),
    #[error("Context: {0}")]
    Context(String),
    #[error("修复失败: {0}")]
    FixFailed(String),
    #[error("补偿执行失败: {0}")]
    CompensationFailed(String),
    #[error("降级执行失败: {0}")]
    FallbackFailed(String),
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Any: {0}")]
    Any(#[from] anyhow::Error),
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum ErrorKind {
    Retryable,
    Fixable,
    Fatal,
}

pub trait ClassifiableError {
    fn kind(&self) -> ErrorKind;
}

impl ClassifiableError for Error {
    fn kind(&self) -> ErrorKind {
        match self {
            Error::Runtime(_) => ErrorKind::Retryable,
            Error::Tool(_) => ErrorKind::Retryable,
            Error::Memory(_) => ErrorKind::Retryable,
            Error::FixFailed(_) => ErrorKind::Fixable,
            Error::CompensationFailed(_) => ErrorKind::Fixable,
            Error::FallbackFailed(_) => ErrorKind::Fixable,
            Error::Graph(_) => ErrorKind::Fixable,
            Error::Expert(_) => ErrorKind::Fatal,
            Error::Orchestrator(_) => ErrorKind::Fatal,
            Error::Event(_) => ErrorKind::Retryable,
            Error::Context(_) => ErrorKind::Fixable,
            Error::Io(_) => ErrorKind::Retryable,
            Error::Serde(_) => ErrorKind::Fixable,
            Error::Any(_) => ErrorKind::Fatal,
        }
    }
}
