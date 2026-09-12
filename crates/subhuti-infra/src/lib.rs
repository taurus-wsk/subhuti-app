//! # Subhuti Infrastructure
//!
//! 基础设施适配层：具体实现，对接第三方服务。
//!
//! ## 架构原则
//!
//! - **core**: 只定义规则和运行时机制
//! - **infra**: 具体实现，对接第三方服务（LLM API、数据库、工具等）
//! - **应用层**: 业务实现，使用 core 接口和 infra 实现

pub mod config;
pub mod data_dir;
pub mod llm;
pub mod memory;
pub mod sutra_library;
pub mod trace_store;
pub mod vertical;

pub use config::{
    BigFive, EmotionalTendency, FeedbackType, FlowConfig, FlowTemplate, InteractionStats,
    RuntimeConfig, SoulProfile, SubhutiConfig, TokenUsage, ToneStyle,
};
pub use llm::{
    CacheStats, CachedLLM, DoubaoClient, DoubaoConfig, MockLLM, OllamaClient, OllamaConfig,
    OpenAIClient, OpenAIConfig, ZhipuClient, ZhipuConfig,
};
pub use memory::{
    BaseStats, ConnectionDynamics, ConvoExchange, ConvoMiner, DedupConfig, DedupResult,
    Deduplicator, EmbeddingConfig, EmbeddingService, Entity, EntityExtractor, EntityRegistry,
    EntitySource, EntityType, FactChecker, FactIssue, IssueType, KeepStrategy, KnowledgeMemory,
    LayerOutput, LongTermMemory, Memory, MemoryConfig, MemoryItem, MemoryLayer, MemoryLayerConfig,
    MemoryStack, MemoryStats, MemoryStore, MinedMemory, SearchResult, SemanticSearchResult,
    ShortTermMemory,
};
pub use vertical::{
    MemoryAssetLibrary, MemoryProjectMemory, MemoryToolRegistry, MemoryWorkflowStore,
};
