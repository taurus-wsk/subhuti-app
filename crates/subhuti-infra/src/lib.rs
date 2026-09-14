//! # Subhuti Infrastructure
//!
//! 基础设施适配层：具体实现，对接第三方服务。
//!
//! ## 架构原则
//!
//! - **core**: 只定义规则和运行时机制
//! - **infra**: 具体实现，对接第三方服务（LLM API、数据库、工具等）
//! - **应用层**: 业务实现，使用 core 接口和 infra 实现

pub mod data_dir;
pub mod llm;
pub mod session_store;
pub mod sutra_library;
pub mod trace_store;

pub use llm::{
    CacheStats, CachedLLM, ContextLimitLLM, DoubaoClient, DoubaoConfig, LimitConfig, MockLLM,
    OllamaClient, OllamaConfig, OpenAIClient, OpenAIConfig, RetryConfig, RetryLLM, ZhipuClient,
    ZhipuConfig,
};
