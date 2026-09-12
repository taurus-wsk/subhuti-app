//! # Subhuti Core
//!
//! 核心框架层：纯机制、接口定义、无业务、无第三方SDK依赖。
//!
//! ## 架构原则
//!
//! - **core**: 只定义规则和运行时机制，不含任何业务细节
//! - **infra**: 具体实现，对接第三方服务（LLM API、数据库、工具等）
//! - **应用层**: 业务实现，使用 core 接口和 infra 实现（注：框架已无 Graph 编排，Workflow 下沉至专家内部）

pub mod common;
pub mod engine;
pub mod event;
pub mod memory;
pub mod observe;
pub mod orchestrator;
pub mod runtime;
pub mod sutra_library;
pub mod vertical;

pub use common::types::CtxId;
pub use engine::Subhuti;
pub use event::{
    bus::EventBus,
    handler::{EventFilter, EventHandler, EventSubscription},
    recorder::EventRecorder,
    types::{AgentEventData, ArcEvent, Event, EventMetadata},
};
pub use memory::Memory;
pub use observe::{
    record_fn_log, FnCallData, FnTracer, LogEntry, LogLevel, SpanData, TraceHandle,
    TraceObserverPort, TraceStatus,
};
pub use orchestrator::{
    execute_plan, execute_plan_adaptive, generate_plan, parse_plan, parse_plan_or_ask, Actor,
    ActorRegistry, AdaptiveOptions, AgentContext, AgentRegistry, AskRequest, BoxFuture,
    DefaultDispatchRule, DefaultExecutionRule, DefaultTaskAnalysisRule, DispatchPlan, DispatchRule,
    DispatchStrategy, EventBusRef, ExecutionResult, ExecutionRule, ExpertAgent,
    ExpertAgentActorAdapter, ExpertState, FrameworkExpertInfo, FromState, Llm, LlmToolFallback,
    MemoryRef, OrchestrationResult, Orchestrator, PlanOrAsk, PlanStep, ResultStrategy, RuleConfig,
    RuleEngine, SkillPlan, Step, StepFallback, TaskAnalysisRule, TaskProfile, TokenUsage,
    ToolExecutor,
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
    /// LLM 调用错误（结构化，便于上层决定是否重试）
    ///
    /// 之所以单独建模而不是继续用 `Any`：重试/降级需要**可靠地**判断
    /// 「这个错误重试有没有意义」。靠解析错误字符串太脆弱，所以在产生错误的
    /// 地方（HTTP 层）就把 `status` 与 `retryable` 标出来。
    #[error("LLM: {message}")]
    Llm {
        /// HTTP 状态码（若来自 HTTP 响应）
        status: Option<u16>,
        /// 是否可安全重试（超时 / 429 / 5xx 等临时性失败）
        retryable: bool,
        /// 面向人的错误描述
        message: String,
    },
}

impl Error {
    /// 构造结构化的 LLM 错误
    pub fn llm(status: Option<u16>, retryable: bool, message: impl Into<String>) -> Self {
        Self::Llm {
            status,
            retryable,
            message: message.into(),
        }
    }

    /// 该错误是否值得重试
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Llm { retryable, .. } => *retryable,
            other => matches!(other.kind(), ErrorKind::Retryable),
        }
    }

    /// 该错误是否由限流（HTTP 429）导致
    pub fn is_rate_limited(&self) -> bool {
        match self {
            Error::Llm {
                status, message, ..
            } => *status == Some(429) || message.contains("429"),
            _ => false,
        }
    }
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
            Error::Expert(_) => ErrorKind::Fatal,
            Error::Orchestrator(_) => ErrorKind::Fatal,
            Error::Event(_) => ErrorKind::Retryable,
            Error::Context(_) => ErrorKind::Fixable,
            Error::Io(_) => ErrorKind::Retryable,
            Error::Serde(_) => ErrorKind::Fixable,
            Error::Any(_) => ErrorKind::Fatal,
            Error::Llm { retryable, .. } => {
                if *retryable {
                    ErrorKind::Retryable
                } else {
                    ErrorKind::Fatal
                }
            }
        }
    }
}
