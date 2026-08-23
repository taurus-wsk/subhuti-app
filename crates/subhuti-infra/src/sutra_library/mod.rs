//! # 藏经阁引擎（Sutra Library）
//!
//! 通用结构化记忆引擎，实现结构化记忆树 + BM25/tsvector 词法检索 + 轻量知识图谱。
//!
//! ## 架构
//!
//! - **物理双层**：内存（高速缓存）+ PostgreSQL（唯一持久）
//! - **逻辑冷热**：临时记忆 / 热记忆 / 冷记忆（动态流转）
//! - **结构三层**：记忆树（纵向）+ 词法检索（横向）+ 轻量图谱（关联）
//! - **领域插件化**：可插拔 Parser + Tokenizer

pub mod domain;
pub mod engine;
pub mod feedback;
pub mod hotness;
pub mod kb_schema;
pub mod models;
pub mod recall;
pub mod retrieval;
pub mod skill;
pub mod storage;
pub mod tree;

pub use engine::MemoryEnginePort;
pub use kb_schema::{
    ChatMessage, ChatSession, EngineRunLog, KbChunk, KgEntity, KgRelation, KnowledgeBase,
    KnowledgeTreeNode, TempKnowledgeBuffer, TreeChunkLink,
};
pub use models::{
    Collection, ContextMode, MemoryNode, RefEdge, RefType, RetrievalQuery, RetrievalResult,
    RetrievalSource, ScoredNode, SemanticChunk, Snapshot, SutraStats,
};
pub use recall::{
    base_search::BaseSearch, config::LibraryRetrieveConfig, graph::EntityGraph,
    graph::GraphPassageStrategy, pipeline::library_retrieve, space::SpaceDepthStrategy, Candidate,
    ChunkUuid, EntityUuid, RetrieveSource as RecallRetrieveSource,
};
pub use skill::MemorySkill;
use std::sync::Arc;
pub use storage::{MemoryStorage, PgGraphStorage, PgStorage};
pub use tree::{SlotTree, SlotTreeNode, TreeNodeKey, TreeValidator};

/// 构建藏经阁引擎的便利函数
pub fn create_sutra_engine(
    pg_pool: Option<sqlx::PgPool>,
) -> (std::sync::Arc<MemoryEnginePort>, MemorySkill) {
    let memory = std::sync::Arc::new(MemoryStorage::new());
    let pg = pg_pool.map(|p| std::sync::Arc::new(PgStorage::new(std::sync::Arc::new(p))));

    // 注册领域适配器
    use crate::sutra_library::domain::{
        blender::BlenderDomainParser, blender::BlenderDomainTokenizer,
        general::GeneralDomainParser, general::GeneralDomainTokenizer, rust::RustDomainParser,
        rust::RustDomainTokenizer, DomainRouter,
    };
    let mut router = DomainRouter::new();
    router.register(
        Box::new(RustDomainParser::new()),
        Box::new(RustDomainTokenizer::new()),
    );
    router.register(
        Box::new(BlenderDomainParser::new()),
        Box::new(BlenderDomainTokenizer::new()),
    );
    router.register(
        Box::new(GeneralDomainParser::new()),
        Box::new(GeneralDomainTokenizer::new()),
    );
    let domain_router = std::sync::Arc::new(router);

    let engine = std::sync::Arc::new(MemoryEnginePort::new(memory, pg.clone(), domain_router));

    // 初始化 PG 表（异步）
    if let Some(ref pg) = pg {
        let pg_clone = pg.clone();
        let pool = pg_clone.pool().clone();
        let engine_ref = engine.clone();
        tokio::task::spawn(async move {
            if let Err(e) = pg_clone.ensure_tables().await {
                tracing::warn!("SutraLibrary: PG table init failed: {}", e);
            }
            // 同时初始化图谱 PG 表并加载数据
            let pg_graph = PgGraphStorage::new(pool);
            if let Err(e) = pg_graph.ensure_tables().await {
                tracing::warn!("SutraLibrary: Graph PG table init failed: {}", e);
            }
            // 将 PG 持久化适配器注入到 EntityGraph
            engine_ref.init_graph_pg_async(Arc::new(pg_graph));

            // 初始化反馈分析器 PG 表
            engine_ref.feedback_analyzer().init_pg_tables().await;

            // 启动反馈分析器后台定时分析任务
            engine_ref.start_feedback_analysis();
        });
    } else {
        // 无 PG 时也启动后台分析（仅内存模式）
        engine.start_feedback_analysis();
    }

    let skill = MemorySkill::new(engine.clone());
    (engine, skill)
}
