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
pub mod debug;
pub mod llm;
pub mod memory;
pub mod tool;
pub mod vertical;

pub use config::{
    BigFive, EmotionalTendency, FeedbackType, FlowConfig, FlowTemplate, InteractionStats,
    RuntimeConfig, SoulProfile, SubhutiConfig, TokenUsage, ToneStyle,
};
pub use debug::{
    assert_with_context, debug_print, diagnose_value, measure_time, HealthReport, HealthStatus,
    LockDetector, Profiler, TestTracker,
};
pub use llm::{
    CacheStats, CachedLLM, DoubaoClient, DoubaoConfig, MockLLM, OllamaClient, OllamaConfig,
    OpenAIClient, OpenAIConfig, ZhipuClient, ZhipuConfig,
};
pub use memory::{
    BaseStats, ConnectionDynamics, ConvoExchange, ConvoMiner, Database, DatabaseStore, DbConfig,
    DedupConfig, DedupResult, Deduplicator, DefaultMemory, EmbeddingConfig, EmbeddingService,
    Entity, EntityExtractor, EntityRegistry, EntitySource, EntityType, FactChecker, FactIssue,
    FeedbackRow, Hallway, HistoryRow, HybridSearchResult, HybridSearcher, IssueType, KeepStrategy,
    KnowledgeGraph, KnowledgeGraphStats, KnowledgeMemory, LayerOutput, LongTermMemory, Memory,
    MemoryConfig, MemoryItem, MemoryLayer, MemoryLayerConfig, MemoryRow, MemoryStack, MemoryStats,
    MemoryStore, MinedMemory, PalaceGraph, PalaceGraphStats, PersonaData, PersonaRow,
    QueryDirection, Room, SearchResult, SemanticSearchResult, ShortTermMemory, SqliteMemoryStore,
    Triple, Tunnel, Wing,
};
pub use tool::{CalculatorTool, FileReadTool, FileWriteTool, WeatherTool, WebSearchTool};
pub use vertical::{
    MemoryAssetLibrary, MemoryProjectMemory, MemoryToolRegistry, MemoryWorkflowStore,
};
