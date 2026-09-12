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
pub mod persistence;
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
pub use persistence::{PersistencePort, SqliteStorage};
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

    // ── 持久化后端选择 ─────────────────────────────────────
    // - 有 PG 池  → PG 后端（图谱、反馈分析均启用）
    // - 无 PG 池  → 降级 SQLite 文件持久化：核心节点/集合/知识库落盘，
    //              图谱与反馈分析退化为内存模式（可选能力，无 PG 不强求）。
    let pg: Option<std::sync::Arc<PgStorage>> =
        pg_pool.map(|p| std::sync::Arc::new(PgStorage::new(std::sync::Arc::new(p))));
    let persistence: Option<std::sync::Arc<dyn PersistencePort>> = if let Some(p) = &pg {
        Some(p.clone())
    } else {
        // 默认落在统一数据目录（~/.subhuti/data），避免相对路径随 cwd 漂移
        let db_path = crate::data_dir::sutra_db_path();
        if let Some(parent) = std::path::Path::new(&db_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match SqliteStorage::open(&db_path) {
            Ok(s) => {
                tracing::info!("SutraLibrary: 无 PG，降级使用 SQLite 持久化 ({})", db_path);
                Some(std::sync::Arc::new(s))
            }
            Err(e) => {
                tracing::warn!("SutraLibrary: SQLite 打开失败，退回纯内存模式: {}", e);
                None
            }
        }
    };
    // 反馈分析器专属 PG 存储（仅 PG 模式启用；无 PG 时走内存窗口模式）
    let feedback_pg = pg.clone();
    let feedback_pg_spawn = pg.clone();

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

    let engine = std::sync::Arc::new(MemoryEnginePort::new(
        memory,
        persistence.clone(),
        feedback_pg,
        domain_router,
    ));

    // 后台初始化（异步，不阻塞启动）：
    // 1. 核心持久化后端建表；2. PG 专属图谱 + 反馈表；3. 启动反馈分析后台任务
    let engine_ref = engine.clone();
    tokio::task::spawn(async move {
        if let Some(port) = &persistence {
            if let Err(e) = port.ensure_tables().await {
                tracing::warn!("SutraLibrary: 持久化表初始化失败: {}", e);
            }
        }
        if let Some(pg) = &feedback_pg_spawn {
            let pool = pg.pool().clone();
            let pg_graph = PgGraphStorage::new(pool);
            if let Err(e) = pg_graph.ensure_tables().await {
                tracing::warn!("SutraLibrary: Graph PG table init failed: {}", e);
            }
            engine_ref.init_graph_pg_async(Arc::new(pg_graph));
            engine_ref.feedback_analyzer().init_pg_tables().await;
        }
        // 启动反馈分析器后台定时分析任务（PG/内存 双模式均可）
        engine_ref.start_feedback_analysis();
    });

    let skill = MemorySkill::new(engine.clone());
    (engine, skill)
}
